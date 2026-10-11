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
use std::io::Read;
use std::process::{Child, Command, Output, Stdio};
use std::thread::JoinHandle;

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
        "the child ran no test named {test}:\n{stdout}\nstandard error:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// A child started by [`spawn`]: its standard output is piped to the
/// caller, and a thread drains its standard error into a buffer, so that
/// a failing check can show the child's last messages.
pub(crate) struct Spawned {
    pub(crate) child: Child,
    stderr: Option<JoinHandle<Vec<u8>>>,
}

impl Spawned {
    /// Kills the child if it still runs, waits for it, and returns its
    /// standard error. The drain ends when the child's end of the pipe
    /// closes, which its exit does.
    ///
    /// # Panics
    ///
    /// If the kill or the wait fails.
    pub(crate) fn kill(&mut self) -> String {
        self.child.kill().expect("kill the child test process");
        self.child.wait().expect("wait for the child test process");
        let bytes = self
            .stderr
            .take()
            .map(|h| h.join().unwrap_or_default())
            .unwrap_or_default();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// Starts the test `test` in a child process with `role` and `vars`, as
/// [`run`] does, and returns without waiting. Its standard output is
/// piped to the caller; its standard error is drained by a thread, read
/// by [`Spawned::kill`].
///
/// Both pipes are read at the same time, by the caller and by the drain:
/// a child that fills a pipe nobody reads blocks (`Stdio::piped`, std
/// documentation), and `Child::wait_with_output` reads both at once for
/// that reason.
///
/// # Panics
///
/// If the child cannot be started.
pub(crate) fn spawn(test: &str, role: &str, vars: &[(&str, &OsStr)]) -> Spawned {
    let exe = std::env::current_exe().expect("path of the test binary");
    let mut child = Command::new(exe)
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env(ROLE_VAR, role)
        .envs(vars.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start the child test process");
    let mut pipe = child.stderr.take().expect("the child's standard error");
    let stderr = std::thread::spawn(move || {
        let mut buf = Vec::new();
        // A read error ends the drain; what was read is kept.
        let _ = pipe.read_to_end(&mut buf);
        buf
    });
    Spawned {
        child,
        stderr: Some(stderr),
    }
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
        let out = run(&test, "fail", &[]);
        assert!(!out.status.success());
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("the child fails on purpose"), "{stderr}");
    }

    /// rmp #60: the standard error of a spawned child is kept and returned
    /// by [`Spawned::kill`], so that a failing check can print it.
    #[test]
    fn a_spawned_child_s_standard_error_is_kept() {
        if role().is_some() {
            eprintln!("STDERR MARK");
            panic!("the spawned child fails on purpose");
        }
        let test = test_path(module_path!(), "a_spawned_child_s_standard_error_is_kept");
        let mut c = spawn(&test, "stderr", &[]);
        let mut stdout = String::new();
        c.child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        assert!(stdout.contains("1 failed"), "{stdout}");
        let stderr = c.kill();
        assert!(stderr.contains("STDERR MARK"), "{stderr}");
        assert!(
            stderr.contains("the spawned child fails on purpose"),
            "{stderr}"
        );
    }
}
