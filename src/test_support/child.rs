//! Runs one test of the current test binary again in a child process.
//!
//! A test that needs a second process calls [`role`] first. In the parent
//! it returns `None`; the test then starts the child with [`run`], naming
//! itself and a role, and checks the child's exit status. In the child,
//! [`role`] returns that role and the test performs it instead.
//!
//! The child is the same libtest binary (`std::env::current_exe`), started
//! with `--exact <test> --nocapture --test-threads=1` and the variable
//! [`ROLE_VAR`]. nextest builds the same libtest binaries, so this works
//! under both `cargo test` and `cargo nextest run`.

use std::ffi::OsStr;
use std::process::{Command, Output};

/// The variable that carries the role into the child.
pub(crate) const ROLE_VAR: &str = "LPG_TEST_CHILD_ROLE";

/// The role of this process: `None` in the parent, the role in a child.
pub(crate) fn role() -> Option<String> {
    std::env::var(ROLE_VAR).ok()
}

/// Runs the test `test` (its full path in the binary, such as
/// `storage::engine::tests::second_open_from_a_child_fails`) in a child
/// process with `role` and the extra variables `vars`, and waits for it.
///
/// # Panics
///
/// If the child cannot be started, or if it ran no test (a wrong `test`
/// path), since the caller's check would then be vacuous.
pub(crate) fn run(test: &str, role: &str, vars: &[(&str, &OsStr)]) -> Output {
    let exe = std::env::current_exe().expect("path of the test binary");
    let out = Command::new(exe)
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(ROLE_VAR, role)
        .envs(vars.iter().copied())
        .output()
        .expect("start the child test process");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("1 passed") || stdout.contains("1 failed"),
        "the child ran no test named {test}:\n{stdout}"
    );
    out
}

/// The test path of a test function in module `module` (`module_path!()`),
/// without the crate name.
pub(crate) fn test_path(module: &str, function: &str) -> String {
    let module = module.split_once("::").map_or("", |(_, rest)| rest);
    format!("{module}::{function}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_runs_the_named_test_with_its_role() {
        if let Some(role) = role() {
            assert_eq!(role, "echo");
            assert_eq!(std::env::var("LPG_TEST_CHILD_EXTRA").unwrap(), "x");
            return;
        }
        let test = test_path(module_path!(), "child_runs_the_named_test_with_its_role");
        assert_eq!(
            test,
            "test_support::child::tests::child_runs_the_named_test_with_its_role"
        );
        let out = run(&test, "echo", &[("LPG_TEST_CHILD_EXTRA", OsStr::new("x"))]);
        assert!(out.status.success(), "{out:?}");
    }

    #[test]
    fn child_failure_is_reported() {
        if role().is_some() {
            panic!("the child fails on purpose");
        }
        let test = test_path(module_path!(), "child_failure_is_reported");
        assert!(!run(&test, "fail", &[]).status.success());
    }
}
