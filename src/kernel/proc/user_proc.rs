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
    fn pmm_alloc_page() -> *mut core::ffi::c_void;
    fn pmm_alloc_pages(count: u64) -> *mut core::ffi::c_void;
    fn pmm_free_page(page: *mut core::ffi::c_void);
    fn vmm_create_user_page_table() -> u64;
    fn vmm_map_page_in_table(table: u64, vaddr: u64, paddr: u64, flags: u64);
    fn vmm_map_page(vaddr: u64, paddr: u64, flags: u64) -> i32;
    fn vmm_split_2mb_page(vaddr: u64) -> i32;
    fn vmm_ensure_path_user(vaddr: u64);
    fn vmm_switch_page_table(table: u64);
    fn vmm_get_physical_in_table(table: u64, vaddr: u64) -> u64;
    fn tss_set_kernel_stack(rsp0: u64);
    fn memset(s: *mut u8, c: i32, n: u64);
    fn memcpy(dest: *mut u8, src: *const u8, n: u64);
    fn kmalloc(size: u64) -> *mut core::ffi::c_void;
}

pub const PAGE_SIZE: u64 = 4096;
pub const USER_STACK_SIZE: u64 = 65536;
pub const USER_STACK_GUARD: u64 = 4096;
pub const USER_STACK_TOP: u64 = 0x7FFFFFFFE000;
pub const USER_KSTACK_SIZE: u64 = 16384;
pub const USER_STACK_MAX_SIZE: u64 = 8 * 1024 * 1024;
pub const USER_STACK_EXPAND_LIMIT: u64 = USER_STACK_TOP - USER_STACK_MAX_SIZE;
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
    pub stack_bottom: AtomicU64,
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

    fn destroy(&self, proc: *mut UserProcess) {
        if proc.is_null() { return; }
        unsafe {
            let cr3 = (*proc).cr3.load(Ordering::SeqCst);
            if cr3 != 0 {
                extern "C" { fn vmm_destroy_page_table(cr3: u64); }
                vmm_destroy_page_table(cr3);
            }
            let kstack = (*proc).kernel_stack.load(Ordering::SeqCst);
            if kstack != 0 {
                // kstack is a higher-half virtual address; convert back to physical for PMM
                let kstack_base_virt = kstack - USER_KSTACK_SIZE;
                let kstack_base_phys = kstack_base_virt - KERNEL_BASE;
                for i in 0..(USER_KSTACK_SIZE / PAGE_SIZE) {
                    pmm_free_page((kstack_base_phys + i * PAGE_SIZE) as *mut core::ffi::c_void);
                }
            }
            let ustack = (*proc).user_stack.load(Ordering::SeqCst);
            if ustack != 0 {
                let stack_virt = USER_STACK_TOP - USER_STACK_SIZE - USER_STACK_GUARD;
                for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
                    let svirt = stack_virt + USER_STACK_GUARD + i * PAGE_SIZE;
                    let phys = vmm_get_physical_in_table(cr3, svirt);
                    if phys != 0 { pmm_free_page(phys as *mut core::ffi::c_void); }
                }
            }
            let pid = (*proc).pid;
            self.processes.lock().remove(&pid);
        }
    }
    
    pub fn get(&self, pid: u32) -> Option<*mut UserProcess> {
        self.processes.lock().get(&pid).copied()
    }

    pub fn destroy_by_pid(&self, pid: u32) {
        if let Some(proc) = self.get(pid) {
            self.destroy(proc);
        }
    }
    
    pub fn create(&self, info: &UserProcInfo, pwid: u64) -> Option<*mut UserProcess> {
        let pid = PROCESS_TABLE.allocate_pid()?;
        
        let proc = unsafe {
            let ptr = kmalloc(core::mem::size_of::<UserProcess>() as u64) as *mut UserProcess;
            if ptr.is_null() { return None; }
            memset(ptr as *mut u8, 0, core::mem::size_of::<UserProcess>() as u64);
            ptr
        };
        
        unsafe {
            (*proc).pid = pid;
            let cr3_val = vmm_create_user_page_table();
            (*proc).cr3.store(cr3_val, Ordering::SeqCst);
            if cr3_val == 0 { return None; }
            
            let stack_pages = pmm_alloc_pages((USER_STACK_SIZE + USER_STACK_GUARD) / PAGE_SIZE);
            if stack_pages.is_null() {
                pmm_free_page(cr3_val as *mut core::ffi::c_void);
                return None;
            }
            
            let stack_phys = stack_pages as u64;
            let stack_virt = USER_STACK_TOP - USER_STACK_SIZE - USER_STACK_GUARD;
            
            for i in 0..(USER_STACK_SIZE / PAGE_SIZE) {
                let svirt = stack_virt + USER_STACK_GUARD + i * PAGE_SIZE;
                let sphys = stack_phys + i * PAGE_SIZE;
                vmm_map_page_in_table(
                    (*proc).cr3.load(Ordering::SeqCst),
                    svirt, sphys,
                    PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER
                );
                vmm_map_page(svirt, sphys, PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
                vmm_ensure_path_user(svirt);
            }
            
            (*proc).user_stack.store(USER_STACK_TOP, Ordering::SeqCst);
            let initial_stack_bottom = USER_STACK_TOP - USER_STACK_SIZE;
            (*proc).stack_bottom.store(initial_stack_bottom, Ordering::SeqCst);
            
            let kstack = pmm_alloc_pages(USER_KSTACK_SIZE / PAGE_SIZE);
            if kstack.is_null() {
                pmm_free_page(stack_pages);
                pmm_free_page((*proc).cr3.load(Ordering::SeqCst) as *mut core::ffi::c_void);
                return None;
            }
            // Convert physical address to higher-half virtual address.
            // The user page table only maps PML4[256..511] (kernel higher-half),
            // so the TSS RSP0 must be a higher-half address to be accessible
            // when an interrupt fires in Ring 3 with the user CR3 loaded.
            let kstack_top = kstack as u64 + KERNEL_BASE + USER_KSTACK_SIZE;
            (*proc).kernel_stack.store(kstack_top, Ordering::SeqCst);
            crate::kernel::proc::process::kernel_stack_write_canary(kstack_top);
            
            (*proc).entry = info.entry;
            (*proc).pwid.store(pwid, Ordering::SeqCst);
            (*proc).state.store(1, Ordering::SeqCst);
            (*proc).create_time = crate::kernel::timer::get_ticks();
        }
        
        self.processes.lock().insert(pid, proc);
        
        let kernel_proc = alloc::boxed::Box::new(Process {
            pid: ProcessId(pid),
            pwid: AtomicU64::new(pwid),
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
            session_id: AtomicU64::new(0),
            fd_table: super::process::FdTable::new(),
            sleep_until: AtomicU64::new(0),
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
            
            let kstack = (*proc).kernel_stack.load(Ordering::SeqCst);
            let rip_val = (*proc).entry;
            let rsp_val = (*proc).user_stack.load(Ordering::SeqCst);
            let ss_val = GDT_USER_DATA | 0x03;
            let cs_val = GDT_USER_CODE | 0x03;
            let rflags_val: u64 = 0x3202;
            
            tss_set_kernel_stack(kstack);
            
            core::arch::asm!(
                "cli",
                "push r8",
                "push r9",
                "push r10",
                "push r11",
                "push r12",
                "iretq",
                in("r8") ss_val,
                in("r9") rsp_val,
                in("r10") rflags_val,
                in("r11") cs_val,
                in("r12") rip_val,
                options(noreturn)
            );
        }
    }
    
    /// Write argc/argv/envp to the user process stack
    /// Returns the new stack pointer (RSP) after setup
    pub unsafe fn setup_user_stack(
        &self,
        proc: *mut UserProcess,
        argv: *const *const u8,
        argc: usize,
        _envp: *const *const u8,
        envc: usize,
    ) -> u64 {
        if proc.is_null() { return 0; }
        
        let stack_top = (*proc).user_stack.load(Ordering::SeqCst);
        let cr3 = (*proc).cr3.load(Ordering::SeqCst);
        
        // Space needed: argc(8) + argv_ptrs(8*(argc+1)) + envp_ptrs(8*(envc+1)) + strings
        let mut string_bytes: usize = 0;
        let mut arg_lens: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
        
        if !argv.is_null() {
            for i in 0..argc {
                let s = *argv.add(i);
                if !s.is_null() {
                    let mut len: usize = 0;
                    while *s.add(len) != 0 { len += 1; }
                    arg_lens.push(len + 1);
                    string_bytes += len + 1;
                } else {
                    arg_lens.push(1);
                    string_bytes += 1;
                }
            }
        }
        
        let ptr_count = 1 + (argc + 1) + (envc + 1); // argc(1) + argv(n+1) + envp(m+1)
        let total = ptr_count * 8 + string_bytes;
        // Ensure 16-byte alignment
        let total = (total + 15) & !15;
        
        if total as u64 > stack_top - USER_STACK_TOP + USER_STACK_SIZE {
            return 0; // Stack overflow
        }
        
        let new_sp = stack_top - total as u64;
        let mut pos = new_sp as usize;
        
        // Write argc
        let argc_off = pos; pos += 8;
        // Skip argv ptrs (written after strings)
        let argv_start_off = pos; pos += (argc + 1) * 8;
        // Skip envp ptrs
        let envp_start_off = pos; pos += (envc + 1) * 8;
        // String area starts here
        let strings_off = pos;
        
        // Resolve virtual addresses in user space by writing through kernel mapping
        let w64 = |off: usize, v: u64| {
            let phys = vmm_get_physical_in_table(cr3, off as u64 & !0xFFF);
            if phys != 0 {
                let addr = (phys + KERNEL_BASE + (off as u64 & 0xFFF)) as *mut u8;
                core::ptr::write_unaligned(addr as *mut u64, v);
            }
        };
        let w8 = |off: usize, v: u8| {
            let phys = vmm_get_physical_in_table(cr3, off as u64 & !0xFFF);
            if phys != 0 {
                let addr = (phys + KERNEL_BASE + (off as u64 & 0xFFF)) as *mut u8;
                *addr = v;
            }
        };
        
        // Write argc
        w64(argc_off, argc as u64);
        
        // Write argv strings + pointers
        let mut str_off = strings_off;
        for i in 0..argc {
            w64(argv_start_off + i * 8, new_sp.wrapping_add(str_off as u64 - new_sp as u64 + strings_off as u64 - strings_off as u64));
            // Actually compute absolute user-space address:
            let abs_addr = str_off as u64;
            // Write pointer: user-space address
            let p_addr = argv_start_off + i * 8;
            let p_phys = vmm_get_physical_in_table(cr3, p_addr as u64 & !0xFFF);
            if p_phys != 0 {
                let p_ptr = (p_phys + KERNEL_BASE + (p_addr as u64 & 0xFFF)) as *mut u64;
                core::ptr::write_unaligned(p_ptr, abs_addr);
            }
            
            if !argv.is_null() && (i < argc) {
                let src = *argv.add(i);
                let l = arg_lens[i];
                for j in 0..l {
                    let b = if src.is_null() { 0u8 } else { unsafe { *src.add(j) } };
                    w8(str_off + j, b);
                }
                str_off += l;
            }
        }
        // argv NULL terminator
        let null_ptr_off = argv_start_off + argc * 8;
        let np_phys = vmm_get_physical_in_table(cr3, null_ptr_off as u64 & !0xFFF);
        if np_phys != 0 {
            let np_ptr = (np_phys + KERNEL_BASE + (null_ptr_off as u64 & 0xFFF)) as *mut u64;
            core::ptr::write_unaligned(np_ptr, 0u64);
        }
        
        // envp pointers (all NULL for now)
        for i in 0..(envc + 1) {
            let ep_off = envp_start_off + i * 8;
            let ep_phys = vmm_get_physical_in_table(cr3, ep_off as u64 & !0xFFF);
            if ep_phys != 0 {
                let ep_ptr = (ep_phys + KERNEL_BASE + (ep_off as u64 & 0xFFF)) as *mut u64;
                core::ptr::write_unaligned(ep_ptr, 0u64);
            }
        }
        
        // Update process stack pointer
        (*proc).user_stack.store(new_sp, Ordering::SeqCst);
        new_sp
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

            let mut allocated_pages: [u64; 1024] = [0; 1024];
            let mut page_count: usize = 0;

            let mut phnum = (*header).phnum as usize;
            if phnum > 256 { self.destroy(proc); return -1; }

            for i in 0..phnum {
                let phdr_size = core::mem::size_of::<ElfPhdr>() as u64;
                let phdr_offset = (*header).phoff + (i as u64) * (*header).phentsize as u64;
                if phdr_offset + phdr_size > elf_size { self.destroy(proc); return -1; }
                let phdr = (elf_data.add(phdr_offset as usize)) as *const ElfPhdr;
                
                if (*phdr).p_type != PT_LOAD { continue; }
                
                let vaddr_start = (*phdr).p_vaddr & !0xFFF;
                let vaddr_end = ((*phdr).p_vaddr + (*phdr).p_memsz + 0xFFF) & !0xFFF;
                let num_pages = (vaddr_end - vaddr_start) / PAGE_SIZE;
                
                for j in 0..num_pages {
                    let vaddr = vaddr_start + j * PAGE_SIZE;
                    
                    let mut flags = PAGE_PRESENT | PAGE_USER;
                    if (*phdr).p_flags & 0x02 != 0 {
                        flags |= PAGE_WRITABLE;
                    }
                    
                    // Check if this page is already mapped (shared by a previous PHDR)
                    let existing_phys = vmm_get_physical_in_table(cr3, vaddr);
                    
                    if existing_phys == 0 {
                        let page = pmm_alloc_page();
                        if page.is_null() {
                            for pi in 0..page_count {
                                pmm_free_page(allocated_pages[pi] as *mut core::ffi::c_void);
                            }
                            self.destroy(proc);
                            return -1;
                        }
                        if page_count < 1024 {
                            allocated_pages[page_count] = page as u64;
                            page_count += 1;
                        }
                        memset(page as *mut u8, 0, PAGE_SIZE);
                        vmm_map_page_in_table(cr3, vaddr, page as u64, flags);
                        vmm_map_page(vaddr, page as u64, flags);
                        vmm_ensure_path_user(vaddr);
                    } else {
                        // Reuse existing page, record it
                        if page_count < 1024 {
                            allocated_pages[page_count] = existing_phys;
                            page_count += 1;
                        }
                    }
                }
                
                if (*phdr).p_filesz > 0 {
                    let file_offset_bytes = (*phdr).p_offset as usize;
                    let mut copied: u64 = 0;
                    let first_page_offset = (*phdr).p_vaddr & 0xFFF;
                    let start_idx = page_count.saturating_sub(num_pages as usize);
                    
                    for j in 0..num_pages {
                        if copied >= (*phdr).p_filesz { break; }
                        let page_phys = allocated_pages[start_idx + j as usize];
                        if page_phys == 0 { continue; }
                        
                        let off_in_page = if j == 0 { first_page_offset } else { 0 };
                        let dest = (page_phys + KERNEL_BASE + off_in_page) as *mut u8;
                        let src = elf_data.add(file_offset_bytes + (copied as usize));
                        let max_in_page = PAGE_SIZE - off_in_page;
                        let remaining = (*phdr).p_filesz as u64 - copied;
                        let chunk = if max_in_page < remaining { max_in_page } else { remaining };
                        memcpy(dest, src, chunk);
                        copied += chunk;
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
                
                memset(page as *mut u8, 0, PAGE_SIZE);
                
                let copy_size = if code_size - i * PAGE_SIZE > PAGE_SIZE {
                    PAGE_SIZE
                } else {
                    code_size - i * PAGE_SIZE
                };
                
                memcpy(page as *mut u8, code.add((i * PAGE_SIZE) as usize), copy_size);
                
                vmm_map_page_in_table(cr3, USER_CODE_BASE + i * PAGE_SIZE, page as u64, PAGE_PRESENT | PAGE_USER);
                
                // Also map into kernel PML4
                let vaddr = USER_CODE_BASE + i * PAGE_SIZE;
                vmm_map_page(vaddr, page as u64, PAGE_PRESENT | PAGE_USER);
                vmm_ensure_path_user(vaddr);
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

    pub fn set_current(&self, proc: Option<*mut UserProcess>) {
        if let Some(p) = proc {
            self.current.store(p as u64, Ordering::SeqCst);
        } else {
            self.current.store(0, Ordering::SeqCst);
        }
    }
}

pub static USER_PROC_MANAGER: UserProcManager = UserProcManager::new();

pub fn init() {
    USER_PROC_MANAGER.init();
}

pub fn try_expand_user_stack(fault_addr: u64) -> bool {
    if fault_addr >= USER_STACK_TOP { return false; }
    if fault_addr < USER_STACK_EXPAND_LIMIT { return false; }

    let proc = match USER_PROC_MANAGER.get_current() {
        Some(p) => p,
        None => return false,
    };

    unsafe {
        let stack_bottom = (*proc).stack_bottom.load(Ordering::SeqCst);
        if fault_addr >= stack_bottom { return false; }

        let cr3 = (*proc).cr3.load(Ordering::SeqCst);
        if cr3 == 0 { return false; }

        let page_addr = fault_addr & !(PAGE_SIZE - 1);
        let pages_needed = ((stack_bottom - page_addr) / PAGE_SIZE) as u64;

        for i in 0..pages_needed {
            let vaddr = page_addr + i * PAGE_SIZE;
            if vaddr >= stack_bottom { break; }

            let phys = vmm_get_physical_in_table(cr3, vaddr);
            if phys != 0 { continue; }

            let new_page = pmm_alloc_page();
            if new_page.is_null() { return false; }

            memset(new_page as *mut u8, 0, PAGE_SIZE);

            vmm_map_page_in_table(cr3, vaddr, new_page as u64,
                PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
            vmm_map_page(vaddr, new_page as u64,
                PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
            vmm_ensure_path_user(vaddr);
        }

        (*proc).stack_bottom.store(page_addr, Ordering::SeqCst);
        true
    }
}
