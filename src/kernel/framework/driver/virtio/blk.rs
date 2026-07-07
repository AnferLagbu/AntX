//! VirtIO 块设备驱动
//!
//! 实现 VirtIO 块设备规范 (设备 ID 2).
//! 使用 split virtqueue 进行 I/O 提交与完成.
//!
//! 每次请求读/写单个 512 字节扇区.
//! 性能上可扩展为多扇区请求与多 outstanding I/O.

use super::queue::VirtQueue;
use super::{VirtioMmioDevice, VIRTIO_ID_BLOCK};
use crate::kernel::framework::driver::BlockDevice;
use crate::kernel::framework::mm::{KERNEL_BASE, PAGE_SIZE};
use crate::klog_info;
use crate::klog_warn;

#[cfg(target_arch = "x86_64")]
use crate::kernel::framework::idt::InterruptFrame;

// I-42: virtio-blk 默认 IRQ 号 (QEMU virt 机器分配, 启动探测时由设备配置覆盖).
// 暴露为 pub const 让 boot/PCI 探测代码能改写.
pub const DEFAULT_VIRTIO_BLK_IRQ: u8 = 11;

// I-42: 全局 ISR → device 查表, 支持多实例.
// 每个 IRQ 号最多映射一个 virtio-blk 设备实例.
// 当前 IDT 框架 register_irq 限制 IRQ < 16, 注册表大小与之对齐.
#[cfg(target_arch = "x86_64")]
const MAX_VIRTIO_BLK_IRQS: usize = 16;

/// I-42: 设备注册表条目: completion 数组 + 设备 MMIO 引用.
/// ISR 通过 IRQ 号索引到此条目, signal 完成事件并 ACK 设备.
#[cfg(target_arch = "x86_64")]
struct VirtioBlkRegistryEntry {
    completion: *const IoCompletionArray,
    device: *const VirtioBlk,
}

/// I-42: 全局设备注册表, IRQ 号 → 设备实例映射.
/// enable_irq() 注册, ISR 查表. 替代原先的单静态指针.
#[cfg(target_arch = "x86_64")]
static mut VIRTIO_BLK_REGISTRY: [Option<VirtioBlkRegistryEntry>; MAX_VIRTIO_BLK_IRQS] = {
    const NONE: Option<VirtioBlkRegistryEntry> = None;
    [NONE; MAX_VIRTIO_BLK_IRQS]
};

// I-42: 轻量级 I/O 完成事件, 替代原 do_io 内的 `loop { pop_used(); spin_loop() }` 忙等.
// 由 ISR (`virtio_blk_irq_handler`) 在设备通知 used ring 有新条目时 signal,
// do_io 在等待时只 spin_loop 检查此 flag, 避免无限空转.
// I-42: 多 outstanding I/O 完成事件数组, 按 request token (descriptor head index) 索引.
// 每个 token 对应一个 AtomicBool, ISR 根据 used ring entry 的 id 字段派发到对应 slot.
// 当前 do_io 仍串行 (每次只用 token 0), 但数据结构已支持并发提交.
use core::sync::atomic::{AtomicBool, Ordering};

/// VirtQueue 最大深度 (与 queue.rs VQ_SIZE 一致)
const VIRTIO_BLK_MAX_TOKENS: usize = 32;

/// 多 outstanding I/O 完成事件数组
pub struct IoCompletionArray {
    slots: [AtomicBool; VIRTIO_BLK_MAX_TOKENS],
}

impl IoCompletionArray {
    /// 创建全 false 的完成事件数组
    pub const fn new() -> Self {
        // AtomicBool 不支持 const fn 数组初始化, 用 core::mem::zeroed
        // SAFETY: AtomicBool 的零值等价于 AtomicBool::new(false)
        Self {
            slots: [const { AtomicBool::new(false) }; VIRTIO_BLK_MAX_TOKENS],
        }
    }
    /// ISR 路径: 标记指定 token 的 I/O 完成
    pub fn signal(&self, token: usize) {
        if token < VIRTIO_BLK_MAX_TOKENS {
            self.slots[token].store(true, Ordering::Release);
        }
    }
    /// 等待者: 检查指定 token 是否完成
    pub fn is_done(&self, token: usize) -> bool {
        if token < VIRTIO_BLK_MAX_TOKENS {
            self.slots[token].load(Ordering::Acquire)
        } else {
            false
        }
    }
    /// 提交新一轮 I/O 前重置指定 token
    pub fn reset(&self, token: usize) {
        if token < VIRTIO_BLK_MAX_TOKENS {
            self.slots[token].store(false, Ordering::Release);
        }
    }
    /// ISR 路径: 信号所有未完成 token (用于 ISR 无法确定 token 的退路场景)
    pub fn signal_all(&self) {
        for slot in &self.slots {
            slot.store(true, Ordering::Release);
        }
    }
}

