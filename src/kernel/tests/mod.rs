use alloc::vec::Vec;
use alloc::string::String;
use crate::kernel::sync::mutex::Mutex;
use core::sync::atomic::{AtomicU32, Ordering};

pub mod test_barrier;
pub mod test_barrier_ext;
pub mod test_hvfs;
pub mod test_hvfs_ext;
pub mod test_pwid;
pub mod test_mm;
pub mod test_vfs;
pub mod test_ipc;
pub mod test_devfs;
pub mod test_proc;

pub type TestFn = fn() -> TestResult;

pub enum TestResult {
    Pass,
    Fail(&'static str),
    Skip(&'static str),
}

pub struct TestCase {
    pub module: &'static str,
    pub name: &'static str,
    pub func: TestFn,
}

pub struct TestRunner {
    pub tests: Mutex<Vec<TestCase>>,
    pub passed: AtomicU32,
    pub failed: AtomicU32,
    pub skipped: AtomicU32,
}

impl TestRunner {
    pub fn new() -> Self {
        Self {
            tests: Mutex::new(Vec::new()),
            passed: AtomicU32::new(0),
            failed: AtomicU32::new(0),
            skipped: AtomicU32::new(0),
        }
    }

    pub fn register(&self, module: &'static str, name: &'static str, func: TestFn) {
        self.tests.lock().push(TestCase { module, name, func });
    }

    pub fn run_all(&self) {
        let tests = self.tests.lock();
        let total = tests.len();
        crate::klog_info!(Test, "");
        crate::klog_info!(Test, "========================================");
        crate::klog_info!(Test, "  AntX Kernel Test Suite");
        crate::klog_info!(Test, "  {} test cases registered", total);
        crate::klog_info!(Test, "========================================");
        crate::klog_info!(Test, "");

        for (i, tc) in tests.iter().enumerate() {
            let start_tick = Self::current_tick();
            let result = (tc.func)();
            let elapsed = Self::current_tick().saturating_sub(start_tick);

            match result {
                TestResult::Pass => {
                    self.passed.fetch_add(1, Ordering::Relaxed);
                    crate::klog_info!(Test, "  [{:3}/{}] PASS {}::{} ({}ms)",
                        i + 1, total, tc.module, tc.name, elapsed);
                }
                TestResult::Fail(msg) => {
                    self.failed.fetch_add(1, Ordering::Relaxed);
                    crate::klog_err!(Test, "  [{:3}/{}] FAIL {}::{} : {} ({}ms)",
                        i + 1, total, tc.module, tc.name, msg, elapsed);
                }
                TestResult::Skip(reason) => {
                    self.skipped.fetch_add(1, Ordering::Relaxed);
                    crate::klog_info!(Test, "  [{:3}/{}] SKIP {}::{} : {}",
                        i + 1, total, tc.module, tc.name, reason);
                }
            }
        }

        let p = self.passed.load(Ordering::Relaxed);
        let f = self.failed.load(Ordering::Relaxed);
        let s = self.skipped.load(Ordering::Relaxed);

        crate::klog_info!(Test, "");
        crate::klog_info!(Test, "========================================");
        if f > 0 {
            crate::klog_info!(Test, "  RESULT: {} passed, {} FAILED, {} skipped (total: {})",
                p, f, s, total);
        } else {
            crate::klog_info!(Test, "  RESULT: ALL {} TESTS PASSED ({} skipped)", p, s);
        }
        crate::klog_info!(Test, "========================================");
        crate::klog_info!(Test, "");
    }

    fn current_tick() -> u64 {
        extern "C" { fn timer_get_ticks() -> u64; }
        unsafe { timer_get_ticks() }
    }
}

static TEST_RUNNER: spin::Once<TestRunner> = spin::Once::new();

pub fn runner() -> &'static TestRunner {
    TEST_RUNNER.call_once(|| TestRunner::new())
}

#[macro_export]
macro_rules! check {
    ($cond:expr, $msg:literal $(,)?) => {
        if !($cond) {
            return $crate::kernel::tests::TestResult::Fail($msg);
        }
    };
}

#[macro_export]
macro_rules! assert_eq_test {
    ($left:expr, $right:expr, $msg:literal $(,)?) => {
        let l = $left;
        let r = $right;
        if l != r {
            return $crate::kernel::tests::TestResult::Fail($msg);
        }
    };
}

#[macro_export]
macro_rules! skip_test {
    ($reason:literal $(,)?) => {
        return $crate::kernel::tests::TestResult::Skip($reason);
    };
}

pub fn run_all_tests() {
    runner().run_all();
}

pub fn test_runner_init() {
    crate::klog_boot_info!("[TEST] === AntX Kernel Test Framework ===");

    test_barrier::register_barrier_tests();
    test_barrier_ext::register_barrier_ext_tests();
    test_hvfs::register_hvfs_tests();
    test_hvfs_ext::register_hvfs_ext_tests();
    test_pwid::register_pwid_tests();
    test_mm::register_mm_tests();
    test_vfs::register_vfs_tests();
    test_ipc::register_ipc_tests();
    test_devfs::register_devfs_tests();
    test_proc::register_proc_tests();

    let r = runner();
    let count = r.tests.lock().len();
    crate::klog_boot_info!("[TEST] Registered {} test cases", count);

    r.run_all();

    let p = r.passed.load(Ordering::Relaxed);
    let f = r.failed.load(Ordering::Relaxed);
    if f == 0 {
        crate::klog_boot_info!("[TEST] ALL TESTS PASSED ({}/{})", p, p + r.skipped.load(Ordering::Relaxed));
    } else {
        crate::klog_boot_info!("[TEST] COMPLETE: {} passed, {} FAILED", p, f);
    }
}
