//! Operator-only macOS user LaunchAgent supervision for the ChatGPT-safe path.
//!
//! # Scope and trust boundary
//!
//! These commands are **operator controls, never model tools**. They exist so a
//! normal Mac login restores the tunnel without a terminal. Nothing in this
//! module is reachable from `tools/call`, and nothing here grants the model any
//! capability it does not already have.
//!
//! Security posture:
//!
//! * Runs as the **user**, never root. The generated plist is a *user*
//!   LaunchAgent under `~/Library/LaunchAgents`; a root-owned agent is refused.
//! * The runtime key is **never** embedded in the plist, the repo, or a log. The
//!   plist references the existing tunnel profile by path only; the key stays in
//!   the operator-owned file the profile already points at via `file:`.
//! * stdout of the supervised process is the MCP JSON-RPC channel. It is
//!   **never** redirected to a log file; only stderr is captured, and it is
//!   size-bounded and rotated.
//! * Restart is throttled by launchd (`KeepAlive` + `ThrottleInterval`) so a
//!   crash loop cannot storm the host.
//!
//! Uninstall removes only what this module created, verified by an ownership
//! marker, so it cannot delete an unrelated plist.

use serde_json::{json, Value};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const LABEL: &str = "com.webcodex.chatgpt-safe";
const PLIST_FILE: &str = "com.webcodex.chatgpt-safe.plist";
/// Ownership marker. Must be a valid XML comment: a shell-style `#` line makes
/// the whole plist unparseable, which would silently break service loading.
const OWNER_MARKER: &str = "managed-by: webcodex-chatgpt-safe";
/// Bounded stderr capture. The MCP stdout channel must never reach a file.
const MAX_STDERR_BYTES: u64 = 256 * 1024;
const MAX_STDERR_LOGS: usize = 3;
const THROTTLE_INTERVAL: u32 = 30;

/// Operator-supplied, non-secret service settings. Paths only.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub tunnel_client: PathBuf,
    pub profile_dir: PathBuf,
    pub profile_name: String,
    pub binary: PathBuf,
    pub registry: PathBuf,
    pub state_dir: PathBuf,
}

impl ServiceConfig {
    fn validate(&self) -> Result<(), String> {
        for (label, path) in [
            ("tunnel client", &self.tunnel_client),
            ("profile directory", &self.profile_dir),
            ("binary", &self.binary),
            ("registry", &self.registry),
        ] {
            if !path.is_absolute() {
                return Err(format!("{label} path must be absolute: {}", path.display()));
            }
            if !path.exists() {
                return Err(format!("{label} does not exist: {}", path.display()));
            }
        }
        if !self.tunnel_client.is_file() {
            return Err("tunnel client must be a regular file".into());
        }
        if !self.binary.is_file() {
            return Err("service binary must be a regular file".into());
        }
        if !self.registry.is_file() {
            return Err("registry must be a regular file".into());
        }
        if !self
            .profile_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err("tunnel profile name has unsupported characters".into());
        }
        Ok(())
    }

    fn run_dir(&self) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.state_dir)
            .map_err(|e| format!("cannot create state directory: {e}"))?;
        Ok(self.state_dir.clone())
    }

    fn log_dir(&self) -> Result<PathBuf, String> {
        let dir = self.run_dir()?.join("logs");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create log directory: {e}"))?;
        Ok(dir)
    }

    fn health_url_file(&self) -> Result<PathBuf, String> {
        Ok(self.run_dir()?.join("health-url"))
    }

    fn pid_file(&self) -> Result<PathBuf, String> {
        Ok(self.run_dir()?.join("tunnel.pid"))
    }
}

fn launchctl(args: &[String]) -> (bool, String) {
    match Command::new("/bin/launchctl")
        .args(args)
        .stdin(Stdio::null())
        .output()
    {
        Ok(output) => (
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ),
        Err(error) => (false, format!("launchctl is unavailable: {error}")),
    }
}