/// VirtIO 块请求头 (小端).
#[repr(C)]
#[derive(Debug)]
struct BlkRequest {
    req_type: u32, // 0=读, 1=写
    reserved: u32,
    sector: u64, // LBA (小端)
}

/// VirtIO 块请求状态字节 (设备完成后写入).
const VIRTIO_BLK_S_OK: u8 = 0;
#[allow(dead_code)] // 规范定义, 待 I/O 错误处理路径启用后使用。
const VIRTIO_BLK_S_IOERR: u8 = 1;
#[allow(dead_code)] // 规范定义, 待不支持的请求类型处理启用后使用。
const VIRTIO_BLK_S_UNSUPP: u8 = 2;

// 请求类型
const VIRTIO_BLK_T_IN: u32 = 0; // 读
const VIRTIO_BLK_T_OUT: u32 = 1; // 写

// 配置空间偏移 (相对于 0x100)
const BLK_CONFIG_CAPACITY_LO: usize = 0x00;
#[allow(dead_code)] // 规范定义, 待 >2TB 块设备容量查询启用后使用。
const BLK_CONFIG_CAPACITY_HI: usize = 0x04;

/// 带 virtqueue 的 virtio-blk 设备.
pub struct VirtioBlk {
    /// MMIO 设备传输引用.
    pub device: VirtioMmioDevice,
    /// 用于 I/O 的单个 virtqueue.
    pub vq: VirtQueue,
    /// 以 512 字节扇区为单位的总容量 (来自配置空间).
    pub capacity_sectors: u64,
    /// 待处理 I/O 请求的 DMA 缓冲区 (从 PMM 分配).
    io_buffer: *mut u8,
    io_buffer_phys: u64,
    /// 最近一次已完成请求的状态字节.
    #[allow(dead_code)] // 待 I/O 错误处理路径启用后读取。
    status_byte: u8,
    /// I-42: 多 outstanding I/O 完成事件数组. ISR 按 token signal, do_io 等待指定 token.
    completion: IoCompletionArray,
    /// I-42: IRQ 是否已注册到 IDT. true = 走事件驱动, false = 退到原 spin-loop.
    irq_registered: bool,
}

// SAFETY: VirtioBlk 使用 PMM 分配的 DMA 缓冲区; 单一所有者 &mut self
// SAFETY: VirtioBlk 含 MMIO 裸指针, 但 &mut self 访问防止同一设备并发 I/O.
//         MMIO 写使用 volatile + 屏障以保证跨 CPU 可见性.
unsafe impl Send for VirtioBlk {}
// SAFETY: 同上, &mut self 保证排他访问.
unsafe impl Sync for VirtioBlk {}

