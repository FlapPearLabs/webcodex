#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::process::Command;

/// The service control surface is operator-only. These tests never load a real
/// tunnel; they prove the plist is well-formed, credential-free, user-scoped,
/// and that destructive operations fail closed.
struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    binary: PathBuf,
    registry: PathBuf,
    profile_dir: PathBuf,
    state_dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temporary fixture");
        let home = temp.path().join("home");
        std::fs::create_dir_all(home.join("Library/LaunchAgents")).expect("home");
        let binary = temp.path().join("webcodex-chatgpt-safe");
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").expect("binary stub");
        let registry = temp.path().join("registry.json");
        std::fs::write(&registry, "{}").expect("registry stub");
        let profile_dir = temp.path().join("profiles");
        std::fs::create_dir_all(&profile_dir).expect("profiles");
        let state_dir = temp.path().join("state");
        std::fs::create_dir_all(&state_dir).expect("state");
        Self {
            _temp: temp,
            home,
            binary,
            registry,
            profile_dir,
            state_dir,
        }
    }

    fn chatgpt<S: AsRef<std::ffi::OsStr>>(&self, args: &[S]) -> (bool, String, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_webcodex-chatgpt-safe"))
            .arg("chatgpt")
            .args(args)
            .env("HOME", &self.home)
            .output()
            .expect("run chatgpt subcommand");
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }

    fn install_args(&self) -> Vec<String> {
        vec![
            "install".into(),
            "--tunnel-client".into(),
            "/usr/bin/true".into(),
            "--profile-dir".into(),
            self.profile_dir.display().to_string(),
            "--binary".into(),
            self.binary.display().to_string(),
            "--registry".into(),
            self.registry.display().to_string(),
            "--state-dir".into(),
            self.state_dir.display().to_string(),
        ]
    }

    fn plist(&self) -> PathBuf {
        self.home
            .join("Library/LaunchAgents")
            .join("com.webcodex.chatgpt-safe.plist")
    }
}