/// Escape XML text/attribute content. The plist embeds operator paths, so this
/// must not be skipped even though the values are operator-controlled: a path
/// containing markup must not be able to inject plist content.
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn render_plist(config: &ServiceConfig, stderr_path: &Path) -> Result<String, String> {
    let binary = xml_escape(&config.binary.display().to_string());
    let registry = xml_escape(&config.registry.display().to_string());
    let tunnel = xml_escape(&config.tunnel_client.display().to_string());
    let profile_dir = xml_escape(&config.profile_dir.display().to_string());
    let profile = xml_escape(&config.profile_name);
    let stderr_log = xml_escape(&stderr_path.display().to_string());
    let health_url = xml_escape(&config.health_url_file()?.display().to_string());
    let pid_file = xml_escape(&config.pid_file()?.display().to_string());
    let log_dir = xml_escape(&config.log_dir()?.display().to_string());
    let state_dir = xml_escape(&config.state_dir.display().to_string());
    // Deliberately absent: the runtime key. The tunnel profile resolves it from
    // its own `file:` reference so no key material enters this file.
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <!-- {OWNER_MARKER} -->
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{tunnel}</string>
        <string>run</string>
        <string>--profile</string>
        <string>{profile}</string>
        <string>--profile-dir</string>
        <string>{profile_dir}</string>
        <string>--health.url-file</string>
        <string>{health_url}</string>
        <string>--pid.file</string>
        <string>{pid_file}</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>WEBCODEX_CHATGPT_SAFE_BINARY</key>
        <string>{binary}</string>
        <key>WEBCODEX_CHATGPT_SAFE_REGISTRY</key>
        <string>{registry}</string>
        <key>WEBCODEX_CHATGPT_SAFE_LOG_DIR</key>
        <string>{log_dir}</string>
        <key>WEBCODEX_CHATGPT_STATE_DIR</key>
        <string>{state_dir}</string>
        <key>WEBCODEX_TUNNEL_CLIENT</key>
        <string>{tunnel}</string>
        <key>WEBCODEX_TUNNEL_PROFILE_DIR</key>
        <string>{profile_dir}</string>
        <key>WEBCODEX_TUNNEL_PROFILE</key>
        <string>{profile}</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ThrottleInterval</key>
    <integer>{THROTTLE_INTERVAL}</integer>
    <key>ProcessType</key>
    <string>Background</string>
    <key>StandardOutPath</key>
    <string>/dev/null</string>
    <key>StandardErrorPath</key>
    <string>{stderr_log}</string>
</dict>
</plist>
"#,
        OWNER_MARKER = OWNER_MARKER,
        LABEL = LABEL,
        tunnel = tunnel,
        profile = profile,
        profile_dir = profile_dir,
        health_url = health_url,
        pid_file = pid_file,
        binary = binary,
        registry = registry,
        log_dir = log_dir,
        state_dir = state_dir,
        stderr_log = stderr_log,
        THROTTLE_INTERVAL = THROTTLE_INTERVAL,
    ))
}

pub fn install(config: &ServiceConfig) -> Result<String, String> {
    config.validate()?;
    if unsafe { libc::geteuid() } == 0 {
        return Err("refusing to install a root-owned LaunchAgent; run as the user".into());
    }
    let plist = plist_path()?;
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create LaunchAgents directory: {e}"))?;
    }
    // Refuse to overwrite a plist this tool does not own. An unreadable plist
    // is NOT treated as absent: ownership would be unverified, and overwriting
    // it could destroy a foreign agent definition.
    match std::fs::read_to_string(&plist) {
        Ok(existing) => {
            if !existing.contains(OWNER_MARKER) {
                return Err(format!(
                    "refusing to overwrite unmanaged plist: {}",
                    plist.display()
                ));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "cannot verify ownership of {}: {error}; refusing to overwrite",
                plist.display()
            ))
        }
    }
    let stderr_path = config.log_dir()?.join("chatgpt-safe.err.log");
    std::fs::write(&plist, render_plist(config, &stderr_path)?)
        .map_err(|e| format!("cannot write plist: {e}"))?;
    let mut perms = std::fs::metadata(&plist)
        .map_err(|e| format!("cannot stat plist: {e}"))?
        .permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(&plist, perms).map_err(|e| format!("cannot chmod plist: {e}"))?;
    Ok(json!({
        "status": "INSTALLED",
        "label": LABEL,
        "plist": plist.display().to_string(),
        "runs_as": "user",
        "key_in_plist": false,
        "restart_throttle_seconds": THROTTLE_INTERVAL,
    })
    .to_string())
}

