// SPDX-License-Identifier: Apache-2.0
//! Unconfined `git` for the two project surfaces that P1B Slice 2A did **not**
//! route, and the reasons routing them is not a one-line change.
//!
//! # Why this file exists at all
//!
//! `catalog.rs` used to own every project `git` launch behind a single
//! `run_git_bounded` helper. Slice 2A routed the model-reachable *read* path
//! (branch / head / dirty, reached by `ToolCall::ListProjects` and every
//! inventory push) through the P1 execution broker. That left two callers that
//! cannot simply follow it:
//!
//! * `lifecycle.rs` — `git init` in a freshly created project directory.
//! * `managed_worktree.rs` — `git worktree add --detach <destination>`.
//!
//! # Why they are not routed here
//!
//! `managed_worktree` is the load-bearing reason. `workspace_git_plan` derives
//! `SandboxPlan::Confined { writable_roots: vec![source_root] }` — a plan that
//! names exactly one writable root. But `ensure_managed_worktree_root`
//! deliberately places the destination **outside** `source_root`, and rejects
//! the operation with `managed_worktree_root_unsafe` if it is inside. Routing
//! that call through the broker would therefore break the feature *by design*:
//! the sandbox would deny the very write the feature exists to perform.
//!
//! Giving it a correct plan is not a mechanical change either. The destination
//! is computed from `allowed_roots` at runtime and must be added as a second
//! writable root, which means the plan has to be derived from two roots rather
//! than one — a change to `workspace_git_plan`'s contract, i.e. the
//! authority-threading work that Slice 2A explicitly does not do.
//!
//! `lifecycle.rs`'s `git init` has the same shape of problem in miniature: it
//! writes into a directory the broker was not told about, and the directory may
//! not even be a git repository yet, which is the case `workspace_git_plan` is
//! designed to reject.
//!
//! # What this file therefore is
//!
//! It is the *named* home of the remaining unconfined project git, so that
//! "catalog no longer launches git" is a structural fact a guard can assert
//! rather than a claim about a helper that was renamed. It is deliberately not
//! a migration and deliberately not a general-purpose launcher: the two callers
//! have distinct authority needs that a shared, more permissive helper would
//! only hide.
//!
//! Both callers remain model-reachable and both remain **unrouted**. That is an
//! open P1B item, tracked as the follow-up to this file — not a resolution.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use webcodex_process::{GracefulTermination, ManagedChild};

/// Tree shutdown also has to let the bounded stdout/stderr readers observe EOF.
/// Darwin process-group teardown and reader scheduling can legitimately take
/// longer than 500ms on loaded native CI hosts, so keep a short but realistic
/// bounded cleanup budget rather than turning successful direct-child exit into
/// a spurious reader-timeout failure.
const PROJECT_GIT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const PROJECT_GIT_OUTPUT_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug)]
pub(super) struct UnconfinedGitOutput {
    pub(super) status: std::process::ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) stdout_capped: bool,
    pub(super) stderr_capped: bool,
}

fn spawn_bounded_git_reader(
    mut pipe: impl Read + Send + 'static,
) -> (mpsc::Receiver<(Vec<u8>, bool)>, thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::sync_channel(1);
    let handle = thread::spawn(move || {
        let mut retained = Vec::with_capacity(PROJECT_GIT_OUTPUT_MAX_BYTES.min(8192));
        let mut chunk = [0_u8; 8192];
        let mut capped = false;
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    let remaining = PROJECT_GIT_OUTPUT_MAX_BYTES.saturating_sub(retained.len());
                    let keep = remaining.min(read);
                    retained.extend_from_slice(&chunk[..keep]);
                    capped |= keep < read;
                }
                Err(_) => break,
            }
        }
        let _ = tx.send((retained, capped));
    });
    (rx, handle)
}