impl VirtioBlk {
    /// 创建并初始化 virtio-blk 驱动实例.
    ///
    /// # Safety
    /// `device` 必须具有 device_id == VIRTIO_ID_BLOCK.
    pub fn new(device: VirtioMmioDevice) -> Option<Self> {
        if device.device_id != VIRTIO_ID_BLOCK {
            return None;
        }

        // 初始化传输层
        if device.init().is_err() {
            klog_warn!(
                Driver,
                "virtio-blk: device init failed at {:#x}",
                device.iomem.phys().as_u64()
            );
            return None;
        }

        // 分配 virtqueue
        let vq = VirtQueue::new(false)?; // x86_64 使用现代模式

        // 在设备上配置 virtqueue 0
        if device.setup_vq(0, &vq).is_err() {
            return None;
        }

        // 设置 DRIVER_OK — 队列配置完成后设备进入 live
        device.set_driver_ok();

        // 从配置空间读取容量
        let capacity = device.read_config64(BLK_CONFIG_CAPACITY_LO);

        // 分配 IO 缓冲区: 512 字节扇区数据 + 请求头 + 状态字节
        let buf_size = 512 + core::mem::size_of::<BlkRequest>() + 1;
        let buf_pages = buf_size.div_ceil(PAGE_SIZE as usize);
        unsafe extern "C" {
            fn pmm_alloc_pages(count: u64) -> *mut u8;
        }
        // SAFETY: extern 函数的参数/返回值类型与 C ABI 声明一致; 调用方保证指针有效
        let buf = unsafe { pmm_alloc_pages(buf_pages as u64) };
        if buf.is_null() {
            return None;
        }

        let buf_phys = buf as u64;
        let buf_virt = (buf_phys + KERNEL_BASE) as *mut u8;
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            core::ptr::write_bytes(buf_virt, 0, buf_size);
        }

        klog_info!(
            Driver,
            "virtio-blk: initialized, capacity={} sectors ({:.1} MB)",
            capacity,
            (capacity * 512) as f64 / (1024.0 * 1024.0)
        );