pub fn start() -> Result<String, String> {
    let label = LABEL.to_string();
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let target = format!("{domain}/{LABEL}");
    let plist = plist_path()?;
    if !plist.is_file() {
        return Err("service is not installed; run chatgpt install first".into());
    }
    // Prefer bootstrapping the plist; if it is already loaded, kickstart it.
    let (boot_ok, boot_err) = launchctl(&[
        "bootstrap".to_string(),
        domain.clone(),
        plist.display().to_string(),
    ]);
    if boot_ok {
        return Ok(json!({"status": "STARTED", "label": label, "method": "bootstrap"}).to_string());
    }
    // `-k` is required to kickstart a service that is not currently running.
    let (kick_ok, kick_err) = launchctl(&["kickstart".to_string(), "-k".to_string(), target]);
    if kick_ok {
        return Ok(json!({"status": "STARTED", "label": label, "method": "kickstart"}).to_string());
    }
    Err(format!(
        "launchctl could not start the service (bootstrap: {boot_err}; kickstart: {kick_err})"
    ))
}

fn plist_path() -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is unavailable")?;
    Ok(home.join("Library/LaunchAgents").join(PLIST_FILE))
}

pub fn stop() -> Result<String, String> {
    let label = LABEL.to_string();
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    // `bootout` is the clean stop path. An already-unloaded service is not an
    // error, but a real launchctl failure must NOT be reported as a successful
    // stop: a silently undelivered bootout would leave the agent loaded while
    // the operator is told it stopped.
    let (ok, err) = launchctl(&["bootout".to_string(), format!("{domain}/{label}")]);
    if ok {
        return Ok(json!({"status": "STOPPED", "label": label}).to_string());
    }
    // Distinguish "was not loaded" (already stopped) from a real failure by
    // asking launchd whether the service exists at all.
    let (loaded, _) = launchctl(&["print".to_string(), format!("{domain}/{label}")]);
    if !loaded {
        return Ok(
            json!({"status": "ALREADY_STOPPED", "label": label, "detail": "service was not loaded"})
                .to_string(),
        );
    }
    Err(format!(
        "launchctl could not stop the service (bootout: {err}); it may still be loaded"
    ))
}

pub fn restart() -> Result<String, String> {
    stop()?;
    start()
}

/// Report process / tunnel / ready / MCP reachability as distinct states.
pub fn status(config: &ServiceConfig) -> Result<String, String> {
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let (loaded, _) = launchctl(&["print".to_string(), format!("{domain}/{LABEL}")]);
    let plist_installed = plist_path().map(|p| p.is_file()).unwrap_or(false);
    let pid = std::fs::read_to_string(config.pid_file().ok().unwrap_or_default().as_path())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    // A pid file can be left behind by a crashed process, so its presence is
    // not evidence that anything is running. Verify with signal 0, and report
    // liveness and tunnel state separately from the raw file contents.
    let process_running = pid.map(|pid| process_alive(pid)).unwrap_or(false);
    let health =
        std::fs::read_to_string(config.health_url_file().ok().unwrap_or_default().as_path())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    let health_state = match &health {
        None => "UNKNOWN",
        Some(_) => "URL_AVAILABLE",
    };
    Ok(json!({
        "status": if loaded { "LOADED" } else { "NOT_LOADED" },
        "label": LABEL,
        "plist_installed": plist_installed,
        "pid_file_present": pid.is_some(),
        "process_running": process_running,
        "pid": pid,
        // Only claim a connected tunnel when the process is actually alive; a
        // stale pid file must never read as a live tunnel.
        "tunnel_connected": process_running,
        "health": health_state,
        "health_url_present": health.is_some(),
        "webcodex_mcp_reachable": "VERIFY_WITH_DOCTOR",
    })
    .to_string())
}