/// Terminate the whole Git process tree within one shared cleanup deadline,
/// then reap the direct child and confirm the complete tree exited.
///
/// The platform tree isolation lives in [`ManagedChild`]: a private process
/// group on Unix, a kill-on-close Job Object on Windows. Phase 1 (Unix only)
/// requests graceful tree termination and gives the tree a short bounded grace
/// to exit on its own; Windows reports [`GracefulTermination::Unsupported`]
/// and skips straight to phase 2. Phase 2 forcefully terminates any tree that
/// is still alive. Then the direct child is reaped and the complete tree (not
/// just the direct child) is confirmed exited — all within `deadline`. The
/// direct child's `ExitStatus`, when it can still be obtained, is returned;
/// failures are joined into one error string, but cleanup never gives up early
/// because a graceful request failed.
fn terminate_project_git_tree(
    child: &mut ManagedChild,
    deadline: Instant,
) -> Result<Option<std::process::ExitStatus>, String> {
    let mut errors = Vec::new();

    match child.request_terminate_tree() {
        Ok(GracefulTermination::Requested) => {
            // The whole tree received a graceful termination request. Give it a
            // short bounded grace to exit on its own; the grace never
            // extends past the overall cleanup deadline.
            let grace_deadline = deadline.min(Instant::now() + Duration::from_millis(50));
            let remaining = grace_deadline.saturating_duration_since(Instant::now());
            match child.wait_tree_exit(remaining) {
                Ok(_) => {}
                Err(error) => {
                    errors.push(format!("git graceful termination wait failed: {error}"));
                }
            }
        }
        Ok(GracefulTermination::AlreadyExited) => {
            // The owned tree was already fully gone; nothing to signal or wait for.
        }
        Ok(GracefulTermination::Unsupported) => {
            // Windows: no generic graceful tree termination. Escalate below.
        }
        Err(error) => {
            errors.push(format!("git graceful termination request failed: {error}"));
        }
    }

    // Forceful phase: any tree still alive is terminated as a whole.
    let tree_alive = match child.try_tree_exit() {
        Ok(exited) => !exited,
        Err(error) => {
            errors.push(format!("git tree liveness probe failed: {error}"));
            true
        }
    };
    if tree_alive {
        if let Err(error) = child.terminate_tree() {
            errors.push(format!("git tree termination failed: {error}"));
        }
    }

    // Reap the direct child within the remaining deadline.
    let mut status = None;
    loop {
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                status = Some(exit_status);
                break;
            }
            Ok(None) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    errors.push("git child reap timed out".to_string());
                    break;
                }
                thread::sleep(Duration::from_millis(10).min(remaining));
            }
            Err(error) => {
                errors.push(format!("git child reap failed: {error}"));
                break;
            }
        }
    }

    // Confirm the complete tree exited, not just the direct child. Forceful
    // termination can complete asynchronously (notably Job Object teardown on
    // Windows), so use the remaining shared cleanup budget rather than a
    // single instantaneous probe.
    let remaining = deadline.saturating_duration_since(Instant::now());
    match child.wait_tree_exit(remaining) {
        Ok(true) => {}
        Ok(false) => errors.push("git process tree did not exit before deadline".to_string()),
        Err(error) => errors.push(format!("git tree exit wait failed: {error}")),
    }

    if errors.is_empty() {
        Ok(status)
    } else {
        Err(errors.join("; "))
    }
}

