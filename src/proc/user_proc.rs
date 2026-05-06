use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;
use super::process::PROCESS_TABLE;
use super::process::Process;
use super::types::{ProcessId, ProcessState, ProcessPriority, ProcessContext};

const KERNEL_BASE: u64 = 0xFFFF800000000000;

#[no_mangle]
pub static mut user_entry_cr3: AtomicU64 = AtomicU64::new(0);

#[no_mangle]
pub static mut user_entry_target: AtomicU64 = AtomicU64::new(0);

extern "C" {
    fn pmm_alloc_page() -> *mut u8;
    fn pmm_alloc_pages(count: u64) -> *mut u8;
    fn pmm_free_page(page: *mut u8);
    fn vmm_create_user_page_table() -> u64;
    fn vmm_map_page_in_table(table: u64, vaddr: u64, paddr: u64, flags: u64);
    fn vmm_switch_page_table(table: u64);
    fn vmm_get_physical_in_table(table: u64, vaddr: u64) -> u64;
    fn tss_set_kernel_stack(rsp0: u64);
    fn timer_get_ticks() -> u64;
    fn memset(s: *mut u8, c: i32, n: u64);
    fn memcpy(dest: *mut u8, src: *const u8, n: u64);
    fn kmalloc(size: u64) -> *mut u8;
}

pub const PAGE_SIZE: u64 = 4096;
pub const USER_STACK_SIZE: u64 = 65536;
pub const USER_STACK_GUARD: u64 = 4096;
pub const USER_STACK_TOP: u64 = 0x7FFFFFFFE000;
pub const USER_CODE_BASE: u64 = 0x400000;

pub const PAGE_PRESENT: u64 = 1;
pub const PAGE_WRITABLE: u64 = 2;
pub const PAGE_USER: u64 = 4;

pub const GDT_USER_CODE: u64 = 0x18;
pub const GDT_USER_DATA: u64 = 0x20;

pub const PT_LOAD: u32 = 1;

#[repr(C)]
pub struct ElfHeader {
    pub magic: [u8; 4],
    pub class: u8,
    pub endian: u8,
    pub version: u8,
    pub os_abi: u8,
    pub abi_version: u8,
    pub padding: [u8; 7],
    pub e_type: u16,
    pub machine: u16,
    pub e_version: u32,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub flags: u32,
    pub ehsize: u16,
    pub phentsize: u16,
    pub phnum: u16,
    pub shentsize: u16,
    pub shnum: u16,
    pub shstrndx: u16,
}

#[repr(C)]
pub struct ElfPhdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

#[repr(C)]
pub struct UserProcInfo {
    pub entry: u64,
    pub name: [u8; 64],
    pub code_size: u64,
    pub code_data: *const u8,
}

#[repr(C)]
pub struct UserProcess {
    pub pid: u32,
    pub pwid: AtomicU64,
    pub cr3: AtomicU64,
    pub kernel_stack: AtomicU64,
    pub user_stack: AtomicU64,
    pub entry: u64,
    pub state: AtomicU32,
    pub create_time: u64,
}

pub struct UserProcManager {
    current: AtomicU64,
    processes: Mutex<alloc::collections::BTreeMap<u32, *mut UserProcess>>,
}

unsafe impl Send for UserProcManager {}
unsafe impl Sync for UserProcManager {}

impl UserProcManager {
    pub const fn new() -> Self {
        Self {
            current: AtomicU64::new(0),
            processes: Mutex::new(alloc::collections::BTreeMap::new()),
        }
    }
    
    pub fn init(&self) {
    }
    
    pub fn get(&self, pid: u32) -> Option<*mut UserProcess> {
        self.processes.lock().get(&pid).copied()
    }
    