/// Liveness probe for a recorded pid.
///
/// `kill(pid, 0)` returns success for a live process and `ESRCH` for a
/// reaped one, without delivering a signal. `EPERM` means the process exists
/// but belongs to another user, which still counts as alive.
fn process_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    // SAFETY: `kill` with signal 0 performs error checking only and sends no
    // signal. ESRCH is reported as -1 with errno, which is read via the
    // return value comparison below.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn doctor(config: &ServiceConfig) -> Result<String, String> {
    let mut checks: Vec<Value> = Vec::new();
    let mut ok = true;

    let uid = unsafe { libc::geteuid() };
    checks.push(json!({"check": "not_root", "pass": uid != 0}));
    ok &= uid != 0;

    let plist = plist_path().unwrap_or_default();
    let plist_ok = plist.is_file();
    checks.push(
        json!({"check": "plist_installed", "pass": plist_ok, "path": plist.display().to_string()}),
    );
    ok &= plist_ok;

    // The credential audit must be unconditional. If the plist exists but
    // cannot be read, skipping the check would let `doctor` report an overall
    // PASS without ever having audited the file for credentials.
    match std::fs::read_to_string(&plist) {
        Ok(text) => {
            // The strongest available proxy for "no credential in the plist": the
            // owned plist contains only these keys, and never a `key`/`secret` field.
            let has_secret_field = text.contains("api-key")
                || text.contains("api_key")
                || text.contains("secret")
                || text.contains("bearer");
            checks.push(json!({"check": "no_credential_in_plist", "pass": !has_secret_field}));
            ok &= !has_secret_field;
            checks
                .push(json!({"check": "plist_owner_marker", "pass": text.contains(OWNER_MARKER)}));
            ok &= text.contains(OWNER_MARKER);
        }
        Err(error) => {
            checks.push(json!({
                "check": "no_credential_in_plist",
                "pass": false,
                "detail": format!("plist is present but unreadable, so it was NOT audited: {error}"),
            }));
            checks.push(json!({
                "check": "plist_owner_marker",
                "pass": false,
                "detail": "plist is present but unreadable, so ownership was NOT verified",
            }));
            ok = false;
        }
    }

    let (loaded, _) = launchctl(&[
        "print".to_string(),
        format!("gui/{}/{}", unsafe { libc::getuid() }, LABEL),
    ]);
    checks.push(json!({"check": "service_loaded", "pass": loaded, "detail": "not running; run webcodex chatgpt start"}));
    // Not-loaded is a legitimate operator state, so it is reported, not failed.

    let stderr_log = config
        .log_dir()
        .unwrap_or_default()
        .join("chatgpt-safe.err.log");
    if let Ok(meta) = std::fs::metadata(&stderr_log) {
        let size = meta.len();
        checks.push(json!({"check":"stderr_log_bounded","pass": size <= MAX_STDERR_BYTES, "bytes": size, "cap": MAX_STDERR_BYTES}));
        ok &= size <= MAX_STDERR_BYTES;
    }

    Ok(json!({
        "status": if ok { "PASS" } else { "FAIL" },
        "label": LABEL,
        "checks": checks,
    })
    .to_string())
}

/// Print the tail of bounded stderr. Never prints MCP stdout or credentials.
pub fn logs(config: &ServiceConfig, lines: usize) -> Result<String, String> {
    let dir = config.log_dir()?;
    let path = dir.join("chatgpt-safe.err.log");
    // An unreadable log must not be reported as an empty log: that is
    // indistinguishable from "the service produced no diagnostics" and would
    // hide a real permission or IO problem from the operator.
    let (text, readable) = match std::fs::read_to_string(&path) {
        Ok(text) => (text, true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (String::new(), true),
        Err(error) => {
            return Err(format!(
                "log is present but unreadable at {}: {error}",
                path.display()
            ))
        }
    };
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines.clamp(1, 2000));
    let body = all[start..].join("\n");
    Ok(json!({
        "log": path.display().to_string(),
        "log_present": path.exists(),
        "log_readable": readable,
        "total_lines": all.len(),
        "shown": all.len() - start,
        "stderr_tail": body,
    })
    .to_string())
}

pub fn uninstall() -> Result<String, String> {
    // Stop first, and do not delete the plist unless the service actually went
    // away. Removing the plist while the agent is still loaded would leave a
    // running, now-unmanaged service behind while reporting UNINSTALLED.
    let stop_result = stop();
    let domain = format!("gui/{}", unsafe { libc::getuid() });
    let (booted_out, boot_err) = launchctl(&["bootout".to_string(), format!("{domain}/{LABEL}")]);
    let (loaded, _) = launchctl(&["print".to_string(), format!("{domain}/{LABEL}")]);
    if loaded {
        return Err(format!(
            "refusing to remove the plist: the service is still loaded \
             (bootout ok: {booted_out}; bootout detail: {boot_err}; stop: {:?})",
            stop_result.err()
        ));
    }
    let mut removed = Vec::new();
    // Only remove a plist that carries our ownership marker.
    if let Some(home) = std::env::var_os("HOME") {
        let plist = PathBuf::from(home)
            .join("Library/LaunchAgents")
            .join(PLIST_FILE);
        match std::fs::read_to_string(&plist) {
            Ok(text) if text.contains(OWNER_MARKER) => {
                std::fs::remove_file(&plist).map_err(|e| format!("cannot remove plist: {e}"))?;
                removed.push(plist.display().to_string());
            }
            Ok(_) => {
                return Err(format!(
                    "refusing to remove unmanaged plist: {}",
                    plist.display()
                ))
            }
            // Only a genuinely absent plist is a no-op. Any other IO failure
            // (permissions, unreadable file) means ownership is UNVERIFIED, and
            // reporting UNINSTALLED while the file is still on disk would be
            // claiming a removal that never happened.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot verify ownership of {}: {error}; refusing to report UNINSTALLED",
                    plist.display()
                ))
            }
        }
    }
    Ok(json!({
        "status": "UNINSTALLED",
        "label": LABEL,
        "removed": removed,
    })
    .to_string())
}