        Some(VirtioBlk {
            device,
            vq,
            capacity_sectors: capacity,
            io_buffer: buf_virt,
            io_buffer_phys: buf_phys,
            status_byte: VIRTIO_BLK_S_OK,
            completion: IoCompletionArray::new(),
            irq_registered: false,
        })
    }

    /// 读取单个扇区 (512 字节) 到 `buf`.
    pub fn read_sector(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), ()> {
        if buf.len() < 512 {
            return Err(());
        }
        self.do_io(lba, VIRTIO_BLK_T_IN, buf)
    }

    /// 从 `buf` 写入单个扇区 (512 字节).
    pub fn write_sector(&mut self, lba: u64, buf: &[u8]) -> Result<(), ()> {
        if buf.len() < 512 {
            return Err(());
        }
        self.do_io(lba, VIRTIO_BLK_T_OUT, buf)
    }

    /// I-42: 注册 virtio-blk IRQ 到 IDT.
    ///
    /// 调用后, do_io 走事件驱动路径: 提交后等待 `completion` 标志,
    /// 由 `virtio_blk_irq_handler` ISR signal. 失败 (例如 IDT 已满, IRQ 已被占用)
    /// 保留 irq_registered = false, do_io 自动退到原 spin-loop 退路.
    #[cfg(target_arch = "x86_64")]
    pub fn enable_irq(&mut self) -> Result<(), &'static str> {
        use crate::kernel::framework::idt::IdtManager;
        if self.irq_registered {
            return Ok(());
        }
        let irq = DEFAULT_VIRTIO_BLK_IRQ;
        // 注册到全局设备注册表 (IRQ → completion + device), ISR 查表使用.
        register_virtio_blk_device(irq as usize, &self.completion, self);
        let manager = IdtManager::instance();
        manager.register_irq(
            irq,
            virtio_blk_irq_handler,
            "virtio-blk",
            0, // flags
        )?;
        manager.enable_irq(irq);
        self.irq_registered = true;
        klog_info!(Driver, "virtio-blk IRQ {} registered", irq);
        Ok(())
    }

    /// I-42: aarch64 平台 virtio-blk 暂未实现 IRQ 路径, 直接报错.
    #[cfg(target_arch = "aarch64")]
    pub fn enable_irq(&mut self) -> Result<(), &'static str> {
        Err("virtio-blk IRQ not implemented for aarch64")
    }

    /// 执行单扇区 I/O 请求 (经 virtqueue).
    ///
    /// 使用链式描述符:
    ///   desc[0] = BlkRequest 头 (设备读)
    ///   desc[1] = 数据缓冲区 (IN 时设备写, OUT 时设备读)
    ///   desc[2] = 状态字节 (设备写)
    fn do_io(&mut self, lba: u64, req_type: u32, buf: &[u8]) -> Result<(), ()> {
        // ── 在 DMA 缓冲区构造请求 ──
        let req_size = core::mem::size_of::<BlkRequest>();
        let data_offset = req_size;
        let status_offset = data_offset + 512;

        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            // 填充请求头
            let req = &mut *(self.io_buffer as *mut BlkRequest);
            req.req_type = req_type.to_le();
            req.reserved = 0;
            req.sector = lba.to_le();

            // 写入时, 将数据复制到 DMA 缓冲区
            if req_type == VIRTIO_BLK_T_OUT {
                let dst = self.io_buffer.add(data_offset);
                core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, buf.len().min(512));
            }
        }

        // ── 准备描述符链 ──
        let desc_req = self
            .vq
            .prepare_desc(self.io_buffer_phys, req_size as u32, false); // 设备读头
        let desc_data = self.vq.prepare_desc(
            self.io_buffer_phys + data_offset as u64,
            512,
            req_type == VIRTIO_BLK_T_IN,
        ); // IN=设备写
        let desc_status = self
            .vq
            .prepare_desc(self.io_buffer_phys + status_offset as u64, 1, true); // 设备写状态

        // 链接链: req → data → status
        // SAFETY: 调用方保证指针/类型有效 (详见上下文)
        unsafe {
            (*self.vq.desc.add(desc_req as usize)).flags |= super::queue::VQ_DESC_F_NEXT;
            (*self.vq.desc.add(desc_req as usize)).next = desc_data;
            (*self.vq.desc.add(desc_data as usize)).flags |= super::queue::VQ_DESC_F_NEXT;
            (*self.vq.desc.add(desc_data as usize)).next = desc_status;
        }

        // ── Submit and kick ──
        // I-42: 重置此 token 的完成事件, 提交后才不会误判上一次完成的残留信号.
        let token = desc_req as usize;
        self.completion.reset(token);
        self.vq.submit(desc_req);
        self.vq.commit_and_kick();

        // 通知设备前确保写可见
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        self.device.notify(0);

        // ── 等待完成 (I-42: 事件驱动) ──
        //
        // 原实现: 紧接 `pop_used()` 自旋, 长时间空转浪费 CPU, 单核可能活锁.
        // 新实现: 优先等 `completion.done`, 由 `virtio_blk_irq_handler` ISR signal;
        //        若 irq_registered=false (测试/未注册) 或等不到 (timeout 退路),
        //        才进入原 pop_used spin-loop.
        //
        // I-42 timeout: 限制 ~10ms 等待 ISR, 之后降级为直接 pop_used.
        // 10ms 是 HDD 平均寻道时间, 在此期间 CPU 几乎零开销.
        if self.irq_registered {
            const EVENT_WAIT_BOUND: u32 = 10_000_000; // ~10ms @ 1 GHz spin_loop()
            let mut spins: u32 = 0;
            while !self.completion.is_done(token) && spins < EVENT_WAIT_BOUND {
                core::hint::spin_loop();
                spins = spins.saturating_add(1);
            }
            if !self.completion.is_done(token) {
                // 退路: IRQ 未触发 (设备异常), 转 spin-loop 直接 drain used ring.
                klog_warn!(
                    Driver,
                    "virtio-blk completion timeout after {} spins (token={}), falling back to poll",
                    spins,
                    token
                );
            }
        }

        // ── 排空 used ring ──
        // IRQ 路径下 `completion` 已 set, 这里一次 pop_used 就拿到结果;
        // poll 退路下走原 spin loop.
        loop {
            if let Some((_id, _len)) = self.vq.pop_used() {
                // 检查状态字节
                // SAFETY: `self` 由调用方保证为有效指针; 只读访问
                let status = unsafe { *self.io_buffer.add(status_offset) };
                self.vq.reclaim_desc(desc_status);
                self.vq.reclaim_desc(desc_data);
                self.vq.reclaim_desc(desc_req);

                if status != VIRTIO_BLK_S_OK {
                    return Err(());
                }

                // 读操作: 将数据从 DMA 缓冲区复制到用户缓冲区
                if req_type == VIRTIO_BLK_T_IN {
                    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                    let src = unsafe { self.io_buffer.add(data_offset) };
                    // SAFETY: 调用方保证指针/类型有效 (详见上下文)
                    unsafe {
                        core::ptr::copy_nonoverlapping(src, buf.as_ptr() as *mut u8, 512);
                    }
                }

                return Ok(());
            }
            // 自旋等待, 带 hint (仅在 IRQ 模式下不会到这里, 因 completion.is_done
            // 为 true 时 pop_used 必成功; poll 退路或未注册 IRQ 才走此分支).
            core::hint::spin_loop();
        }
    }
}

