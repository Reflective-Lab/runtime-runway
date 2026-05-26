//! Shared assertion helpers and contract-test context.

use std::sync::Mutex;

/// Context passed into every suite. Carries the backend name (for failure
/// messages) and the namespace prefix used for collection/key/topic isolation.
#[derive(Debug, Clone)]
pub struct ContractContext {
    pub backend: String,
    pub namespace: String,
}

impl ContractContext {
    pub fn new(backend: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            backend: backend.into(),
            namespace: namespace.into(),
        }
    }

    /// Returns a collection/key prefix scoped to this run.
    pub fn scope(&self, suffix: &str) -> String {
        format!("{}/{}", self.namespace, suffix)
    }
}

/// Pass/fail record per test.
#[derive(Debug)]
pub struct SuiteReport {
    pub backend: String,
    pub trait_name: String,
    results: Mutex<Vec<TestResult>>,
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub failure: Option<String>,
}

impl SuiteReport {
    pub fn new(backend: impl Into<String>, trait_name: impl Into<String>) -> Self {
        Self {
            backend: backend.into(),
            trait_name: trait_name.into(),
            results: Mutex::new(Vec::new()),
        }
    }

    pub fn record(&self, name: impl Into<String>, result: Result<(), String>) {
        self.results.lock().unwrap().push(TestResult {
            name: name.into(),
            passed: result.is_ok(),
            failure: result.err(),
        });
    }

    /// Panics with all failures formatted for the test runner.
    pub fn assert_passed(self) {
        let results = self.results.into_inner().unwrap();
        let failures: Vec<&TestResult> = results.iter().filter(|r| !r.passed).collect();
        if failures.is_empty() {
            return;
        }
        let mut msg = format!(
            "\n{} contract violations [{} @ {}]:\n",
            failures.len(),
            self.trait_name,
            self.backend,
        );
        for f in &failures {
            msg.push_str(&format!(
                "  - {}: {}\n",
                f.name,
                f.failure.as_deref().unwrap_or("(no detail)"),
            ));
        }
        panic!("{msg}");
    }
}

/// Runs an async closure, captures panics and `Err` returns into the report.
#[macro_export]
macro_rules! contract_test {
    ($report:expr, $name:literal, $body:expr) => {{
        let result: Result<(), String> = async {
            let r: Result<(), String> = $body.await;
            r
        }
        .await;
        $report.record($name, result);
    }};
}

/// Assert helper that returns `Err(String)` so a failure aborts the current
/// contract test but allows the suite to continue.
#[macro_export]
macro_rules! contract_assert {
    ($cond:expr, $($arg:tt)*) => {
        if !$cond {
            return Err(format!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! contract_assert_eq {
    ($left:expr, $right:expr, $($arg:tt)*) => {{
        let l = &$left;
        let r = &$right;
        if l != r {
            return Err(format!(
                "{}: expected {:?}, got {:?}",
                format!($($arg)*), r, l,
            ));
        }
    }};
}