    pub fn create(&self, info: &UserProcInfo, pwid: u64) -> Option<*mut UserProcess> {
        let pid = PROCESS_TABLE.allocate_pid()?;
        
        let proc = unsafe {
            let ptr = kmalloc(core::mem::size_of::<UserProcess>() as u64) as *mut UserProcess;
            if ptr.is_null() {
                return None;
            }
            memset(ptr as *mut u8, 0, core::mem::size_of::<UserProcess>() as u64);
            ptr
        };
        
        unsafe {
            (*proc).pid = pid;
            (*proc).cr3.store(vmm_create_user_page_table(), Ordering::SeqCst);
            if (*proc).cr3.load(Ordering::SeqCst) == 0 {
                return None;
            }
            
            let stack_pages = pmm_alloc_pages((USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE);
            if stack_pages.is_null() {
                pmm_free_page((*proc).cr3.load(Ordering::SeqCst) as *mut u8);
                return None;
            }
            
            let stack_phys = stack_pages as u64;
            let stack_virt = USER_STACK_TOP - USER_STACK_SIZE - USER_STACK_GUARD;
            
            for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
                vmm_map_page_in_table(
                    (*proc).cr3.load(Ordering::SeqCst),
                    stack_virt + USER_STACK_GUARD + i * PAGE_SIZE,
                    stack_phys + i * PAGE_SIZE,
                    PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER
                );
            }
            
            (*proc).user_stack.store(USER_STACK_TOP, Ordering::SeqCst);
            
            let kstack = pmm_alloc_page();
            if kstack.is_null() {
                pmm_free_page(stack_pages);
                pmm_free_page((*proc).cr3.load(Ordering::SeqCst) as *mut u8);
                return None;
            }
            (*proc).kernel_stack.store(kstack as u64 + PAGE_SIZE, Ordering::SeqCst);
            
            (*proc).entry = info.entry;
            (*proc).pwid.store(pwid, Ordering::SeqCst);
            (*proc).state.store(1, Ordering::SeqCst);
            (*proc).create_time = timer_get_ticks();
        }
        
        self.processes.lock().insert(pid, proc);
        
        let kernel_proc = alloc::boxed::Box::new(Process {
            pid: ProcessId(pid),
            state: AtomicU32::new(ProcessState::Ready as u32),
            priority: AtomicU32::new(ProcessPriority::Normal as u32),
            flags: AtomicU32::new(0),
            name: Mutex::new(alloc::string::String::from("user_proc")),
            parent: None,
            children: Mutex::new(alloc::vec::Vec::new()),
            context: Mutex::new(ProcessContext::new()),
            cr3: AtomicU64::new(unsafe { (*proc).cr3.load(Ordering::SeqCst) }),
            kernel_stack: AtomicU64::new(unsafe { (*proc).kernel_stack.load(Ordering::SeqCst) }),
            user_stack: AtomicU64::new(unsafe { (*proc).user_stack.load(Ordering::SeqCst) }),
            exit_code: AtomicU32::new(0),
            cpu_time: AtomicU64::new(0),
            block_reason: AtomicU32::new(0),
            sched_policy: AtomicU32::new(super::scheduler::SchedPolicy::Normal as u32),
            rt_priority: AtomicU32::new(0),
        });
        
        let kernel_proc_ptr = alloc::boxed::Box::into_raw(kernel_proc);
        PROCESS_TABLE.insert(kernel_proc_ptr);
        
        Some(proc)
    }
    
    pub fn enter(&self, proc: *mut UserProcess) {
        if proc.is_null() { return; }
        
        unsafe {
            self.current.store(proc as u64, Ordering::SeqCst);
            (*proc).state.store(2, Ordering::SeqCst);
            
            tss_set_kernel_stack((*proc).kernel_stack.load(Ordering::SeqCst));
            vmm_switch_page_table((*proc).cr3.load(Ordering::SeqCst));
            
            let ss_val = GDT_USER_DATA | 0x03;
            let cs_val = GDT_USER_CODE | 0x03;
            let rip_val = (*proc).entry;
            let rsp_val = (*proc).user_stack.load(Ordering::SeqCst);
            let rflags_val: u64 = 0x202;
            
            core::arch::asm!(
                "cli",
                "mov ds, dx",
                "mov es, dx",
                "mov fs, dx",
                "mov gs, dx",
                "push {ss}",
                "push {rsp}",
                "push {rflags}",
                "push {cs}",
                "push {rip}",
                "iretq",
                in("dx") ss_val,
                ss = in(reg) ss_val,
                rsp = in(reg) rsp_val,
                rflags = in(reg) rflags_val,
                cs = in(reg) cs_val,
                rip = in(reg) rip_val,
                options(noreturn)
            );
        }
    }
    
