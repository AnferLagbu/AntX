use alloc::vec::Vec;
use crate::kernel::sync::mutex::Mutex;

pub mod test_barrier;
pub mod test_hvfs;

pub type TestFn = fn() -> Result<(), &'static str>;

pub struct TestCase {
    pub module: &'static str,
    pub name: &'static str,
    pub func: TestFn,
}

pub struct TestRunner {
    pub tests: Mutex<Vec<TestCase>>,
    pub passed: Mutex<usize>,
    pub failed: Mutex<usize>,
}

impl TestRunner {
    pub fn new() -> Self {
        Self {
            tests: Mutex::new(Vec::new()),
            passed: Mutex::new(0),
            failed: Mutex::new(0),
        }
    }

    pub fn register(&self, module: &'static str, name: &'static str, func: TestFn) {
        self.tests.lock().push(TestCase { module, name, func });
    }

    pub fn run_all(&self) {
        let tests = self.tests.lock();
        let total = tests.len();
        crate::klog_info!(Test, "=== Running {} tests ===", total);

        for tc in tests.iter() {
            let result = (tc.func)();
            match result {
                Ok(()) => {
                    *self.passed.lock() += 1;
                }
                Err(msg) => {
                    *self.failed.lock() += 1;
                    crate::klog_err!(Test, "  FAIL {}::{} : {}", tc.module, tc.name, msg);
                }
            }
        }

        let p = *self.passed.lock();
        let f = *self.failed.lock();
        if f > 0 {
            crate::klog_info!(Test, "=== DONE: {}/{} passed, {} FAILED ===", p, total, f);
        } else {
            crate::klog_info!(Test, "=== DONE: {}/{} passed ===", p, total);
        }
    }
}

static mut TEST_RUNNER: Option<TestRunner> = None;

pub fn runner() -> &'static TestRunner {
    unsafe {
        if TEST_RUNNER.is_none() {
            TEST_RUNNER = Some(TestRunner::new());
        }
        TEST_RUNNER.as_ref().unwrap()
    }
}

#[macro_export]
macro_rules! check {
    ($cond:expr, $msg:literal $(,)?) => {
        if !($cond) {
            return Err($msg);
        }
    };
}

#[macro_export]
macro_rules! assert_eq_test {
    ($left:expr, $right:expr, $msg:literal $(,)?) => {
        let l = $left;
        let r = $right;
        if l != r {
            return Err($msg);
        }
    };
}

pub fn run_all_tests() {
    runner().run_all();
}

pub fn test_runner_init() {
    crate::klog_boot_info!("[TEST] === AntX Kernel Test Framework ===");

    // Register all subsystem tests
    test_barrier::register_barrier_tests();
    test_hvfs::register_hvfs_tests();

    let r = runner();
    let count = r.tests.lock().len();
    crate::klog_boot_info!("[TEST] Registered {} test cases", count);

    r.run_all();

    let p = *r.passed.lock();
    let f = *r.failed.lock();
    if f == 0 {
        crate::klog_boot_info!("[TEST] ALL TESTS PASSED ({}/{})", p, p + f);
    } else {
        crate::klog_boot_info!("[TEST] COMPLETE: {} passed, {} FAILED", p, f);
    }
}