/// Run `git <args>` **unconfined**, for the two callers documented at the top of
/// this file.
///
/// # Not model-safe, and not claimed to be
///
/// This preserves the pre-Slice-2A behaviour exactly, including the full
/// inherited environment, so that routing the catalog read path cannot be blamed
/// for a regression in worktree creation. It remains an open normalization
/// item; the guard in `normalization_p1_tests.rs` asserts that it is *named*
/// here rather than growing anywhere else.
pub(super) fn run_unconfined_git_bounded(
    program: &str,
    path: &Path,
    args: &[&str],
    timeout: Duration,
    shutdown: Option<&AtomicBool>,
) -> Result<UnconfinedGitOutput, String> {
    if shutdown.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
        return Err("git stopped during runner shutdown".to_string());
    }
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // ManagedChild owns the whole Git process tree: a private process group on
    // Unix, a kill-on-close Job Object on Windows. Spawn remains direct process
    // spawning with the standard Command spawn failure semantics.
    let mut child = match ManagedChild::spawn(&mut command) {
        Ok(child) => child,
        Err(error) => return Err(format!("failed to spawn git: {error}")),
    };
    let Some(stdout) = child.child_mut().stdout.take() else {
        let cleanup_deadline = Instant::now() + PROJECT_GIT_CLEANUP_TIMEOUT;
        let _ = terminate_project_git_tree(&mut child, cleanup_deadline);
        return Err("git stdout pipe was unavailable".to_string());
    };
    let Some(stderr) = child.child_mut().stderr.take() else {
        drop(stdout);
        let cleanup_deadline = Instant::now() + PROJECT_GIT_CLEANUP_TIMEOUT;
        let _ = terminate_project_git_tree(&mut child, cleanup_deadline);
        return Err("git stderr pipe was unavailable".to_string());
    };
    let (stdout_rx, stdout_reader) = spawn_bounded_git_reader(stdout);
    let (stderr_rx, stderr_reader) = spawn_bounded_git_reader(stderr);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                let stopping = shutdown.is_some_and(|flag| flag.load(Ordering::SeqCst));
                if stopping || Instant::now() >= deadline {
                    // Cleanup and then report the stopping cause: the cleanup
                    // outcome is deliberately not allowed to replace the
                    // user-visible timeout/shutdown error.
                    let _ = terminate_project_git_tree(
                        &mut child,
                        Instant::now() + PROJECT_GIT_CLEANUP_TIMEOUT,
                    );
                    return Err(if stopping {
                        "git stopped during runner shutdown".to_string()
                    } else {
                        "git command timed out".to_string()
                    });
                }
                thread::sleep(
                    Duration::from_millis(10)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(error) => {
                let _ = terminate_project_git_tree(
                    &mut child,
                    Instant::now() + PROJECT_GIT_CLEANUP_TIMEOUT,
                );
                return Err(format!("failed to wait for git: {error}"));
            }
        }
    };

    // A helper descendant must not keep either pipe open after Git itself
    // exits. Direct-child exit alone is not tree exit: if descendants remain,
    // clean up the surviving tree, then drain the bounded readers — all within
    // one shared cleanup deadline so no operation gets a fresh independent one.
    let cleanup_deadline = Instant::now() + PROJECT_GIT_CLEANUP_TIMEOUT;
    match child.try_tree_exit() {
        Ok(true) => {}
        Ok(false) | Err(_) => {
            let _ = terminate_project_git_tree(&mut child, cleanup_deadline);
        }
    }
    let stdout = stdout_rx
        .recv_timeout(cleanup_deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "git stdout reader timed out".to_string())?;
    let stderr = stderr_rx
        .recv_timeout(cleanup_deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "git stderr reader timed out".to_string())?;
    if stdout_reader.is_finished() {
        let _ = stdout_reader.join();
    }
    if stderr_reader.is_finished() {
        let _ = stderr_reader.join();
    }
    Ok(UnconfinedGitOutput {
        status,
        stdout: stdout.0,
        stderr: stderr.0,
        stdout_capped: stdout.1,
        stderr_capped: stderr.1,
    })
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[cfg(feature = "runner-real-process-tests")]
    use std::path::PathBuf;
    #[cfg(feature = "runner-real-process-tests")]
    use std::sync::{Arc, OnceLock};
    #[cfg(feature = "runner-real-process-tests")]
    use std::time::SystemTime;

    // -----------------------------------------------------------------------
    // Tree-lifecycle regression coverage for the *unconfined* launcher this
    // module owns.
    //
    // These scenarios run the real `validation_tree_helper` fixture (compiled at
    // test time with rustc, exactly like the validation and job tree tests)
    // through the `run_unconfined_git_bounded` seam, so the same
    // tests run on Windows and Unix without cmd, PowerShell, or bash. Each
    // test tracks the real parent/descendant pids written to marker files and
    // probes them with platform-native APIs, and every test reaps the tree it
    // starts before returning.
    // -----------------------------------------------------------------------

    /// Compiled copy of the `validation_tree_helper` fixture, kept alive for
    /// the whole test process so its binary path never disappears under a
    /// running descendant.
    #[cfg(feature = "runner-real-process-tests")]
    struct GitTreeHelper {
        _temp: tempfile::TempDir,
        path: PathBuf,
    }

    #[cfg(feature = "runner-real-process-tests")]
    static GIT_TREE_HELPER: OnceLock<Arc<GitTreeHelper>> = OnceLock::new();

    #[cfg(feature = "runner-real-process-tests")]
    fn helper_binary() -> PathBuf {
        GIT_TREE_HELPER
            .get_or_init(|| {
                let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("src/webcodex_runner/validation/validation_tree_helper.rs");
                let temp = tempfile::tempdir().unwrap();
                let output = temp
                    .path()
                    .join(format!("git-tree-helper{}", std::env::consts::EXE_SUFFIX));
                let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
                let result = Command::new(rustc)
                    .arg("--edition=2021")
                    .arg("--crate-name=webcodex_git_tree_helper")
                    .arg(&source)
                    .arg("-o")
                    .arg(&output)
                    .output()
                    .expect("run rustc for git tree helper");
                assert!(
                    result.status.success(),
                    "git tree helper compilation failed: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
                Arc::new(GitTreeHelper {
                    _temp: temp,
                    path: output,
                })
            })
            .path
            .clone()
    }

    #[cfg(feature = "runner-real-process-tests")]
    fn str_args(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    /// A unique temp file, removed on drop.
    #[cfg(feature = "runner-real-process-tests")]
    struct CleanupPath(PathBuf);

    #[cfg(feature = "runner-real-process-tests")]
    impl std::ops::Deref for CleanupPath {
        type Target = PathBuf;
        fn deref(&self) -> &PathBuf {
            &self.0
        }
    }

    #[cfg(feature = "runner-real-process-tests")]
    impl Drop for CleanupPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[cfg(feature = "runner-real-process-tests")]
    fn unique_temp_path(tag: &str) -> CleanupPath {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "wc-project-git-{tag}-{}-{nanos}",
            std::process::id()
        ));
        CleanupPath(path)
    }

    #[cfg(feature = "runner-real-process-tests")]
    fn wait_until_file(path: &Path, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            if path.exists() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Parse `KEY=<pid>` from a marker file written by the helper.
    #[cfg(feature = "runner-real-process-tests")]
    fn read_pid(marker: &Path, key: &str) -> u32 {
        let text = std::fs::read_to_string(marker).expect("read pid marker");
        text.lines()
            .find_map(|line| {
                line.strip_prefix(key)
                    .and_then(|rest| rest.strip_prefix('='))
                    .and_then(|value| value.trim().parse().ok())
            })
            .unwrap_or_else(|| panic!("marker {marker:?} missing {key}: {text}"))
    }

    #[cfg(feature = "runner-real-process-tests")]
    #[cfg(windows)]
    fn process_alive(pid: u32) -> bool {
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SAFETY: OpenProcess returns a handle or NULL; NULL means the pid no
        // longer exists (or is inaccessible, which also means not ours).
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return false;
        }
        let mut exit_code = 0u32;
        // SAFETY: `handle` is valid; `exit_code` is a valid out-param.
        let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
        // SAFETY: close the handle we opened.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
        ok == 1 && exit_code == 259 // 259 == STILL_ACTIVE
    }

    #[cfg(feature = "runner-real-process-tests")]
    #[cfg(target_os = "linux")]
    fn process_alive(pid: u32) -> bool {
        // `kill(pid, 0)` also succeeds for zombies, while ManagedChild's Linux
        // tree-liveness contract deliberately treats zombies as unable to run.
        // Use /proc to align this test probe with that contract, but fall back
        // conservatively if procfs cannot be read or parsed.
        // SAFETY: signal 0 is an existence probe; the pid comes from our own
        // test helper.
        if (unsafe { libc::kill(pid as i32, 0) }) != 0 {
            return false;
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return true;
        };
        let Some((_, rest)) = stat.rsplit_once(')') else {
            return true;
        };
        let state = rest.split_whitespace().next().unwrap_or("");
        state != "Z" && state != "X"
    }

    #[cfg(feature = "runner-real-process-tests")]
    #[cfg(all(unix, not(target_os = "linux")))]
    fn process_alive(pid: u32) -> bool {
        // SAFETY: signal 0 is an existence probe; the pid comes from our own
        // test helper. Non-Linux Unix test hosts reap orphaned descendants
        // promptly, so a successful probe represents a live process here.
        (unsafe { libc::kill(pid as i32, 0) }) == 0
    }

    /// Upper bound for the whole test body including cleanup; the fixture
    /// sleeps far longer (600s), so any run exceeding this is a cleanup hang,
    /// not a slow exit.
    #[cfg(feature = "runner-real-process-tests")]
    const BOUNDEDNESS_LIMIT: Duration = Duration::from_secs(15);

    /// A. Normal completion: a short-lived process exits successfully, its
    /// stdout/stderr are collected, and no cleanup stall occurs.
    #[test]
    #[cfg(feature = "runner-real-process-tests")]
    #[ignore = "runner real-process lane: spawns the Git ManagedChild process-tree fixture"]
    fn runner_real_process_git_normal_completion_collects_output_and_returns_bounded() {
        let cwd = tempfile::tempdir().unwrap();
        let program = helper_binary();
        let started = Instant::now();
        let output = run_unconfined_git_bounded(
            &program.to_string_lossy(),
            cwd.path(),
            &["sleep", "0", "7"],
            Duration::from_secs(10),
            None,
        )
        .expect("normal completion must succeed");
        assert_eq!(output.status.code(), Some(7));
        assert!(!output.stdout_capped);
        assert!(!output.stderr_capped);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("VALIDATION_HELPER_STDOUT"), "{stdout}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("VALIDATION_HELPER_STDERR"), "{stderr}");
        assert!(
            started.elapsed() < BOUNDEDNESS_LIMIT,
            "normal completion was not bounded"
        );
    }

    /// B. Explicit timeout kills the whole tree: the direct Git process and
    /// its pipe-holding descendant must both die, with the timeout error
    /// unchanged.
    #[test]
    #[cfg(feature = "runner-real-process-tests")]
    #[ignore = "runner real-process lane: spawns the Git ManagedChild process-tree fixture"]
    fn runner_real_process_git_timeout_terminates_whole_tree() {
        let parent_marker = unique_temp_path("timeout-parent");
        let alive_marker = unique_temp_path("timeout-desc");
        let cwd = tempfile::tempdir().unwrap();
        let program = helper_binary();
        let args = str_args(&[
            "spawn-descendant-keepalive",
            parent_marker.to_str().unwrap(),
            alive_marker.to_str().unwrap(),
            "600",
        ]);
        let started = Instant::now();
        let result = thread::scope(|scope| {
            let handle = scope.spawn(|| {
                run_unconfined_git_bounded(
                    &program.to_string_lossy(),
                    cwd.path(),
                    &args.iter().map(String::as_str).collect::<Vec<_>>(),
                    Duration::from_secs(2),
                    None,
                )
            });
            assert!(
                wait_until_file(&parent_marker, Duration::from_secs(5)),
                "parent marker never appeared"
            );
            assert!(
                wait_until_file(&alive_marker, Duration::from_secs(5)),
                "descendant marker never appeared"
            );
            let parent_pid = read_pid(&parent_marker, "PARENT_PID");
            let descendant_pid = read_pid(&parent_marker, "DESCENDANT_PID");
            // Both sleep 600s while the timeout is 2s, so both must still be
            // alive when the timeout fires.
            assert!(process_alive(parent_pid), "parent not alive before timeout");
            assert!(
                process_alive(descendant_pid),
                "descendant not alive before timeout"
            );
            handle.join().expect("run_git_bounded panicked")
        });
        let error = match result {
            Ok(_) => panic!("run_git_bounded must report a timeout, not success"),
            Err(error) => error,
        };
        assert_eq!(error, "git command timed out");
        assert!(
            started.elapsed() < BOUNDEDNESS_LIMIT,
            "timeout cleanup not bounded"
        );
        assert!(
            !process_alive(read_pid(&parent_marker, "PARENT_PID")),
            "Git parent survived timeout cleanup"
        );
        assert!(
            !process_alive(read_pid(&parent_marker, "DESCENDANT_PID")),
            "Git descendant survived timeout cleanup"
        );
    }

    /// C. Runner shutdown terminates the whole tree with the shutdown error
    /// unchanged. Works on Windows and Linux.
    #[test]
    #[cfg(feature = "runner-real-process-tests")]
    #[ignore = "runner real-process lane: spawns the Git ManagedChild process-tree fixture"]
    fn runner_real_process_git_runner_shutdown_terminates_whole_tree() {
        let parent_marker = unique_temp_path("shutdown-parent");
        let alive_marker = unique_temp_path("shutdown-desc");
        let cwd = tempfile::tempdir().unwrap();
        let program = helper_binary();
        let args = str_args(&[
            "spawn-descendant-keepalive",
            parent_marker.to_str().unwrap(),
            alive_marker.to_str().unwrap(),
            "600",
        ]);
        let shutdown = AtomicBool::new(false);
        let started = Instant::now();
        let result = thread::scope(|scope| {
            let handle = scope.spawn(|| {
                run_unconfined_git_bounded(
                    &program.to_string_lossy(),
                    cwd.path(),
                    &args.iter().map(String::as_str).collect::<Vec<_>>(),
                    Duration::from_secs(60),
                    Some(&shutdown),
                )
            });
            assert!(
                wait_until_file(&parent_marker, Duration::from_secs(5)),
                "parent marker never appeared"
            );
            assert!(
                wait_until_file(&alive_marker, Duration::from_secs(5)),
                "descendant marker never appeared"
            );
            let parent_pid = read_pid(&parent_marker, "PARENT_PID");
            let descendant_pid = read_pid(&parent_marker, "DESCENDANT_PID");
            assert!(
                process_alive(parent_pid),
                "parent not alive before shutdown"
            );
            assert!(
                process_alive(descendant_pid),
                "descendant not alive before shutdown"
            );
            shutdown.store(true, Ordering::SeqCst);
            handle.join().expect("run_git_bounded panicked")
        });
        let error = match result {
            Ok(_) => panic!("run_git_bounded must report shutdown, not success"),
            Err(error) => error,
        };
        assert_eq!(error, "git stopped during runner shutdown");
        assert!(
            started.elapsed() < BOUNDEDNESS_LIMIT,
            "shutdown cleanup not bounded"
        );
        assert!(
            !process_alive(read_pid(&parent_marker, "PARENT_PID")),
            "Git parent survived runner shutdown"
        );
        assert!(
            !process_alive(read_pid(&parent_marker, "DESCENDANT_PID")),
            "Git descendant survived runner shutdown"
        );
    }

    /// D. The direct Git process exits while its descendant survives and holds
    /// the captured pipes. Direct-child exit alone must not finish cleanup:
    /// the surviving tree is terminated, the readers reach EOF, and
    /// run_git_bounded returns without an indefinite reader wait.
    #[test]
    #[cfg(feature = "runner-real-process-tests")]
    #[ignore = "runner real-process lane: spawns the Git ManagedChild process-tree fixture"]
    fn runner_real_process_git_parent_exit_alone_does_not_finish_cleanup() {
        let parent_marker = unique_temp_path("parent-first");
        let alive_marker = unique_temp_path("parent-first-desc");
        let cwd = tempfile::tempdir().unwrap();
        let program = helper_binary();
        let args = str_args(&[
            "spawn-descendant",
            parent_marker.to_str().unwrap(),
            alive_marker.to_str().unwrap(),
            "600",
        ]);
        let started = Instant::now();
        let output = thread::scope(|scope| {
            let handle = scope.spawn(|| {
                run_unconfined_git_bounded(
                    &program.to_string_lossy(),
                    cwd.path(),
                    &args.iter().map(String::as_str).collect::<Vec<_>>(),
                    Duration::from_secs(30),
                    None,
                )
            });
            // The direct child exits almost immediately after spawning its
            // descendant. The descendant's marker appears only if it actually
            // ran, so its existence proves the descendant was alive after the
            // direct child exited.
            assert!(
                wait_until_file(&alive_marker, Duration::from_secs(5)),
                "descendant marker never appeared"
            );
            handle.join().expect("run_git_bounded panicked")
        })
        .expect("direct-parent exit must not turn into an error");
        assert!(
            output.status.success(),
            "direct child exited 0; tree cleanup must not change its status"
        );
        // The captured stdout contains the helper's pid line only when the
        // reader hit EOF, which requires every descendant holding the pipe to
        // be gone. A cleanup that stops at the direct child leaves stdout
        // stuck at the un-flushed line or empty.
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("DESCENDANT_PID="),
            "stdout reader never reached EOF: {stdout}"
        );
        assert!(
            !process_alive(read_pid(&parent_marker, "DESCENDANT_PID")),
            "descendant survived cleanup after direct child exit"
        );
        assert!(
            started.elapsed() < BOUNDEDNESS_LIMIT,
            "parent-exit cleanup not bounded"
        );
    }

    /// E. A SIGTERM-resistant tree is escalated to force: the graceful request
    /// gets a short bounded grace, then the whole tree is killed. Never
    /// unbounded. (Windows has no generic graceful tree termination, so there
    /// is nothing to escalate from there.)
    #[cfg(unix)]
    #[test]
    #[cfg(feature = "runner-real-process-tests")]
    #[ignore = "runner real-process lane: spawns the Git ManagedChild process-tree fixture"]
    fn runner_real_process_git_sigterm_resistant_tree_is_forcefully_escalated() {
        let parent_marker = unique_temp_path("resist-parent");
        let alive_marker = unique_temp_path("resist-desc");
        let cwd = tempfile::tempdir().unwrap();
        let program = helper_binary();
        let args = str_args(&[
            "ignore-term-keepalive",
            parent_marker.to_str().unwrap(),
            alive_marker.to_str().unwrap(),
            "600",
        ]);
        let started = Instant::now();
        let result = thread::scope(|scope| {
            let handle = scope.spawn(|| {
                run_unconfined_git_bounded(
                    &program.to_string_lossy(),
                    cwd.path(),
                    &args.iter().map(String::as_str).collect::<Vec<_>>(),
                    Duration::from_secs(2),
                    None,
                )
            });
            assert!(
                wait_until_file(&parent_marker, Duration::from_secs(5)),
                "parent marker never appeared"
            );
            assert!(
                wait_until_file(&alive_marker, Duration::from_secs(5)),
                "descendant marker never appeared"
            );
            let parent_pid = read_pid(&parent_marker, "PARENT_PID");
            let descendant_pid = read_pid(&parent_marker, "DESCENDANT_PID");
            assert!(process_alive(parent_pid), "parent not alive before timeout");
            assert!(
                process_alive(descendant_pid),
                "descendant not alive before timeout"
            );
            handle.join().expect("run_git_bounded panicked")
        });
        let error = match result {
            Ok(_) => panic!("run_git_bounded must report a timeout, not success"),
            Err(error) => error,
        };
        assert_eq!(error, "git command timed out");
        // Both processes ignore SIGTERM (inherited SIG_IGN), so only the
        // forceful escalation can have ended them.
        assert!(
            !process_alive(read_pid(&parent_marker, "PARENT_PID")),
            "SIGTERM-resistant parent survived escalation"
        );
        assert!(
            !process_alive(read_pid(&parent_marker, "DESCENDANT_PID")),
            "SIGTERM-resistant descendant survived escalation"
        );
        assert!(
            started.elapsed() < BOUNDEDNESS_LIMIT,
            "SIGTERM-resistant cleanup not bounded"
        );
    }

    /// F. Spawn failure keeps the standard direct-spawn failure semantics with
    /// the existing user-visible error prefix.
    #[test]
    fn spawn_failure_reports_spawn_error() {
        let cwd = tempfile::tempdir().unwrap();
        let error = match run_unconfined_git_bounded(
            "webcodex-git-command-that-does-not-exist-xyz",
            cwd.path(),
            &["--version"],
            Duration::from_secs(5),
            None,
        ) {
            Ok(_) => panic!("spawn of a nonexistent executable must fail"),
            Err(error) => error,
        };
        assert!(
            error.starts_with("failed to spawn git"),
            "unexpected spawn error: {error}"
        );
    }
}
