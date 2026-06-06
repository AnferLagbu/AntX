use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;

#[cfg(feature = "kernel_test")]
pub mod arch;
#[cfg(feature = "kernel_test")]
pub mod driver;
#[cfg(feature = "kernel_test")]
pub mod idt;
#[cfg(feature = "kernel_test")]
pub mod net;
#[cfg(feature = "kernel_test")]
pub mod reset;
#[cfg(feature = "kernel_test")]
pub mod sched;
#[cfg(feature = "kernel_test")]
pub mod string;
#[cfg(feature = "kernel_test")]
pub mod sync;
#[cfg(feature = "kernel_test")]
pub mod sys;
pub mod test_barrier;
pub mod test_barrier_ext;
pub mod test_config;
pub mod test_devfs;
#[cfg(target_arch = "x86_64")]
pub mod test_hvfs;
#[cfg(target_arch = "x86_64")]
pub mod test_hvfs_ext;
pub mod test_ipc;
pub mod test_mm;
pub mod test_new_features;
pub mod test_proc;
pub mod test_pwm;
pub mod test_smp;
pub mod test_vfs;

pub type TestFn = fn() -> TestResult;

pub enum TestResult {
    Pass,
    Fail(&'static str),
    Skip(&'static str),
}

#[derive(Copy, Clone)]
pub struct TestCase {
    pub module: &'static str,
    pub name: &'static str,
    pub func: TestFn,
}

const MAX_TESTS: usize = 256;

fn noop_test() -> TestResult {
    TestResult::Pass
}

const NOOP_CASE: TestCase = TestCase {
    module: "",
    name: "",
    func: noop_test,
};

struct TestRegistry {
    count: usize,
    cases: [TestCase; MAX_TESTS],
}

impl TestRegistry {
    const fn new() -> Self {
        Self {
            count: 0,
            cases: [NOOP_CASE; MAX_TESTS],
        }
    }

    fn register(&mut self, module: &'static str, name: &'static str, func: TestFn) {
        if self.count < MAX_TESTS {
            self.cases[self.count] = TestCase { module, name, func };
            self.count += 1;
        }
    }
}

pub struct TestRunner {
    registry: spin::Mutex<TestRegistry>,
    pub passed: AtomicU32,
    pub failed: AtomicU32,
    pub skipped: AtomicU32,
}

impl TestRunner {
    pub const fn new() -> Self {
        Self {
            registry: spin::Mutex::new(TestRegistry::new()),
            passed: AtomicU32::new(0),
            failed: AtomicU32::new(0),
            skipped: AtomicU32::new(0),
        }
    }

    pub fn register(&self, module: &'static str, name: &'static str, func: TestFn) {
        self.registry.lock().register(module, name, func);
    }

    pub fn run_all(&self) {
        let reg = self.registry.lock();
        let total = reg.count;

        Self::serial_print(b"\n========================================\n");
        Self::serial_print(b"  QueenX Test Suite\n  ");
        Self::serial_print_num(total as u64);
        Self::serial_print(b" test cases registered\n");
        Self::serial_print(b"========================================\n\n");

        crate::arch!(interrupt_disable());

        for i in 0..total {
            let tc = reg.cases[i];
            let module = tc.module;
            let name = tc.name;
            let func = tc.func;

            Self::serial_print(b"[");
            Self::serial_print_num((i + 1) as u64);
            Self::serial_print(b"/");
            Self::serial_print_num(total as u64);
            Self::serial_print(b"] ");
            Self::serial_print(module.as_bytes());
            Self::serial_print(b"::");
            Self::serial_print(name.as_bytes());
            Self::serial_print(b"...");

            let result = func();

            match result {
                TestResult::Pass => {
                    self.passed.fetch_add(1, Ordering::Relaxed);
                    Self::serial_print(b"PASS\n");
                }
                TestResult::Fail(msg) => {
                    self.failed.fetch_add(1, Ordering::Relaxed);
                    Self::serial_print(b"FAIL: ");
                    Self::serial_print(msg.as_bytes());
                    Self::serial_print(b"\n");
                }
                TestResult::Skip(reason) => {
                    self.skipped.fetch_add(1, Ordering::Relaxed);
                    Self::serial_print(b"SKIP: ");
                    Self::serial_print(reason.as_bytes());
                    Self::serial_print(b"\n");
                }
            }
        }

        drop(reg);

        let p = self.passed.load(Ordering::Relaxed);
        let f = self.failed.load(Ordering::Relaxed);
        let s = self.skipped.load(Ordering::Relaxed);

        Self::serial_print(b"\n========================================\n");
        if f > 0 {
            Self::serial_print(b"  RESULT: ");
            Self::serial_print_num(p as u64);
            Self::serial_print(b" passed, ");
            Self::serial_print_num(f as u64);
            Self::serial_print(b" FAILED, ");
            Self::serial_print_num(s as u64);
            Self::serial_print(b" skipped\n");
        } else {
            Self::serial_print(b"  RESULT: ALL ");
            Self::serial_print_num(p as u64);
            Self::serial_print(b" TESTS PASSED (");
            Self::serial_print_num(s as u64);
            Self::serial_print(b" skipped)\n");
        }
        Self::serial_print(b"========================================\n");
    }

    fn serial_print(s: &[u8]) {
        #[cfg(target_arch = "x86_64")]
        serial_print(s);
        #[cfg(target_arch = "aarch64")]
        for &b in s {
            unsafe {
                crate::kernel::framework::arch::aarch64::uart::putc(b);
            }
        }
    }

    fn serial_print_num(n: u64) {
        #[cfg(target_arch = "x86_64")]
        serial_print_num(n);
        #[cfg(target_arch = "aarch64")]
        {
            if n == 0 {
                unsafe {
                    crate::kernel::framework::arch::aarch64::uart::putc(b'0');
                }
                return;
            }
            let mut buf = [0u8; 20];
            let mut pos = 0usize;
            let mut val = n;
            while val > 0 {
                buf[pos] = (val % 10) as u8 + b'0';
                pos += 1;
                val /= 10;
            }
            for i in (0..pos).rev() {
                unsafe {
                    crate::kernel::framework::arch::aarch64::uart::putc(buf[i]);
                }
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn port_inb(port: u16) -> u8 {
    crate::arch!(inb(port))
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn port_outb(port: u16, value: u8) {
    crate::arch!(outb(port, value));
}

#[cfg(target_arch = "x86_64")]
pub fn serial_print(s: &[u8]) {
    const COM1: u16 = 0x3F8;
    for &b in s {
        unsafe {
            while (port_inb(COM1 + 5) & 0x20) == 0 {
                core::hint::spin_loop();
            }
            port_outb(COM1, b);
        }
        if b == b'\n' {
            unsafe {
                port_outb(COM1, b'\r');
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
pub fn serial_print_num(mut n: u64) {
    if n == 0 {
        serial_print(b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut pos = 0usize;
    while n > 0 {
        buf[pos] = (n % 10) as u8 + b'0';
        pos += 1;
        n /= 10;
    }
    for i in (0..pos).rev() {
        unsafe {
            const COM1: u16 = 0x3F8;
            while (port_inb(COM1 + 5) & 0x20) == 0 {
                core::hint::spin_loop();
            }
            port_outb(COM1, buf[i]);
        }
    }
}

static TEST_RUNNER: spin::Once<TestRunner> = spin::Once::new();

pub fn runner() -> &'static TestRunner {
    TEST_RUNNER.call_once(TestRunner::new)
}

#[macro_export]
macro_rules! check {
    ($cond:expr, $msg:literal $(,)?) => {
        if !($cond) {
            return $crate::kernel::framework::tests::TestResult::Fail($msg);
        }
    };
}

#[macro_export]
macro_rules! assert_eq_test {
    ($left:expr, $right:expr, $msg:literal $(,)?) => {
        let l = $left;
        let r = $right;
        if l != r {
            return $crate::kernel::framework::tests::TestResult::Fail($msg);
        }
    };
}

#[macro_export]
macro_rules! skip_test {
    ($reason:literal $(,)?) => {
        return $crate::kernel::framework::tests::TestResult::Skip($reason);
    };
}

#[macro_export]
macro_rules! register_tests_inner {
    ($r:ident: $($mod:literal: { $($name:literal: $func:ident),* $(,)? }),* $(,)?) => {
        $(
            $(
                $r.register($mod, $name, $func);
            )*
        )*
    };
}

pub use {assert_eq_test, check, skip_test};

pub fn test_runner_init() {
    crate::klog_boot_info!("[TEST] === QueenX Test Framework ===");

    test_barrier::register_barrier_tests();
    test_barrier_ext::register_barrier_ext_tests();
    test_config::register_config_tests();
    #[cfg(target_arch = "x86_64")]
    {
        test_hvfs::register_hvfs_tests();
        test_hvfs_ext::register_hvfs_ext_tests();
    }
    test_pwm::register_pwm_tests();
    test_mm::register_mm_tests();
    test_vfs::register_vfs_tests();
    test_ipc::register_ipc_tests();
    test_devfs::register_devfs_tests();
    test_proc::register_proc_tests();
    test_new_features::register_new_tests();
    test_smp::register_smp_tests();

    #[cfg(feature = "kernel_test")]
    {
        #[cfg(target_arch = "x86_64")]
        {
            arch::register_tests();
            sys::register_tests();
            idt::register_tests();
            driver::register_tests();
        }
        string::register_tests();
        sched::register_tests();
        sync::register_tests();
        net::register_tests();
        reset::register_tests();
        #[cfg(target_arch = "x86_64")]
        {
            crate::kernel::framework::timer::pit::register_pit_tests();
            crate::kernel::framework::timer::calibration::register_timer_calibration_tests();
        }
        crate::kernel::framework::timer::tick::register_timer_tick_tests();
        #[cfg(target_arch = "x86_64")]
        crate::kernel::framework::timer::irq::register_timer_irq_tests();
        crate::kernel::framework::timer::sleep::register_timer_sleep_tests();
        crate::kernel::framework::timer::hrtimer::register_hrtimer_tests();
    }

    let r = runner();
    let count = r.registry.lock().count;
    crate::klog_boot_info!("[TEST] Registered {} test cases", count);

    r.run_all();

    let p = r.passed.load(Ordering::Relaxed);
    let f = r.failed.load(Ordering::Relaxed);
    if f == 0 {
        crate::klog_boot_info!(
            "[TEST] ALL TESTS PASSED ({}/{})",
            p,
            p + r.skipped.load(Ordering::Relaxed)
        );
    } else {
        crate::klog_boot_info!("[TEST] COMPLETE: {} passed, {} FAILED", p, f);
    }
}

pub fn qemu_exit(success: bool) -> ! {
    #[cfg(target_arch = "x86_64")]
    {
        let exit_code = if success { 0x10 } else { 0x11 };
        unsafe {
            use core::arch::asm;
            asm!(
                "out dx, al",
                in("dx") 0xf4u16,
                in("al") exit_code as u8,
                options(nomem, nostack)
            );
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = success;
    }
    loop {
        crate::arch!(halt());
    }
}