// I-42: virtio-blk ISR — 设备 used ring 写入后触发, signal 完成事件.
//
// ISR 不做 pop_used, 因为设备可能在事件产生后才把 status byte 写入 DMA,
// 而 do_io 接下来会自己 pop. 这里只把 "有完成" 这件事广播给等待者.
//
// 当前 IDT 框架的 IRQ handler 签名不传 IRQ 号, 因此 ISR 遍历注册表
// 查找已注册设备. 由于注册表最多 16 项且通常仅 1 个设备, 遍历开销可忽略.
// 多实例限制: 同一时刻只能有一个 virtio-blk 设备的 ISR 被分发到此函数,
// 因为 IDT 按 IRQ 号分发, 每个 IRQ 有独立的 handler 槽位.
#[cfg(target_arch = "x86_64")]
#[unsafe(no_mangle)]
pub extern "C" fn virtio_blk_irq_handler(frame: *mut InterruptFrame) {
    // SAFETY: 注册表由 enable_irq() 在启动时单线程写入, ISR 只读, 无数据竞争.
    unsafe {
        for i in 0..MAX_VIRTIO_BLK_IRQS {
            if let Some(ref entry) = VIRTIO_BLK_REGISTRY[i] {
                // signal 完成事件
                if !entry.completion.is_null() {
                    (*entry.completion).signal_all();
                }
                // ACK 设备中断 (VirtIO MMIO 规范要求)
                if !entry.device.is_null() {
                    (*entry.device).device.ack_interrupt();
                }
                return;
            }
        }
    }
    // 未找到注册设备 — 不应发生, 记录告警
    klog_warn!(Driver, "virtio-blk ISR fired but no device registered");
    let _ = frame; // 抑制未使用参数告警
}

// I-42: 注册设备到全局注册表 (IRQ → completion + device).
// enable_irq() 调用, ISR 查表使用.
#[cfg(target_arch = "x86_64")]
pub fn register_virtio_blk_device(irq: usize, completion: &IoCompletionArray, device: &VirtioBlk) {
    // SAFETY: 启动阶段单线程调用, 无并发风险.
    //         completion 指针: IoCompletionArray 是 VirtioBlk 的字段, 生命周期与 VirtioBlk 绑定.
    //         device 指针: VirtioBlk 由 storage_init 创建后通过 register_block_device 传入
    //         Chitin, Chitin 将其 Box 化并持有至系统关闭, 故 device 指针在注册期间始终有效.
    //         若未来 Chitin 支持设备热拔插, 需在注销时同步清除注册表条目.
    unsafe {
        if irq < MAX_VIRTIO_BLK_IRQS {
            VIRTIO_BLK_REGISTRY[irq] = Some(VirtioBlkRegistryEntry {
                completion: completion as *const IoCompletionArray,
                device: device as *const VirtioBlk,
            });
        }
    }
}

/// 检查设备 ID 是否表示块设备.
#[inline]
pub fn is_block_device(device_id: u32) -> bool {
    device_id == VIRTIO_ID_BLOCK
}

// ── BlockDevice trait 实现 ──

impl BlockDevice for VirtioBlk {
    fn blk_read(&mut self, sector: u64, buf: &mut [u8]) -> i32 {
        match self.read_sector(sector, buf) {
            Ok(()) => 0,
            Err(()) => -1,
        }
    }

    fn blk_write(&mut self, sector: u64, buf: &[u8]) -> i32 {
        match self.write_sector(sector, buf) {
            Ok(()) => 0,
            Err(()) => -1,
        }
    }

    fn blk_is_present(&self) -> bool {
        true
    }

    fn blk_total_sectors(&self) -> u64 {
        self.capacity_sectors
    }
}