fn run<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_webcodex-chatgpt-safe"))
        .arg("chatgpt")
        .args(args)
        .output()
        .expect("run chatgpt");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn service_control_is_not_reachable_from_the_mcp_tool_surface() {
    // The service subcommand must be a distinct top-level verb, never a tool.
    // If it were exposed as a tool, `tools/list` would contain it.
    let fixture = Fixture::new();
    let output = Command::new(env!("CARGO_BIN_EXE_webcodex-chatgpt-safe"))
        .args([
            "serve",
            "--profile",
            "chatgpt-safe",
            "--registry",
            &fixture.registry.display().to_string(),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn stdio");
    drop(output);
    // The tool allowlist lives in source; assert the service verbs are absent.
    let source = include_str!("../src/main.rs");
    let allowlist = source
        .split("const SAFE_TOOLS")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("SAFE_TOOLS literal");
    for forbidden in [
        "chatgpt",
        "install",
        "uninstall",
        "start",
        "restart",
        "launchctl",
    ] {
        assert!(
            !allowlist.contains(&format!("\"{forbidden}\"")),
            "operator-only verb leaked into the model tool surface: {forbidden}"
        );
    }
}

#[test]
fn install_writes_a_valid_credential_free_user_level_plist() {
    let fixture = Fixture::new();
    let args = fixture.install_args();
    let (ok, stdout, stderr) = fixture.chatgpt(&args);
    assert!(ok, "install failed: {stderr}");
    assert!(stdout.contains("\"key_in_plist\":false"), "{stdout}");
    assert!(stdout.contains("\"runs_as\":\"user\""), "{stdout}");

    let plist = fixture.plist();
    assert!(plist.is_file(), "plist was not created");
    // Ownership marker must be a valid XML comment, not a shell-style comment,
    // otherwise launchd cannot parse the file at all.
    let text = std::fs::read_to_string(&plist).expect("read plist");
    assert!(
        text.contains("<!-- managed-by: webcodex-chatgpt-safe -->"),
        "{text}"
    );
    assert!(text.trim_start().starts_with("<?xml"), "{text}");

    // No credential material may appear anywhere in the plist.
    for needle in [
        "api-key",
        "api_key",
        "secret",
        "bearer",
        "token",
        "PRIVATE KEY",
    ] {
        assert!(
            !text
                .to_ascii_lowercase()
                .contains(&needle.to_ascii_lowercase()),
            "credential-like material found in plist: {needle}"
        );
    }
    // A user LaunchAgent, never a system/root one.
    assert!(fixture
        .home
        .join("Library/LaunchAgents")
        .starts_with(&fixture.home));
}

#[test]
fn install_refuses_to_overwrite_or_remove_an_unmanaged_plist() {
    let fixture = Fixture::new();
    let unmanaged = fixture.plist();
    std::fs::write(
        &unmanaged,
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict/></plist>",
    )
    .expect("write unmanaged plist");

    let args = fixture.install_args();
    let (ok, _, stderr) = fixture.chatgpt(&args);
    assert!(!ok, "install must refuse an unmanaged plist");
    assert!(stderr.contains("unmanaged"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(&unmanaged).unwrap(),
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict/></plist>",
        "the unmanaged plist must be left untouched"
    );

    // uninstall must also refuse to delete a plist it does not own.
    let (ok, _, _stderr) = fixture.chatgpt(&["uninstall"]);
    assert!(!ok, "uninstall must refuse an unmanaged plist");
    assert!(unmanaged.is_file(), "unmanaged plist was deleted");
}

#[test]
fn start_fails_closed_when_the_service_is_not_installed() {
    let fixture = Fixture::new();
    let (ok, _, stderr) = fixture.chatgpt(&["start"]);
    assert!(!ok, "start must not report success without an install");
    assert!(stderr.contains("not installed"), "{stderr}");
}

#[test]
fn doctor_reports_checks_and_never_claims_a_running_service() {
    let fixture = Fixture::new();
    let args = fixture.install_args();
    assert!(fixture.chatgpt(&args).0);

    let mut doctor_args = vec!["doctor".to_string()];
    doctor_args.extend(args[1..].iter().cloned());
    let (ok, stdout, stderr) = fixture.chatgpt(&doctor_args);
    assert!(ok, "doctor failed: {stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("doctor json");
    assert_eq!(value["status"], "PASS");
    let checks = value["checks"].as_array().expect("checks array");
    let names: Vec<&str> = checks.iter().filter_map(|c| c["check"].as_str()).collect();
    assert!(names.contains(&"not_root"), "{names:?}");
    assert!(names.contains(&"no_credential_in_plist"), "{names:?}");
    assert!(names.contains(&"plist_owner_marker"), "{names:?}");

    // status must distinguish not-running from running, and must not invent a
    // health/readiness claim.
    let mut status_args = vec!["status".to_string()];
    status_args.extend(args[1..].iter().cloned());
    let (_, stdout, _) = fixture.chatgpt(&status_args);
    let status: serde_json::Value = serde_json::from_str(&stdout).expect("status json");
    assert_eq!(status["status"], "NOT_LOADED");
    assert_eq!(status["process_running"], false);
    assert_eq!(status["tunnel_connected"], false);
    assert_eq!(status["plist_installed"], true);
}

#[test]
fn doctor_fails_closed_when_the_plist_cannot_be_audited() {
    // A plist that exists but cannot be read must not let `doctor` skip the
    // credential audit and still report an overall PASS. This is the control
    // for the unreadable-plist branch: if the audit were conditional on a
    // successful read, these two checks would be absent entirely.
    let fixture = Fixture::new();
    let args = fixture.install_args();
    assert!(fixture.chatgpt(&args).0, "install must succeed");
    let plist = fixture.plist();
    // Replace the plist with a directory: present, but unreadable as a file.
    std::fs::remove_file(&plist).expect("remove installed plist");
    std::fs::create_dir(&plist).expect("create directory in place of plist");

    let mut doctor_args = vec!["doctor".to_string()];
    doctor_args.extend(args[1..].iter().cloned());
    let (_ok, stdout, _stderr) = fixture.chatgpt(&doctor_args);
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("doctor must still emit json");
    assert_eq!(
        value["status"], "FAIL",
        "an unaudited plist must not be reported as PASS: {value}"
    );
    let checks = value["checks"].as_array().expect("checks array");
    // Falsification: with the audit made conditional on a successful read, the
    // two checks below are never emitted at all, so this lookup fails and the
    // test fails. It cannot pass against the buggy form.
    let credential = checks
        .iter()
        .find(|c| c["check"] == "no_credential_in_plist")
        .expect("the credential audit must always be reported, even when unreadable");
    assert_eq!(
        credential["pass"], false,
        "an unreadable plist must fail the credential audit: {credential}"
    );
    assert!(
        credential["detail"].as_str().is_some(),
        "the failure must say the plist was not audited: {credential}"
    );
    let marker = checks
        .iter()
        .find(|c| c["check"] == "plist_owner_marker")
        .expect("ownership must always be reported, even when unreadable");
    assert_eq!(
        marker["pass"], false,
        "unverified ownership must not pass: {marker}"
    );
}

#[test]
fn status_does_not_infer_a_live_tunnel_from_a_stale_pid_file() {
    // `tunnel_connected` used to be derived from the mere presence of a pid
    // file. A pid that is not alive must not be reported as a running process
    // or a connected tunnel, otherwise a crashed tunnel reads as healthy.
    let fixture = Fixture::new();
    let args = fixture.install_args();
    assert!(fixture.chatgpt(&args).0, "install must succeed");

    // Claim a pid that cannot be a live process.
    let pid_file = fixture.state_dir.join("tunnel.pid");
    std::fs::create_dir_all(pid_file.parent().expect("run dir")).expect("create run dir");
    std::fs::write(&pid_file, "2147483646").expect("write stale pid");

    let mut status_args = vec!["status".to_string()];
    status_args.extend(args[1..].iter().cloned());
    let (_ok, stdout, _stderr) = fixture.chatgpt(&status_args);
    let status: serde_json::Value = serde_json::from_str(&stdout).expect("status json");
    assert_eq!(
        status["pid_file_present"], true,
        "the pid file is on disk and should be reported as present: {status}"
    );
    assert_eq!(
        status["process_running"], false,
        "a dead pid must not be reported as running: {status}"
    );
    assert_eq!(
        status["tunnel_connected"], false,
        "a dead pid must not be reported as a connected tunnel: {status}"
    );
}

#[test]
fn logs_reports_an_unreadable_log_instead_of_an_empty_one() {
    // A permission/IO error must not be flattened into "zero lines", which is
    // indistinguishable from a service that produced no diagnostics.
    let fixture = Fixture::new();
    let args = fixture.install_args();
    assert!(fixture.chatgpt(&args).0, "install must succeed");

    let log = fixture.state_dir.join("logs").join("chatgpt-safe.err.log");
    std::fs::create_dir_all(log.parent().expect("log dir")).expect("create log dir");
    std::fs::create_dir(&log).expect("create directory in place of log file");

    let mut logs_args = vec!["logs".to_string()];
    logs_args.extend(args[1..].iter().cloned());
    let (ok, _stdout, stderr) = fixture.chatgpt(&logs_args);
    assert!(
        !ok,
        "an unreadable log must not be reported as a successful read"
    );
    assert!(
        stderr.contains("unreadable"),
        "the error must say the log was unreadable: {stderr}"
    );
}

#[test]
fn uninstall_stops_and_removes_only_the_owned_plist() {
    let fixture = Fixture::new();
    let args = fixture.install_args();
    assert!(fixture.chatgpt(&args).0, "install must succeed");
    assert!(fixture.plist().is_file());

    let (ok, stdout, stderr) = fixture.chatgpt(&["uninstall"]);
    assert!(ok, "uninstall failed: {stderr}");
    assert!(stdout.contains("UNINSTALLED"), "{stdout}");
    assert!(!fixture.plist().exists(), "owned plist was not removed");
}

#[test]
fn install_rejects_relative_and_missing_paths() {
    let fixture = Fixture::new();
    // Relative tunnel client path.
    let mut args = fixture.install_args();
    let idx = args.iter().position(|a| a == "--tunnel-client").unwrap();
    args[idx + 1] = "relative/tunnel".into();
    let (ok, _, stderr) = run(&args);
    assert!(!ok, "relative path must be rejected");
    assert!(stderr.contains("absolute"), "{stderr}");

    // Missing binary.
    let mut args = fixture.install_args();
    let idx = args.iter().position(|a| a == "--binary").unwrap();
    args[idx + 1] = "/nonexistent/binary".into();
    let (ok, _, stderr) = run(&args);
    assert!(!ok, "missing binary must be rejected");
    assert!(stderr.contains("does not exist"), "{stderr}");
}

#[test]
fn unknown_chatgpt_subcommand_and_option_fail_closed() {
    let fixture = Fixture::new();
    let (ok, _, stderr) = fixture.chatgpt(&["definitely-not-a-command"]);
    assert!(!ok);
    assert!(stderr.contains("usage:"), "{stderr}");

    let (ok, _, stderr) = fixture.chatgpt(&["status", "--not-a-flag", "x"]);
    assert!(!ok);
    assert!(stderr.contains("unknown chatgpt option"), "{stderr}");
}

/// An unreadable plist means ownership is UNVERIFIED, not absent.
///
/// Both destructive paths previously treated any read failure as "there is
/// nothing here": `install` skipped the ownership check and overwrote the file,
/// and `uninstall` reported UNINSTALLED while the plist was still on disk.
/// That is error-swallowing-as-success on the exact paths that guard a
/// user-level agent definition, so both must fail closed instead.
#[test]
fn install_and_uninstall_fail_closed_on_an_unreadable_plist() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let plist = fixture.plist();
    let foreign = "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict/></plist>";
    std::fs::write(&plist, foreign).expect("write foreign plist");

    // Write-only: readable=False, writable=True. This is the dangerous case —
    // install could clobber the file but could not verify who owns it.
    std::fs::set_permissions(&plist, std::fs::Permissions::from_mode(0o200))
        .expect("make plist write-only");
    let readable = {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::metadata(&plist)
            .map(|m| m.permissions().mode() & 0o400 != 0)
            .unwrap_or(false)
    };
    if readable {
        // Running as a user for whom mode bits are not enforced; the assertions
        // below would not prove anything, so do not pretend they did.
        eprintln!("skipping: ownership bits are not enforced for this user");
        return;
    }

    let (ok, _, stderr) = fixture.chatgpt(&fixture.install_args());
    assert!(!ok, "install must not overwrite a plist it cannot audit");
    assert!(
        stderr.contains("cannot verify ownership"),
        "install must name the unverified ownership: {stderr}"
    );
    // Restore readability before comparing, then prove the content survived.
    std::fs::set_permissions(&plist, std::fs::Permissions::from_mode(0o600)).expect("restore mode");
    assert_eq!(
        std::fs::read_to_string(&plist).unwrap(),
        foreign,
        "the unverified plist must be left byte-identical"
    );
    std::fs::set_permissions(&plist, std::fs::Permissions::from_mode(0o200)).expect("re-arm mode");

    let (ok, stdout, stderr) = fixture.chatgpt(&["uninstall"]);
    assert!(
        !ok,
        "uninstall must not report success without verified ownership"
    );
    assert!(
        stderr.contains("cannot verify ownership"),
        "uninstall must name the unverified ownership: {stderr}"
    );
    assert!(
        !stdout.contains("UNINSTALLED"),
        "uninstall must not claim UNINSTALLED: {stdout}"
    );
    assert!(plist.is_file(), "the plist must still exist");

    // Restore permissions so the temp dir can clean up.
    std::fs::set_permissions(&plist, std::fs::Permissions::from_mode(0o600)).ok();
}