    pub fn load_elf_from_memory(&self, elf_data: *const u8, elf_size: u64, pwid: u64) -> i32 {
        if elf_data.is_null() || elf_size < core::mem::size_of::<ElfHeader>() as u64 {
            return -1;
        }
        
        unsafe {
            let header = elf_data as *const ElfHeader;
            
            if (*header).magic[0] != 0x7F || (*header).magic[1] != b'E' ||
               (*header).magic[2] != b'L' || (*header).magic[3] != b'F' {
                return -1;
            }
            
            if (*header).class != 2 || (*header).machine != 0x3E {
                return -1;
            }
            
            let info = UserProcInfo {
                entry: (*header).entry,
                name: [0; 64],
                code_size: 0,
                code_data: core::ptr::null(),
            };
            
            let proc = match self.create(&info, pwid) {
                Some(p) => p,
                None => return -1,
            };
            
            let cr3 = (*proc).cr3.load(Ordering::SeqCst);
            
            for i in 0..(*header).phnum as usize {
                let phdr_offset = (*header).phoff + (i as u64) * (*header).phentsize as u64;
                let phdr = (elf_data.add(phdr_offset as usize)) as *const ElfPhdr;
                
                if (*phdr).p_type != PT_LOAD { continue; }
                
                let vaddr_start = (*phdr).p_vaddr & !0xFFF;
                let vaddr_end = ((*phdr).p_vaddr + (*phdr).p_memsz + 0xFFF) & !0xFFF;
                let num_pages = (vaddr_end - vaddr_start) / PAGE_SIZE;
                
                for j in 0..num_pages {
                    let page = pmm_alloc_page();
                    if page.is_null() {
                        return -1;
                    }
                    
                    memset(page, 0, PAGE_SIZE);
                    
                    let mut flags = PAGE_PRESENT | PAGE_USER;
                    if (*phdr).p_flags & 0x02 != 0 {
                        flags |= PAGE_WRITABLE;
                    }
                    
                    vmm_map_page_in_table(cr3, vaddr_start + j * PAGE_SIZE, page as u64, flags);
                }
                
                if (*phdr).p_filesz > 0 {
                    let offset_in_first = (*phdr).p_vaddr & 0xFFF;
                    
                    for k in 0..(*phdr).p_filesz {
                        let page_idx = (offset_in_first + k) / PAGE_SIZE;
                        let offset_in_page = (offset_in_first + k) % PAGE_SIZE;
                        
                        if page_idx < num_pages {
                            let phys = vmm_get_physical_in_table(cr3, vaddr_start + page_idx * PAGE_SIZE);
                            if phys != 0 {
                                let dest = (phys + KERNEL_BASE + offset_in_page) as *mut u8;
                                let src = elf_data.add((*phdr).p_offset as usize + k as usize);
                                *dest = *src;
                            }
                        }
                    }
                }
            }
            
            (*proc).entry = (*header).entry;
            
            (*proc).pid as i32
        }
    }
    
    pub fn create_from_binary(&self, code: *const u8, code_size: u64, pwid: u64) -> i32 {
        let info = UserProcInfo {
            entry: USER_CODE_BASE,
            name: [0; 64],
            code_size,
            code_data: code,
        };
        
        let proc = match self.create(&info, pwid) {
            Some(p) => p,
            None => return -1,
        };
        
        unsafe {
            let cr3 = (*proc).cr3.load(Ordering::SeqCst);
            let num_code_pages = (code_size + PAGE_SIZE - 1) / PAGE_SIZE;
            
            for i in 0..num_code_pages {
                let page = pmm_alloc_page();
                if page.is_null() {
                    return -1;
                }
                
                memset(page, 0, PAGE_SIZE);
                
                let copy_size = if code_size - i * PAGE_SIZE > PAGE_SIZE {
                    PAGE_SIZE
                } else {
                    code_size - i * PAGE_SIZE
                };
                
                memcpy(page, code.add((i * PAGE_SIZE) as usize), copy_size);
                
                vmm_map_page_in_table(cr3, USER_CODE_BASE + i * PAGE_SIZE, page as u64, PAGE_PRESENT | PAGE_USER);
            }
            
            (*proc).pid as i32
        }
    }
    
    pub fn get_current(&self) -> Option<*mut UserProcess> {
        let current = self.current.load(Ordering::SeqCst);
        if current != 0 {
            Some(current as *mut UserProcess)
        } else {
            None
        }
    }
}

pub static USER_PROC_MANAGER: UserProcManager = UserProcManager::new();

pub fn init() {
    USER_PROC_MANAGER.init();
}