/// Rotate bounded stderr logs so a long-running agent cannot fill the disk.
pub fn rotate_logs(config: &ServiceConfig) -> Result<(), String> {
    let dir = config.log_dir()?;
    let base = dir.join("chatgpt-safe.err.log");
    if !base.exists() {
        return Ok(());
    }
    let meta = std::fs::metadata(&base).map_err(|e| e.to_string())?;
    if meta.len() <= MAX_STDERR_BYTES {
        return Ok(());
    }
    let oldest = dir.join(format!("chatgpt-safe.err.log.{MAX_STDERR_LOGS}"));
    let _ = std::fs::remove_file(&oldest);
    for index in (1..MAX_STDERR_LOGS).rev() {
        let from = dir.join(format!("chatgpt-safe.err.log.{index}"));
        let to = dir.join(format!("chatgpt-safe.err.log.{}", index + 1));
        if from.exists() {
            let _ = std::fs::rename(&from, &to);
        }
    }
    std::fs::rename(&base, dir.join("chatgpt-safe.err.log.1")).map_err(|e| e.to_string())?;
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&base);
    Ok(())
}

/// Best-effort truncate on startup so a stale large log cannot persist.
pub fn prepare_logs(config: &ServiceConfig) -> Result<(), String> {
    rotate_logs(config)
}

/// Operator command surface: `webcodex chatgpt <subcommand>`.
pub fn run(args: &[String]) -> Result<(), String> {
    let sub = args.first().map(String::as_str).unwrap_or("");
    let config = || -> Result<ServiceConfig, String> { service_config_from_args(args) };
    let output = match sub {
        "install" => install(&config()?)?,
        "start" => start()?,
        "stop" => stop()?,
        "restart" => restart()?,
        "status" => status(&config()?)?,
        "doctor" => doctor(&config()?)?,
        "logs" => {
            let lines = args
                .iter()
                .skip_while(|a| a.as_str() != "--lines")
                .nth(1)
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(100);
            logs(&config()?, lines)?
        }
        "uninstall" => uninstall()?,
        _ => {
            return Err(
                "usage: chatgpt <install|start|stop|restart|status|doctor|logs|uninstall> \
                 --tunnel-client PATH --profile-dir PATH [--profile NAME] \
                 --binary PATH --registry PATH --state-dir PATH"
                    .into(),
            )
        }
    };
    println!("{output}");
    Ok(())
}

/// Parse operator paths from flags. These are operator inputs only and never
/// reach the model tool surface.
fn service_config_from_args(args: &[String]) -> Result<ServiceConfig, String> {
    let mut tunnel_client = None;
    let mut profile_dir = None;
    let mut profile_name =
        std::env::var("WEBCODEX_TUNNEL_PROFILE").unwrap_or_else(|_| "webcodex-chatgpt".to_string());
    let mut binary = None;
    let mut registry = None;
    let mut state_dir = None;
    let mut index = 1;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("{flag} requires a value"))?;
        match flag {
            "--tunnel-client" => tunnel_client = Some(PathBuf::from(value)),
            "--profile-dir" => profile_dir = Some(PathBuf::from(value)),
            "--profile" => profile_name = value.clone(),
            "--binary" => binary = Some(PathBuf::from(value)),
            "--registry" => registry = Some(PathBuf::from(value)),
            "--state-dir" => state_dir = Some(PathBuf::from(value)),
            "--lines" => {}
            _ => return Err(format!("unknown chatgpt option: {flag}")),
        }
        index += 2;
    }
    Ok(ServiceConfig {
        tunnel_client: tunnel_client.ok_or("--tunnel-client is required")?,
        profile_dir: profile_dir.ok_or("--profile-dir is required")?,
        profile_name,
        binary: binary.ok_or("--binary is required")?,
        registry: registry.ok_or("--registry is required")?,
        state_dir: state_dir.ok_or("--state-dir is required")?,
    })
}
