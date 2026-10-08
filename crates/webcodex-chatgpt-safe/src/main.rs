use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use webcodex_core::lsp_bridge::{RunnerLspPayload, RunnerLspRequest};
use webcodex_lsp::{execute_lsp_operation, LspSupervisor};
use webcodex_process::execution_broker::{
    BrokerError, EnvPolicy, ExecutionBroker, SpawnSpec, StreamPolicy, TrustedToolchainRoot,
    WorkspaceAuthority,
};
use webcodex_process::ManagedChild;
use webcodex_workspace::git_broker::run_git_bounded_read;

/// The service control plane drives launchd, which is macOS-only, and the module
/// uses unix-only APIs. Gating it here keeps the crate compiling on Windows.
#[cfg(unix)]
mod service;

const MAX_REQUEST: usize = 64 * 1024;
const MAX_HELPER_REQUEST: usize = 32 * 1024;
const MAX_OUTPUT: usize = 12 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(15);
const SHELL_TIMEOUT_MAX: u64 = 30;
const DRAIN_TAIL: Duration = Duration::from_millis(250);
const PYTHON: &str = "/opt/homebrew/bin/python3";
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

// Safe managed asynchronous job surface (Slice A).
const JOB_TIMEOUT_MAX: u64 = 1800;
const MAX_CONCURRENT_JOBS: usize = 8;
/// Terminal job records kept pollable before the oldest are evicted. Separate
/// from MAX_CONCURRENT_JOBS, which bounds only jobs that are still running.
const MAX_RETAINED_TERMINAL_JOBS: usize = 64;
const JOB_STREAM_CAP: usize = 256 * 1024;
const JOB_POLL_CAP: usize = 16 * 1024;
const JOB_READ_CHUNK: usize = 4096;
const JOB_SLEEP: Duration = Duration::from_millis(20);

// LSP request default/max bounds.
const LSP_LIMIT_MAX: usize = 200;
const LSP_OPERATION_TIMEOUT: Duration = Duration::from_secs(20);

const SAFE_TOOLS: [&str; 16] = [
    "project_list",
    "project_select",
    "project_current",
    "files_search",
    "files_read",
    "files_apply_patch",
    "shell_run",
    "job_start",
    "job_poll",
    "job_cancel",
    "git_status",
    "git_diff",
    "lsp_symbols",
    "lsp_definition",
    "lsp_references",
    "lsp_diagnostics",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    id: String,
    name: String,
    root: PathBuf,
}

struct App {
    project: Registry,
    authority: WorkspaceAuthority,
    root: PathBuf,
    python: PathBuf,
    jobs: Mutex<JobRegistry>,
    lsp: Mutex<LspSupervisor>,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("webcodex-chatgpt-safe: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // Operator-only service control. Never reachable from the MCP tool surface.
    if raw.first().map(String::as_str) == Some("chatgpt") {
        #[cfg(unix)]
        return service::run(&raw[1..]);
        #[cfg(not(unix))]
        return Err("service control is only available on unix".into());
    }
    let mut args = raw.into_iter();
    let command = args.next().unwrap_or_default();
    let mut profile = None;
    let mut registry = None;
    let mut python = PathBuf::from(PYTHON);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--profile" => profile = args.next(),
            "--registry" => registry = args.next().map(PathBuf::from),
            "--python" => {
                python = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or("--python requires a path")?
            }
            _ => return Err("unknown command-line argument".into()),
        }
    }
    let registry = registry.ok_or("--registry is required")?;
    let app = App::load(&registry, python)?;
    match command.as_str() {
        "doctor" => doctor(&app)?,
        "status" if profile.is_none() => println!(
            "{{\"status\":\"configured\",\"profile\":\"chatgpt-safe\",\"project_id\":{}}}",
            json!(app.project.id)
        ),
        "status" => return Err("status does not accept a profile override".into()),
        "serve" if profile.as_deref() == Some("chatgpt-safe") => {
            if let Ok(config) = service_config_from_env() {
                let _ = service::prepare_logs(&config);
            }
            serve(&app)?
        }
        "serve" => return Err("serve requires --profile chatgpt-safe".into()),
        _ => return Err("command must be serve, status, doctor, or chatgpt".into()),
    }
    Ok(())
}

// LaunchAgent settings are passed as paths only, so no credential ever appears
// in the process environment of the supervised agent.
fn service_config_from_env() -> Result<service::ServiceConfig, String> {
    let get = |key: &str| {
        std::env::var(key)
            .ok()
            .map(PathBuf::from)
            .ok_or_else(|| format!("{key} is not set"))
    };
    Ok(service::ServiceConfig {
        tunnel_client: get("WEBCODEX_TUNNEL_CLIENT")?,
        profile_dir: get("WEBCODEX_TUNNEL_PROFILE_DIR")?,
        profile_name: std::env::var("WEBCODEX_TUNNEL_PROFILE")
            .unwrap_or_else(|_| "webcodex-chatgpt".to_string()),
        binary: get("WEBCODEX_CHATGPT_SAFE_BINARY")?,
        registry: get("WEBCODEX_CHATGPT_SAFE_REGISTRY")?,
        state_dir: get("WEBCODEX_CHATGPT_STATE_DIR")?,
    })
}

impl App {
    fn load(registry_path: &Path, python: PathBuf) -> Result<Self, String> {
        let metadata = std::fs::metadata(registry_path).map_err(|_| "cannot read registry")?;
        if !metadata.is_file() {
            return Err("registry must be a regular file".into());
        }
        let bytes = std::fs::read(registry_path).map_err(|_| "cannot read registry")?;
        if bytes.len() > 16 * 1024 {
            return Err("registry is too large".into());
        }
        let project: Registry = serde_json::from_slice(&bytes)
            .map_err(|_| "registry must contain exactly id, name, and root")?;
        if project.id.is_empty()
            || project.id.len() > 128
            || project.name.is_empty()
            || project.name.len() > 256
        {
            return Err("invalid project identity".into());
        }
        if !project.root.is_absolute() {
            return Err("project root must be absolute".into());
        }
        let root = project
            .root
            .canonicalize()
            .map_err(|_| "project root is unavailable")?;
        if !root.is_dir() || too_broad_root(&root) {
            return Err("project root is not an eligible narrow directory".into());
        }
        let registry_path = registry_path
            .canonicalize()
            .map_err(|_| "registry path unavailable")?;
        if registry_path.starts_with(&root) {
            return Err("registry must be outside the project".into());
        }
        if python != Path::new(PYTHON) {
            return Err("python path must be the operator-fixed /opt/homebrew/bin/python3".into());
        }
        if !python.is_file() {
            return Err("operator-fixed Python is unavailable".into());
        }
        let authority = WorkspaceAuthority::for_trusted_root(&root)
            .map_err(|_| "project root authority unavailable")?;
        Ok(Self {
            project,
            authority,
            root,
            python,
            jobs: Mutex::new(JobRegistry::default()),
            lsp: Mutex::new(LspSupervisor::default()),
        })
    }

    fn check_root(&self) -> Result<(), String> {
        let now = self
            .project
            .root
            .canonicalize()
            .map_err(|_| "project root changed or disappeared")?;
        if now != self.root {
            return Err("project root changed; restart required".into());
        }
        Ok(())
    }
}

fn too_broad_root(root: &Path) -> bool {
    let count = root
        .components()
        .filter(|c| matches!(c, Component::Normal(_)))
        .count();
    let home = std::env::var_os("HOME").and_then(|h| PathBuf::from(h).canonicalize().ok());
    let broad_name = root.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        matches!(
            n,
            "Users"
                | "System"
                | "Applications"
                | "Library"
                | "Desktop"
                | "Documents"
                | "Downloads"
                | "Projects"
                | "Codex"
                | "tmp"
                | "var"
                | "etc"
        )
    });
    // Require project-depth paths and reject common host collection roots.
    count < 4 || root == Path::new("/") || broad_name || home.as_ref().is_some_and(|h| root == *h)
}

fn broker_failure(error: &BrokerError) -> &'static str {
    match error {
        BrokerError::UnsupportedPlatform => "HOST_UNAVAILABLE: sandbox backend is unsupported",
        BrokerError::PlanRefused(_) | BrokerError::CwdOutsideSandboxRoots { .. } => {
            "POLICY_DENIED: broker rejected the project scope"
        }
        BrokerError::PlanNotCompilable(_) => {
            "HOST_UNAVAILABLE: sandbox profile could not be compiled"
        }
        BrokerError::Launch(e) if e.to_string().contains("sandbox_apply") => {
            "ENV_BLOCKED: sandbox policy could not be applied"
        }
        BrokerError::Launch(_) | BrokerError::SpecInvalid(_) => {
            "HOST_UNAVAILABLE: broker could not start the fixed executable"
        }
    }
}

fn doctor(app: &App) -> Result<(), String> {
    app.check_root()?;
    let probe = spawn_python(
        app,
        &["-I", "-S", "-c", "pass"],
        app.root.clone(),
        Duration::from_secs(3),
    )
    .map_err(|(_, message)| message.to_string())?;
    if probe["success"] != true {
        let stderr = probe["stderr"].as_str().unwrap_or("");
        let class =
            if stderr.contains("sandbox_apply") || stderr.contains("Operation not permitted") {
                "ENV_BLOCKED"
            } else {
                probe["outcome"].as_str().unwrap_or("HOST_UNAVAILABLE")
            };
        return Err(format!("{class}: fixed Python sandbox probe failed"));
    }
    println!(
        "{{\"status\":\"ready\",\"profile\":\"chatgpt-safe\",\"project_id\":{}}}",
        json!(app.project.id)
    );
    Ok(())
}

fn serve(app: &App) -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let mut input = stdin.lock();
    loop {
        let mut line = Vec::with_capacity(4096);
        let mut complete = false;
        loop {
            let available = input.fill_buf().map_err(|_| "stdio read failed")?;
            if available.is_empty() {
                complete = true;
                break;
            }
            let count = available
                .iter()
                .position(|b| *b == b'\n')
                .map_or(available.len(), |i| i + 1);
            if line.len() + count > MAX_REQUEST {
                return Err("MCP message exceeds 64 KiB limit".into());
            }
            let has_newline = available[count - 1] == b'\n';
            line.extend_from_slice(&available[..count]);
            input.consume(count);
            if has_newline {
                break;
            }
        }
        if line.is_empty() && complete {
            break;
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        let request: Value =
            serde_json::from_slice(&line).map_err(|_| "invalid JSON-RPC message")?;
        let response = handle(app, &request);
        if let Some(response) = response {
            serde_json::to_writer(&mut stdout, &response).map_err(|_| "stdio write failed")?;
            stdout.write_all(b"\n").map_err(|_| "stdio write failed")?;
            stdout.flush().map_err(|_| "stdio flush failed")?;
        }
        if complete {
            break;
        }
    }
    Ok(())
}

fn handle(app: &App, req: &Value) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    if id.is_none() {
        return None;
    }
    if id
        .as_ref()
        .is_some_and(|v| !(v.is_null() || v.is_string() || v.is_number()))
    {
        return Some(
            json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"invalid JSON-RPC id"}}),
        );
    }
    if req.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || !req.get("method").is_some_and(Value::is_string)
    {
        return Some(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"invalid JSON-RPC request"}}),
        );
    }
    let result = match method {
        "initialize" => Ok(
            json!({"protocolVersion":MCP_PROTOCOL_VERSION,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"webcodex-chatgpt-safe","version":"0.1.0"}}),
        ),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list()),
        "tools/call" => tool_call(app, req.get("params").unwrap_or(&Value::Null)),
        _ => Err((-32601, "method not found")),
    };
    Some(match result {
        Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
        Err((code, message)) => {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
        }
    })
}

fn tools_list() -> Value {
    let defs = [
        ("project_list", "List the single operator-registered project.", json!({"type":"object","properties":{},"additionalProperties":false})),
        ("project_select", "Confirm the registered project identity. This does not change authority.", json!({"type":"object","properties":{"project_id":{"type":"string"}},"required":["project_id"],"additionalProperties":false})),
        ("project_current", "Report the single operator-registered project identity that all tools act within. This does not change authority.", json!({"type":"object","properties":{"project_id":{"type":"string"}},"required":["project_id"],"additionalProperties":false})),
        ("files_search", "Search literal text within the registered project. Results are bounded; if any path is unreadable, results are partial and truncated is true.", json!({"type":"object","properties":{"project_id":{"type":"string"},"query":{"type":"string","minLength":1,"maxLength":512}},"required":["project_id","query"],"additionalProperties":false})),
        ("files_read", "Read a bounded range from a project-relative file.", json!({"type":"object","properties":{"project_id":{"type":"string"},"path":{"type":"string","minLength":1,"maxLength":512},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":1400}},"required":["project_id","path","offset","limit"],"additionalProperties":false})),
        ("files_apply_patch", "Replace exactly one unique text occurrence only after verifying the current file SHA-256 revision; does not create files. After a timeout or incomplete result, read the current revision before retrying to avoid repeating an effect.", json!({"type":"object","properties":{"project_id":{"type":"string"},"path":{"type":"string","minLength":1,"maxLength":512},"revision":{"type":"string","pattern":"^[a-f0-9]{64}$"},"old_text":{"type":"string","maxLength":8192},"new_text":{"type":"string","maxLength":8192}},"required":["project_id","path","revision","old_text","new_text"],"additionalProperties":false})),
        ("shell_run", "Run a foreground command in the project through the deny-network process broker; output is bounded. Success covers the direct child, owned process group, and captured streams. Timeout is not a whole-family deadline; daemonized/setsid processes and durable execution are unsupported. Timeout or incomplete results may already have effects; inspect state before retrying.", json!({"type":"object","properties":{"project_id":{"type":"string"},"cwd":{"type":"string"},"command":{"type":"string","maxLength":16384},"timeout_seconds":{"type":"integer","minimum":1,"maximum":30}},"required":["project_id","cwd","command","timeout_seconds"],"additionalProperties":false})),
        ("job_start", "Start a safe managed asynchronous command through the deny-network process broker; output is captured incrementally and bounded. Jobs are NOT durable and do NOT survive a WebCodex restart. Returns a bounded job identity and metadata.", json!({"type":"object","properties":{"project_id":{"type":"string"},"cwd":{"type":"string"},"command":{"type":"string","maxLength":16384},"timeout_seconds":{"type":"integer","minimum":1,"maximum":1800}},"required":["project_id","cwd","command","timeout_seconds"],"additionalProperties":false})),
        ("job_poll", "Poll a managed job for incremental stdout/stderr since the last cursor and its current status. Supports polling after terminal state. Unknown or stale job ids are reported; jobs do not survive restart.", json!({"type":"object","properties":{"project_id":{"type":"string"},"job_id":{"type":"string","minLength":1,"maxLength":128},"stdout_cursor":{"type":"integer","minimum":0},"stderr_cursor":{"type":"integer","minimum":0}},"required":["project_id","job_id"],"additionalProperties":false})),
        ("job_cancel", "Request cancellation of an entire managed job process tree. The managed tree is terminated; the job is not merely forgotten by PID.", json!({"type":"object","properties":{"project_id":{"type":"string"},"job_id":{"type":"string","minLength":1,"maxLength":128}},"required":["project_id","job_id"],"additionalProperties":false})),
        ("git_status", "Read fixed bounded Git status and branch/head metadata.", json!({"type":"object","properties":{"project_id":{"type":"string"}},"required":["project_id"],"additionalProperties":false})),
        ("git_diff", "Read fixed bounded Git diff without external diff or text conversion.", json!({"type":"object","properties":{"project_id":{"type":"string"}},"required":["project_id"],"additionalProperties":false})),
        ("lsp_symbols", "Read-only document symbols for a project file through the brokered language server. The language server executable is operator-configured; the model cannot choose it.", json!({"type":"object","properties":{"project_id":{"type":"string"},"path":{"type":"string","minLength":1,"maxLength":512},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["project_id","path"],"additionalProperties":false})),
        ("lsp_definition", "Read-only goto-definition for a position through the brokered language server.", json!({"type":"object","properties":{"project_id":{"type":"string"},"path":{"type":"string","minLength":1,"maxLength":512},"line":{"type":"integer","minimum":1},"column":{"type":"integer","minimum":1},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["project_id","path","line","column"],"additionalProperties":false})),
        ("lsp_references", "Read-only find-references for a position through the brokered language server.", json!({"type":"object","properties":{"project_id":{"type":"string"},"path":{"type":"string","minLength":1,"maxLength":512},"line":{"type":"integer","minimum":1},"column":{"type":"integer","minimum":1},"include_declaration":{"type":"boolean"},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["project_id","path","line","column"],"additionalProperties":false})),
        ("lsp_diagnostics", "Read-only document diagnostics for a project file through the brokered language server.", json!({"type":"object","properties":{"project_id":{"type":"string"},"path":{"type":"string","minLength":1,"maxLength":512},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["project_id","path"],"additionalProperties":false})),
    ];
    json!({"tools":defs.into_iter().map(|(name,description,input_schema)|json!({"name":name,"description":description,"inputSchema":input_schema,"annotations":{"readOnlyHint":!matches!(name,"files_apply_patch"|"shell_run"|"job_start"|"job_cancel"),"destructiveHint":matches!(name,"files_apply_patch"|"shell_run"|"job_start"|"job_cancel"),"idempotentHint":!matches!(name,"files_apply_patch"|"shell_run"|"job_start"|"job_cancel"),"openWorldHint":false}})).collect::<Vec<_>>()})
}

fn tool_call(app: &App, params: &Value) -> Result<Value, (i64, &'static str)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| SAFE_TOOLS.contains(n))
        .ok_or((-32602, "unknown tool"))?;
    if let Some(meta) = params.get("_meta") {
        if !meta.is_object() {
            return Err((-32602, "_meta must be an object"));
        }
        // MCP metadata is protocol-only; it never enters tool arguments or authority.
        exact_keys(params, &["name", "arguments", "_meta"])?;
    } else {
        exact_keys(params, &["name", "arguments"])?;
    }
    let args = params
        .get("arguments")
        .cloned()
        .ok_or((-32602, "arguments are required"))?;
    let out = match name {
        "project_list" => {
            exact_keys(&args, &[])?;
            json!({"projects":[{"project_id":app.project.id,"name":app.project.name}]})
        }
        "project_select" => {
            exact_keys(&args, &["project_id"])?;
            check_project(app, &args)?;
            json!({"selected":app.project.id,"authority_changed":false})
        }
        "files_search" => {
            exact_keys(&args, &["project_id", "query"])?;
            check_project(app, &args)?;
            let q = args["query"]
                .as_str()
                .filter(|q| !q.is_empty() && q.len() <= 512)
                .ok_or((-32602, "query must be 1-512 UTF-8 bytes"))?;
            let _ = q;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            helper(app, "search", &args)?
        }
        "files_read" => {
            exact_keys(&args, &["project_id", "path", "offset", "limit"])?;
            check_project(app, &args)?;
            validate_file_args(&args)?;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            helper(app, "read", &args)?
        }
        "files_apply_patch" => {
            exact_keys(
                &args,
                &["project_id", "path", "revision", "old_text", "new_text"],
            )?;
            check_project(app, &args)?;
            validate_file_args(&args)?;
            let rev = args["revision"]
                .as_str()
                .filter(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                })
                .ok_or((-32602, "revision must be lowercase SHA-256 hex"))?;
            let _ = rev;
            for k in ["old_text", "new_text"] {
                if args[k].as_str().is_none_or(|s| s.len() > 8192) {
                    return Err((-32602, "patch text exceeds 8 KiB or is not text"));
                }
            }
            if args["old_text"].as_str() == Some("") {
                return Err((-32602, "old_text must not be empty"));
            }
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            helper(app, "patch", &args)?
        }
        "shell_run" => {
            exact_keys(&args, &["project_id", "cwd", "command", "timeout_seconds"])?;
            check_project(app, &args)?;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            shell(app, &args)?
        }
        "git_status" => {
            exact_keys(&args, &["project_id"])?;
            check_project(app, &args)?;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            git_status(app)?
        }
        "git_diff" => {
            exact_keys(&args, &["project_id"])?;
            check_project(app, &args)?;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            git_read(
                app,
                &[
                    "-c",
                    "core.fsmonitor=false",
                    "-c",
                    "core.hooksPath=/dev/null",
                    "diff",
                    "--no-ext-diff",
                    "--no-textconv",
                ],
                Instant::now() + GIT_TIMEOUT,
            )?
        }
        "project_current" => {
            exact_keys(&args, &["project_id"])?;
            check_project(app, &args)?;
            json!({
                "project_id": app.project.id,
                "name": app.project.name,
                "status": "SELECTED",
                "tool_count": SAFE_TOOLS.len(),
                "authority_changed": false,
            })
        }
        "job_start" => {
            exact_keys(&args, &["project_id", "cwd", "command", "timeout_seconds"])?;
            check_project(app, &args)?;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            let cwd_rel = args["cwd"]
                .as_str()
                .ok_or((-32602, "cwd must be project-relative"))?;
            let command = args["command"]
                .as_str()
                .ok_or((-32602, "command must be a string"))?;
            let timeout = args["timeout_seconds"]
                .as_u64()
                .ok_or((-32602, "timeout_seconds must be an integer"))?;
            app.jobs
                .lock()
                .unwrap()
                .spawn_job(app, &app.project.id, cwd_rel, command, timeout)?
        }
        "job_poll" => {
            exact_keys_optional(
                &args,
                &["project_id", "job_id"],
                &["stdout_cursor", "stderr_cursor"],
            )?;
            check_project(app, &args)?;
            let job_id = args["job_id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or((-32602, "job_id must be 1-128 UTF-8 bytes"))?;
            let stdout_cursor = args["stdout_cursor"].as_u64().unwrap_or(0);
            let stderr_cursor = args["stderr_cursor"].as_u64().unwrap_or(0);
            app.jobs
                .lock()
                .unwrap()
                .poll(&app.project.id, job_id, stdout_cursor, stderr_cursor)?
        }
        "job_cancel" => {
            exact_keys(&args, &["project_id", "job_id"])?;
            check_project(app, &args)?;
            let job_id = args["job_id"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 128)
                .ok_or((-32602, "job_id must be 1-128 UTF-8 bytes"))?;
            app.jobs.lock().unwrap().cancel(&app.project.id, job_id)?
        }
        "lsp_symbols" => {
            exact_keys_optional(&args, &["project_id", "path"], &["limit"])?;
            check_project(app, &args)?;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            let path = args["path"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 512)
                .ok_or((-32602, "path must be 1-512 UTF-8 bytes"))?;
            let limit = lsp_limit(&args)?;
            lsp_run(
                app,
                RunnerLspRequest::DocumentSymbols {
                    path: path.to_string(),
                    limit,
                },
                path,
            )?
        }
        "lsp_definition" => {
            exact_keys_optional(&args, &["project_id", "path", "line", "column"], &["limit"])?;
            check_project(app, &args)?;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            let path = args["path"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 512)
                .ok_or((-32602, "path must be 1-512 UTF-8 bytes"))?;
            let (line, column) = lsp_position(&args)?;
            let limit = lsp_limit(&args)?;
            lsp_run(
                app,
                RunnerLspRequest::GotoDefinition {
                    path: path.to_string(),
                    line,
                    column,
                    limit,
                },
                path,
            )?
        }
        "lsp_references" => {
            exact_keys_optional(
                &args,
                &["project_id", "path", "line", "column"],
                &["include_declaration", "limit"],
            )?;
            check_project(app, &args)?;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            let path = args["path"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 512)
                .ok_or((-32602, "path must be 1-512 UTF-8 bytes"))?;
            let (line, column) = lsp_position(&args)?;
            let include_declaration = args["include_declaration"].as_bool().unwrap_or(true);
            let limit = lsp_limit(&args)?;
            lsp_run(
                app,
                RunnerLspRequest::FindReferences {
                    path: path.to_string(),
                    line,
                    column,
                    include_declaration,
                    limit,
                },
                path,
            )?
        }
        "lsp_diagnostics" => {
            exact_keys_optional(&args, &["project_id", "path"], &["limit"])?;
            check_project(app, &args)?;
            app.check_root()
                .map_err(|_| (-32000, "project authority unavailable"))?;
            let path = args["path"]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= 512)
                .ok_or((-32602, "path must be 1-512 UTF-8 bytes"))?;
            let limit = lsp_limit(&args)?;
            lsp_run(
                app,
                RunnerLspRequest::DocumentDiagnostics {
                    path: path.to_string(),
                    limit,
                },
                path,
            )?
        }
        _ => return Err((-32602, "unknown tool")),
    };
    let is_error = out.get("success").and_then(Value::as_bool) == Some(false);
    Ok(
        json!({"content":[{"type":"text","text":out.to_string()}],"structuredContent":out,"isError":is_error}),
    )
}

fn exact_keys(value: &Value, keys: &[&str]) -> Result<(), (i64, &'static str)> {
    let object = value
        .as_object()
        .ok_or((-32602, "arguments must be an object"))?;
    if object.len() != keys.len() || object.keys().any(|k| !keys.contains(&k.as_str())) {
        return Err((-32602, "arguments contain missing or unknown keys"));
    }
    Ok(())
}

/// Validate a business argument object against a required set plus a set of
/// declared-optional keys.
///
/// A tool schema advertises optional arguments, so the handler must accept a
/// call that omits them — otherwise every schema-conformant call fails
/// validation and the advertised contract is uncallable. Unknown keys are still
/// rejected outright: optionality is declared per key, never open-ended, so this
/// cannot become a hole for smuggling authority-bearing fields.
fn exact_keys_optional(
    value: &Value,
    required: &[&str],
    optional: &[&str],
) -> Result<(), (i64, &'static str)> {
    let object = value
        .as_object()
        .ok_or((-32602, "arguments must be an object"))?;
    if object
        .keys()
        .any(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err((-32602, "arguments contain unknown keys"));
    }
    for key in required {
        if !object.contains_key(*key) {
            return Err((-32602, "arguments contain missing or unknown keys"));
        }
    }
    Ok(())
}
fn check_project(app: &App, args: &Value) -> Result<(), (i64, &'static str)> {
    if args.get("project_id").and_then(Value::as_str) == Some(&app.project.id) {
        Ok(())
    } else {
        Err((-32000, "unknown project identity"))
    }
}

fn lsp_limit(args: &Value) -> Result<usize, (i64, &'static str)> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(50)
        .clamp(1, LSP_LIMIT_MAX as u64) as usize;
    Ok(limit)
}

fn lsp_position(args: &Value) -> Result<(usize, usize), (i64, &'static str)> {
    let line = args
        .get("line")
        .and_then(Value::as_u64)
        .filter(|v| *v >= 1)
        .ok_or((-32602, "line must be a 1-based integer"))? as usize;
    let column = args
        .get("column")
        .and_then(Value::as_u64)
        .filter(|v| *v >= 1)
        .ok_or((-32602, "column must be a 1-based integer"))? as usize;
    if line > 1_000_000 || column > 1_000_000 {
        return Err((-32602, "position exceeds bounded range"));
    }
    Ok((line, column))
}

fn validate_file_args(args: &Value) -> Result<(), (i64, &'static str)> {
    let path = args["path"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .ok_or((-32602, "path must be 1-512 UTF-8 bytes"))?;
    if path.contains('\\') {
        return Err((-32602, "path must use slash-separated relative components"));
    }
    if let Some(offset) = args.get("offset") {
        if offset.as_u64().is_none() {
            return Err((-32602, "offset must be a non-negative integer"));
        }
        let limit = args["limit"]
            .as_u64()
            .ok_or((-32602, "limit must be an integer"))?;
        if !(1..=1400).contains(&limit) {
            return Err((-32602, "limit must be 1-1400 bytes"));
        }
    }
    Ok(())
}

fn helper(app: &App, operation: &str, args: &Value) -> Result<Value, (i64, &'static str)> {
    let mut request = args.clone();
    request
        .as_object_mut()
        .ok_or((-32602, "arguments must be an object"))?
        .remove("project_id");
    let payload = serde_json::to_string(&request).map_err(|_| (-32602, "invalid arguments"))?;
    if payload.len() > MAX_HELPER_REQUEST {
        return Err((-32602, "helper request exceeds 32 KiB"));
    }
    let action = json!({"op":operation,"args":request});
    let payload = action.to_string();
    let child = spawn_python(
        app,
        &["-I", "-S", "-c", HELPER, &payload],
        app.root.clone(),
        Duration::from_secs(15),
    )?;
    let value: Value = serde_json::from_str(&child["stdout"].as_str().unwrap_or("")).unwrap_or_else(|_|json!({"success":false,"outcome":"incomplete","error":"helper returned incomplete or invalid JSON","stdout_truncated":child["stdout_truncated"]}));
    if child["success"] != true {
        let captured = child["outcome"].as_str().unwrap_or("failed");
        let capture_priority = matches!(
            captured,
            "ENV_BLOCKED"
                | "timed_out"
                | "incomplete"
                | "output_capped"
                | "outcome_unknown"
                | "HOST_UNAVAILABLE"
        );
        // A helper-level failure is a business result, not a transport failure:
        // keep its own bounded recovery fields (current_revision, conflict counts,
        // retry guidance) so the model can self-recover a normal patch conflict.
        if !capture_priority {
            let mut merged = value.as_object().cloned().unwrap_or_default();
            merged.insert("success".into(), json!(false));
            merged.insert("outcome".into(), json!("failed"));
            merged.insert("return_code".into(), child["return_code"].clone());
            merged
                .entry("stdout_truncated")
                .or_insert_with(|| child["stdout_truncated"].clone());
            merged
                .entry("stderr_truncated")
                .or_insert_with(|| child["stderr_truncated"].clone());
            return Ok(Value::Object(merged));
        }
        let outcome = if capture_priority {
            json!(captured)
        } else {
            value.get("code").cloned().unwrap_or(json!(captured))
        };
        return Ok(
            json!({"success":false,"outcome":outcome,"return_code":child["return_code"],"error":value.get("error").cloned().unwrap_or(json!("helper did not complete")),"stdout_truncated":child["stdout_truncated"],"stderr_truncated":child["stderr_truncated"],"timed_out":child["timed_out"],"incomplete":child["incomplete"]}),
        );
    }
    Ok(value)
}

fn git_read(app: &App, argv: &[&str], deadline: Instant) -> Result<Value, (i64, &'static str)> {
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .min(GIT_TIMEOUT);
    if remaining.is_zero() {
        return Err((-32000, "TIMED_OUT: Git operation deadline elapsed"));
    }
    match run_git_bounded_read(&app.root, argv, MAX_OUTPUT, MAX_OUTPUT, remaining) {
        Err(error) => Err((
            -32000,
            match error.code {
                "git_executable_unavailable" => "HOST_UNAVAILABLE: Git executable is unavailable",
                "git_spawn_refused" if error.detail.contains("sandbox_apply") => {
                    "ENV_BLOCKED: Git sandbox could not be applied"
                }
                "git_workspace_root_invalid" => "POLICY_DENIED: project Git root is invalid",
                "git_wait_failed" => "INCOMPLETE: Git child or output drain failed",
                "git_timeout_wait_failed" => "OUTCOME_UNKNOWN: Git timeout cleanup failed",
                _ => "FAIL: Git broker could not complete the fixed read",
            },
        )),
        Ok(result) => {
            let complete = result.status.success()
                && !result.timed_out
                && !result.drain_incomplete
                && !result.stdout_capped
                && !result.stderr_capped;
            let stderr = String::from_utf8_lossy(&result.stderr);
            let outcome = if !complete && stderr.contains("sandbox_apply") {
                "ENV_BLOCKED"
            } else if result.timed_out {
                "timed_out"
            } else if result.drain_incomplete {
                "incomplete"
            } else if result.stdout_capped || result.stderr_capped {
                "output_capped"
            } else if !result.status.success() {
                "failed"
            } else {
                "success"
            };
            Ok(
                json!({"success":complete,"outcome":outcome,"return_code":result.status.code(),"stdout":String::from_utf8_lossy(&result.stdout),"stderr":String::from_utf8_lossy(&result.stderr),"stdout_truncated":result.stdout_capped,"stderr_truncated":result.stderr_capped,"timed_out":result.timed_out,"incomplete":result.drain_incomplete}),
            )
        }
    }
}

fn git_status(app: &App) -> Result<Value, (i64, &'static str)> {
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = git_read(
        app,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "status",
            "--short",
        ],
        deadline,
    )?;
    if status["success"] != true {
        return Ok(status);
    }
    let branch = git_read(
        app,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "rev-parse",
            "--abbrev-ref",
            "HEAD",
        ],
        deadline,
    )?;
    if branch["success"] != true {
        return Ok(branch);
    }
    let head = git_read(
        app,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "rev-parse",
            "--short",
            "HEAD",
        ],
        deadline,
    )?;
    if head["success"] != true {
        return Ok(head);
    }
    Ok(
        json!({"status":status["stdout"],"branch":branch["stdout"],"head":head["stdout"],"success":true}),
    )
}

// Shared preparation path for all brokered shell-like execution. `job_start`
// reuses this exact path; there is no alternate naked `Command`/`spawn` route.
fn prepare_shell(
    app: &App,
    cwd_rel: &str,
    command: &str,
) -> Result<(SpawnSpec, Vec<TrustedToolchainRoot>), (i64, &'static str)> {
    if cwd_rel.len() > 4096 {
        return Err((-32602, "cwd path is too long"));
    }
    let cwd = if cwd_rel == "." {
        app.root.clone()
    } else {
        checked_path(&app.root, cwd_rel, false)
            .map_err(|_| (-32602, "cwd must resolve inside the project"))?
    };
    if !cwd.is_dir() {
        return Err((-32602, "cwd must be a directory"));
    }
    if command.is_empty() || command.len() > 16 * 1024 {
        return Err((-32602, "command length is invalid"));
    }
    let toolchain = app.authority.toolchain_roots_for(&app.python);
    let spec = SpawnSpec::new("/bin/sh", &cwd, app.authority.plan())
        .arg("-c")
        .arg(command)
        .env(EnvPolicy::Minimal)
        .env_var("PATH", "/opt/homebrew/bin:/usr/bin:/bin")
        .env_var("HOME", &app.root)
        .env_var("TMPDIR", &app.root)
        .stdin(StreamPolicy::Null)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    Ok((spec, toolchain))
}

fn shell(app: &App, args: &Value) -> Result<Value, (i64, &'static str)> {
    let cwd_rel = args["cwd"]
        .as_str()
        .ok_or((-32602, "cwd must be project-relative"))?;
    let command = args["command"]
        .as_str()
        .ok_or((-32602, "command must be a string"))?;
    let timeout = args["timeout_seconds"]
        .as_u64()
        .ok_or((-32602, "timeout_seconds must be an integer"))?
        .clamp(1, SHELL_TIMEOUT_MAX);
    let (spec, toolchain) = prepare_shell(app, cwd_rel, command)?;
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut child = spawn_broker(&spec, &toolchain).map_err(|e| (-32000, broker_failure(&e)))?;
    capture_child(&mut child, deadline)
}

fn capture_child(
    child: &mut webcodex_process::ManagedChild,
    deadline: Instant,
) -> Result<Value, (i64, &'static str)> {
    #[cfg(not(unix))]
    {
        let _ = child.terminate_tree();
        return Ok(
            json!({"success":false,"outcome":"HOST_UNAVAILABLE","return_code":null,"stdout":"","stderr":"","stdout_truncated":false,"stderr_truncated":false,"timed_out":false,"incomplete":true}),
        );
    }
    #[cfg(unix)]
    capture_child_unix(child, deadline)
}

#[cfg(unix)]
fn capture_child_unix(
    child: &mut webcodex_process::ManagedChild,
    deadline: Instant,
) -> Result<Value, (i64, &'static str)> {
    let out = child
        .child_mut()
        .stdout
        .take()
        .ok_or((-32000, "stdout pipe unavailable"))?;
    let err = child
        .child_mut()
        .stderr
        .take()
        .ok_or((-32000, "stderr pipe unavailable"))?;
    set_nonblocking(out.as_raw_fd())
        .map_err(|_| (-32000, "HOST_UNAVAILABLE: stdout nonblocking setup failed"))?;
    set_nonblocking(err.as_raw_fd())
        .map_err(|_| (-32000, "HOST_UNAVAILABLE: stderr nonblocking setup failed"))?;
    let mut out = out;
    let mut err = err;
    let mut stdout = Vec::with_capacity(MAX_OUTPUT);
    let mut stderr = Vec::with_capacity(MAX_OUTPUT);
    let (mut out_eof, mut err_eof, mut out_cap, mut err_cap, mut read_failed) =
        (false, false, false, false, false);
    let mut status: Option<ExitStatus> = None;
    let mut timed_out = false;
    let mut tree_exited = false;
    loop {
        poll_reader(
            &mut out,
            &mut stdout,
            &mut out_eof,
            &mut out_cap,
            &mut read_failed,
        )?;
        poll_reader(
            &mut err,
            &mut stderr,
            &mut err_eof,
            &mut err_cap,
            &mut read_failed,
        )?;
        if status.is_none() {
            status = child
                .try_wait()
                .map_err(|_| (-32000, "process status unavailable"))?;
        }
        if !timed_out && Instant::now() >= deadline {
            timed_out = true;
            if status.is_none() {
                let _ = child.terminate_tree();
            }
        }
        if timed_out && status.is_none() {
            status = child.try_wait().ok().flatten();
            poll_reader(
                &mut out,
                &mut stdout,
                &mut out_eof,
                &mut out_cap,
                &mut read_failed,
            )?;
            poll_reader(
                &mut err,
                &mut stderr,
                &mut err_eof,
                &mut err_cap,
                &mut read_failed,
            )?;
            tree_exited = child.wait_tree_exit(Duration::ZERO).unwrap_or(false);
        }
        if status.is_some() {
            let cleanup_deadline = std::cmp::min(deadline, Instant::now() + DRAIN_TAIL);
            loop {
                poll_reader(
                    &mut out,
                    &mut stdout,
                    &mut out_eof,
                    &mut out_cap,
                    &mut read_failed,
                )?;
                poll_reader(
                    &mut err,
                    &mut stderr,
                    &mut err_eof,
                    &mut err_cap,
                    &mut read_failed,
                )?;
                tree_exited = child.wait_tree_exit(Duration::ZERO).unwrap_or(false);
                if out_eof && err_eof && tree_exited {
                    break;
                }
                if Instant::now() >= cleanup_deadline {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
            break;
        }
        // The direct child was not accounted by the operation deadline. Do one
        // nonblocking state check, then report uncertainty and let ManagedChild
        // Drop terminate the owned tree; no reader threads can outlive this call.
        if timed_out {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    let capped = out_cap || err_cap;
    let incomplete = status.is_none() || !tree_exited || !out_eof || !err_eof || read_failed;
    if !tree_exited {
        let _ = child.terminate_tree();
    }
    let ok = status.is_some_and(|s| s.success()) && !timed_out && !incomplete && !capped;
    let stderr_text = String::from_utf8_lossy(&stderr);
    let outcome = if !ok && stderr_text.contains("sandbox_apply") {
        "ENV_BLOCKED"
    } else if timed_out {
        "timed_out"
    } else if status.is_none() {
        "outcome_unknown"
    } else if incomplete {
        "incomplete"
    } else if capped {
        "output_capped"
    } else if ok {
        "success"
    } else {
        "failed"
    };
    Ok(
        json!({"success":ok,"outcome":outcome,"return_code":status.and_then(|s|s.code()),"stdout":String::from_utf8_lossy(&stdout),"stderr":String::from_utf8_lossy(&stderr),"stdout_truncated":out_cap,"stderr_truncated":err_cap,"timed_out":timed_out,"incomplete":incomplete}),
    )
}

#[cfg(unix)]
fn set_nonblocking(fd: i32) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn poll_reader<R: Read>(
    reader: &mut R,
    kept: &mut Vec<u8>,
    eof: &mut bool,
    capped: &mut bool,
    failed: &mut bool,
) -> Result<(), (i64, &'static str)> {
    if *eof {
        return Ok(());
    }
    let mut buf = [0u8; 4096];
    match reader.read(&mut buf) {
        Ok(0) => *eof = true,
        Ok(n) => {
            let take = n.min(MAX_OUTPUT - kept.len());
            kept.extend_from_slice(&buf[..take]);
            if take < n {
                *capped = true;
            }
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
        Err(_) => {
            *failed = true;
            *eof = true;
        }
    }
    Ok(())
}

// ===========================================================================
// Slice A: safe managed asynchronous jobs
//
// These jobs are explicitly NOT durable and do NOT survive a WebCodex restart.
// They reuse the exact brokered preparation path (`prepare_shell` -> `spawn_broker`)
// that `shell_run` uses. Cancellation terminates the whole managed process tree,
// not just a direct PID. Output is captured incrementally and bounded; an
// uncertain process disposition is reported as OUTCOME_UNKNOWN rather than a
// fabricated success.
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum JobState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
    OutcomeUnknown,
}

impl JobState {
    fn label(self) -> &'static str {
        match self {
            JobState::Running => "JOB_RUNNING",
            JobState::Succeeded => "SUCCEEDED",
            JobState::Failed => "FAILED",
            JobState::Cancelled => "CANCELLED",
            JobState::TimedOut => "TIMED_OUT",
            JobState::OutcomeUnknown => "OUTCOME_UNKNOWN",
        }
    }
}

// Interleaved, bounded append log for one job's stdout/stderr. Each record is a
// single tag byte (0 = stdout, 1 = stderr) followed by a big-endian u32 length
// and the bytes. A single cursor indexes into the full emitted stream so one
// poll can return increments for both streams since the last cursor.
struct JobOutput {
    buf: Vec<u8>,
    total: u64,
    dropped: u64,
    truncated: bool,
}

impl JobOutput {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            total: 0,
            dropped: 0,
            truncated: false,
        }
    }

    fn push(&mut self, stream: u8, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let record_len = 5 + data.len();
        if self.buf.len() + record_len > JOB_STREAM_CAP {
            // Drop whole leading records until there is room.
            let mut drop = self.buf.len() + record_len - JOB_STREAM_CAP;
            let mut i = 0;
            while drop > 0 && i < self.buf.len() {
                let len = u32::from_be_bytes([
                    self.buf[i + 1],
                    self.buf[i + 2],
                    self.buf[i + 3],
                    self.buf[i + 4],
                ]) as usize;
                let rec = 5 + len;
                let step = rec.min(self.buf.len() - i);
                i += step;
                drop = drop.saturating_sub(step);
            }
            self.buf.drain(0..i);
            self.dropped += i as u64;
            self.truncated = true;
        }
        self.buf.push(stream);
        self.buf
            .extend_from_slice(&(data.len() as u32).to_be_bytes());
        self.buf.extend_from_slice(data);
        self.total += record_len as u64;
    }

    /// Snap an arbitrary byte offset down to the nearest record boundary.
    ///
    /// A caller controls the cursor, so it can arrive pointing into the middle
    /// of a record. Indexing there reads a length field out of payload bytes,
    /// immediately fails the `i + rec <= len` bound, and returns nothing — and
    /// because the next cursor is then the same offset, the caller is stuck
    /// forever on an empty delta. Snapping down to the enclosing boundary is
    /// lossless: the caller re-reads bytes it already had rather than skipping
    /// any it never saw.
    ///
    /// Returns the snapped offset and whether it was already exact.
    fn floor_boundary(&self, offset: usize) -> (usize, bool) {
        let mut boundary = 0usize;
        let mut exact = offset == 0;
        let mut i = 0usize;
        while i < offset && i + 5 <= self.buf.len() {
            let len = u32::from_be_bytes([
                self.buf[i + 1],
                self.buf[i + 2],
                self.buf[i + 3],
                self.buf[i + 4],
            ]) as usize;
            let rec = 5 + len;
            if i + rec > self.buf.len() {
                // A partial trailing record: it is not a usable boundary.
                break;
            }
            i += rec;
            if i == offset {
                exact = true;
            }
            if i < offset {
                boundary = i;
            }
        }
        if exact {
            (offset, true)
        } else {
            (boundary, false)
        }
    }

    // Returns (stdout_delta, stderr_delta, next_cursor, history_lost, capped).
    //
    // stdout and stderr are interleaved in ONE record stream, so they must be
    // served by ONE scan. Two independent scans each advancing their own cursor
    // can walk one cursor past records the other never delivered, which loses
    // output silently. `next_cursor` therefore only ever advances to a record
    // boundary that was actually delivered in full, so an incremental caller
    // can never silently skip output it has not seen.
    fn read_since(&self, cursor: u64) -> (Vec<u8>, Vec<u8>, u64, bool, bool) {
        let requested: usize = if cursor < self.dropped {
            0usize
        } else {
            (cursor - self.dropped) as usize
        };
        // A cursor below the eviction watermark means the caller's history was
        // discarded; `self.truncated` means it may have been discarded earlier.
        let history_lost: bool = cursor < self.dropped || self.truncated;
        // Never index into the middle of a record: that both loses the rest of
        // the stream and can stall the caller permanently.
        let (mut i, exact) = self.floor_boundary(requested.min(self.buf.len()));
        if !exact {
            // Re-reading from an earlier boundary means the caller's cursor was
            // not one this server issued, so its view of history is incomplete.
            // Report it as lost rather than letting it believe it is in sync.
            return (Vec::new(), Vec::new(), self.dropped + i as u64, true, false);
        }
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut capped = false;
        while i + 5 <= self.buf.len() {
            let len = u32::from_be_bytes([
                self.buf[i + 1],
                self.buf[i + 2],
                self.buf[i + 3],
                self.buf[i + 4],
            ]) as usize;
            let rec = 5 + len;
            if i + rec > self.buf.len() {
                break;
            }
            let stream = self.buf[i];
            let body = &self.buf[i + 5..i + rec];
            let target = if stream == 0 {
                &mut stdout
            } else {
                &mut stderr
            };
            // Stop BEFORE consuming a record that cannot be delivered whole, so
            // next_cursor never jumps past bytes the caller did not receive.
            if target.len() + body.len() > JOB_POLL_CAP {
                capped = true;
                break;
            }
            target.extend_from_slice(body);
            i += rec;
        }
        (
            stdout,
            stderr,
            self.dropped + i as u64,
            history_lost,
            capped,
        )
    }

    fn mark_truncated(&mut self) {
        self.truncated = true;
    }
}

/// Immutable terminal snapshot published atomically.
///
/// The three fields are written together and read together. Keeping them in
/// separate mutexes let a reader observe a half-published transition (a
/// running state with a final exit code), which is a self-contradictory answer
/// to "how did this job end?".
#[derive(Clone, Copy)]
struct JobTerminal {
    state: JobState,
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
}

struct JobRecord {
    project_id: String,
    started_at_unix: u64,
    deadline_unix: u64,
    cancel: Arc<AtomicBool>,
    terminal: Mutex<JobTerminal>,
    output: Arc<Mutex<JobOutput>>,
}

#[derive(Default)]
struct JobRegistry {
    jobs: BTreeMap<String, Arc<JobRecord>>,
    counter: AtomicU64,
}

impl JobRegistry {
    fn now_unix() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Bound retained job records while keeping recent results pollable.
    ///
    /// Dropping every terminal record on the next spawn would make a finished
    /// job unpollable, so terminal results are retained for a bounded window
    /// and only the oldest are evicted once that window is exceeded. A Running
    /// job is never evicted, and the runner thread holds its own
    /// `Arc<JobRecord>`, so eviction cannot orphan a managed process.
    ///
    /// Job ids embed a zero-padded monotonic sequence, so id order is
    /// insertion order.
    fn reclaim_terminal(&mut self) {
        let terminal: Vec<String> = self
            .jobs
            .iter()
            .filter(|(_, record)| {
                !matches!(record.terminal.lock().unwrap().state, JobState::Running)
            })
            .map(|(id, _)| id.clone())
            .collect();
        let excess = terminal.len().saturating_sub(MAX_RETAINED_TERMINAL_JOBS);
        for id in terminal.into_iter().take(excess) {
            self.jobs.remove(&id);
        }
    }

    fn spawn_job(
        &mut self,
        app: &App,
        project_id: &str,
        cwd_rel: &str,
        command: &str,
        timeout_seconds: u64,
    ) -> Result<Value, (i64, &'static str)> {
        // Bound the number of LIVE jobs, not the number ever started. Counting
        // every record ever inserted would make this a lifetime cap: after
        // MAX_CONCURRENT_JOBS starts the tool would fail forever, because
        // nothing ever removed entries. Terminal jobs are reclaimed here.
        self.reclaim_terminal();
        let live = self
            .jobs
            .values()
            .filter(|record| matches!(record.terminal.lock().unwrap().state, JobState::Running))
            .count();
        if live >= MAX_CONCURRENT_JOBS {
            return Err((
                -32000,
                "JOB_LIMIT_REACHED: too many concurrent jobs; poll or cancel existing ones first",
            ));
        }
        let (spec, toolchain) = prepare_shell(app, cwd_rel, command)?;
        let child = spawn_broker(&spec, &toolchain).map_err(|e| (-32000, broker_failure(&e)))?;
        let id_seq = self.counter.fetch_add(1, Ordering::SeqCst);
        let job_id = format!(
            "wcjob-{:08x}-{:x}",
            id_seq,
            JobRegistry::now_unix() & 0xffff
        );
        let deadline_seconds = timeout_seconds.clamp(1, JOB_TIMEOUT_MAX);
        let now = JobRegistry::now_unix();
        let record = Arc::new(JobRecord {
            project_id: project_id.to_string(),
            started_at_unix: now,
            deadline_unix: now + deadline_seconds,
            cancel: Arc::new(AtomicBool::new(false)),
            terminal: Mutex::new(JobTerminal {
                state: JobState::Running,
                exit_code: None,
                duration_ms: None,
            }),
            output: Arc::new(Mutex::new(JobOutput::new())),
        });
        self.jobs.insert(job_id.clone(), record.clone());
        let started = Instant::now();
        // SAFETY: `ManagedChild` is `Send` (owns a `Child`, a `u32`, and an
        // `AtomicBool`); the captured pipes are `ChildStdout`/`ChildStderr`, also
        // `Send`. The child is owned exclusively by this thread. The `JobRecord`
        // is shared through `Arc`; only its internal `Mutex`es are mutated.
        thread::spawn(move || {
            job_runner(record, child, started, deadline_seconds);
        });
        let deadline = now + deadline_seconds;
        Ok(json!({
            "job_id": job_id,
            "status": JobState::Running.label(),
            "durable": false,
            "survive_webcodex_restart": false,
            "started_at": now,
            "deadline": deadline,
            "timeout_seconds": deadline_seconds,
            "output_cursor": 0u64,
        }))
    }

    fn poll(
        &self,
        project_id: &str,
        job_id: &str,
        stdout_cursor: u64,
        stderr_cursor: u64,
    ) -> Result<Value, (i64, &'static str)> {
        let record = self
            .jobs
            .get(job_id)
            .ok_or((-32602, "INVALID_ARGUMENT: unknown or stale job id"))?;
        if record.project_id != project_id {
            return Err((
                -32000,
                "AUTHORITY_DENIED: job does not belong to the selected project",
            ));
        }
        // One lock for the whole snapshot. Reading state, exit_code and
        // duration_ms separately can report JOB_RUNNING alongside a populated
        // exit code, or a terminal status with a null duration, because the
        // writer publishes them one lock at a time.
        //
        // stdout and stderr share one interleaved record stream, so they are
        // read in a single scan from a single position. Taking the minimum of
        // the two cursors is lossless: a caller that has consumed up to N
        // re-reads from N and never skips a record it has not already seen.
        let from = stdout_cursor.min(stderr_cursor);
        let (out_d, err_d, next, history_lost, capped, total, state, exit_code, duration_ms) = {
            let guard = record.terminal.lock().unwrap();
            let output = record.output.lock().unwrap();
            let (out_d, err_d, next, history_lost, capped) = output.read_since(from);
            (
                out_d,
                err_d,
                next,
                history_lost,
                capped,
                output.total,
                guard.state,
                guard.exit_code,
                guard.duration_ms,
            )
        };
        Ok(json!({
            "job_id": job_id,
            "project_id": project_id,
            "status": state.label(),
            "durable": false,
            "survive_webcodex_restart": false,
            "started_at": record.started_at_unix,
            "deadline": record.deadline_unix,
            "exit_code": exit_code,
            "duration_ms": duration_ms,
            "stdout_delta": String::from_utf8_lossy(&out_d),
            "stderr_delta": String::from_utf8_lossy(&err_d),
            // One shared cursor for the interleaved stream; both fields are
            // returned so an incremental caller cannot desynchronize them.
            "stdout_cursor": next,
            "stderr_cursor": next,
            "bytes_captured": total,
            // True means output history was discarded or this response was
            // capped, i.e. the deltas are NOT the complete output.
            "stdout_truncated": history_lost || capped,
            "stderr_truncated": history_lost || capped,
            "output_history_lost": history_lost,
            "output_capped": capped,
        }))
    }

    fn cancel(&self, project_id: &str, job_id: &str) -> Result<Value, (i64, &'static str)> {
        let record = self
            .jobs
            .get(job_id)
            .ok_or((-32602, "INVALID_ARGUMENT: unknown or stale job id"))?;
        if record.project_id != project_id {
            return Err((
                -32000,
                "AUTHORITY_DENIED: job does not belong to the selected project",
            ));
        }
        let already_terminal = !matches!(record.terminal.lock().unwrap().state, JobState::Running);
        record.cancel.store(true, Ordering::SeqCst);
        Ok(json!({
            "job_id": job_id,
            "status": if already_terminal { "ALREADY_TERMINAL" } else { "CANCEL_REQUESTED" },
            "durable": false,
        }))
    }
}

#[cfg(unix)]
fn job_runner(
    record: Arc<JobRecord>,
    mut child: ManagedChild,
    started: Instant,
    timeout_seconds: u64,
) {
    use std::os::unix::io::AsRawFd;
    let cancel = record.cancel.clone();
    let output = record.output.clone();
    let mut stdout = match child.child_mut().stdout.take() {
        Some(o) => o,
        None => {
            record_terminal(&record, JobState::OutcomeUnknown, None, started);
            return;
        }
    };
    let mut stderr = match child.child_mut().stderr.take() {
        Some(e) => e,
        None => {
            record_terminal(&record, JobState::OutcomeUnknown, None, started);
            return;
        }
    };
    set_nonblocking(stdout.as_raw_fd()).ok();
    set_nonblocking(stderr.as_raw_fd()).ok();
    let mut out_eof = false;
    let mut err_eof = false;
    // A pipe read error is NOT clean EOF. Marking it as EOF would let a job with
    // silently lost output be reported SUCCEEDED/FAILED, which is exactly the
    // fabricated success this module forbids. Track it separately and force
    // OUTCOME_UNKNOWN whenever the captured output cannot be trusted.
    let mut read_failed = false;
    let deadline = started + Duration::from_secs(timeout_seconds);
    loop {
        let mut buf = [0u8; JOB_READ_CHUNK];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => {
                    out_eof = true;
                    break;
                }
                Ok(n) => output.lock().unwrap().push(0, &buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    // Not EOF: the stream failed, so the captured output is incomplete.
                    read_failed = true;
                    out_eof = true;
                    break;
                }
            }
        }
        loop {
            match stderr.read(&mut buf) {
                Ok(0) => {
                    err_eof = true;
                    break;
                }
                Ok(n) => output.lock().unwrap().push(1, &buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    read_failed = true;
                    err_eof = true;
                    break;
                }
            }
        }
        let status = child.try_wait().ok().flatten();
        if status.is_some() {
            let tree_exited = drain_job_pipes(
                &child,
                &output,
                &mut stdout,
                &mut stderr,
                &mut out_eof,
                &mut err_eof,
                &mut read_failed,
            );
            let code = status.unwrap().code();
            // Succeeded/FAILED require trustworthy output: both pipes must have
            // reached clean EOF, the whole process tree must be provably gone,
            // and no read may have failed. Anything else is OUTCOME_UNKNOWN —
            // an uncertain disposition is never reported as a clean result.
            let trustworthy = out_eof && err_eof && tree_exited && !read_failed;
            let outcome = if trustworthy {
                if status.unwrap().success() {
                    JobState::Succeeded
                } else {
                    JobState::Failed
                }
            } else {
                JobState::OutcomeUnknown
            };
            if !trustworthy {
                output.lock().unwrap().mark_truncated();
            }
            record_terminal(&record, outcome, code, started);
            return;
        }
        if cancel.load(Ordering::SeqCst) {
            let _ = child.terminate_tree();
            // The process may have exited on its own between the status poll
            // above and this cancellation. If a real exit code is observable,
            // keep it instead of discarding it and reporting a bare
            // CANCELLED, so the caller still sees how the process ended.
            let reaped = child.try_wait().ok().flatten();
            let code = reaped.and_then(|status| status.code());
            let tree_exited = child
                .wait_tree_exit(Duration::from_secs(2))
                .unwrap_or(false);
            let outcome = if tree_exited {
                JobState::Cancelled
            } else {
                JobState::OutcomeUnknown
            };
            // A terminated tree often still has buffered bytes in its pipes.
            // Drain them before recording the terminal state, and mark the
            // output truncated if the drain did not reach clean EOF — otherwise
            // the caller is told the output is complete while most of it is
            // silently discarded.
            let drained = drain_job_pipes(
                &child,
                &output,
                &mut stdout,
                &mut stderr,
                &mut out_eof,
                &mut err_eof,
                &mut read_failed,
            );
            if !(out_eof && err_eof && !read_failed) || !drained {
                output.lock().unwrap().mark_truncated();
            }
            record_terminal(&record, outcome, code, started);
            return;
        }
        if Instant::now() >= deadline {
            let _ = child.terminate_tree();
            let reaped = child.try_wait().ok().flatten();
            let code = reaped.and_then(|status| status.code());
            let tree_exited = child
                .wait_tree_exit(Duration::from_secs(2))
                .unwrap_or(false);
            let outcome = if tree_exited {
                JobState::TimedOut
            } else {
                JobState::OutcomeUnknown
            };
            // Same contract as cancellation: a timed-out tree can have buffered
            // output, so drain it and report truncation honestly.
            let drained = drain_job_pipes(
                &child,
                &output,
                &mut stdout,
                &mut stderr,
                &mut out_eof,
                &mut err_eof,
                &mut read_failed,
            );
            if !(out_eof && err_eof && !read_failed) || !drained {
                output.lock().unwrap().mark_truncated();
            }
            record_terminal(&record, outcome, code, started);
            return;
        }
        thread::sleep(JOB_SLEEP);
    }
}

/// Drain both job pipes to EOF (or to `DRAIN_TAIL`) and wait for the tree to
/// exit. Returns whether the whole tree was observed gone.
///
/// Shared by every terminal path. A path that terminates a job without
/// draining would discard whatever was still buffered and then report the
/// output as complete, which is the fabricated-success pattern this module
/// exists to prevent.
#[cfg(unix)]
fn drain_job_pipes(
    child: &ManagedChild,
    output: &Mutex<JobOutput>,
    stdout: &mut std::process::ChildStdout,
    stderr: &mut std::process::ChildStderr,
    out_eof: &mut bool,
    err_eof: &mut bool,
    read_failed: &mut bool,
) -> bool {
    use std::io::Read;
    let cleanup_deadline = Instant::now() + DRAIN_TAIL;
    let mut tree_exited;
    loop {
        let mut buf = [0u8; JOB_READ_CHUNK];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => {
                    *out_eof = true;
                    break;
                }
                Ok(n) => output.lock().unwrap().push(0, &buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    // Not EOF: the stream failed, so captured output is incomplete.
                    *read_failed = true;
                    *out_eof = true;
                    break;
                }
            }
        }
        loop {
            match stderr.read(&mut buf) {
                Ok(0) => {
                    *err_eof = true;
                    break;
                }
                Ok(n) => output.lock().unwrap().push(1, &buf[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    *read_failed = true;
                    *err_eof = true;
                    break;
                }
            }
        }
        tree_exited = child.wait_tree_exit(Duration::ZERO).unwrap_or(false);
        if *out_eof && *err_eof && tree_exited {
            break;
        }
        if Instant::now() >= cleanup_deadline {
            break;
        }
        thread::sleep(JOB_SLEEP);
    }
    tree_exited
}

#[cfg(not(unix))]
fn job_runner(
    record: Arc<JobRecord>,
    mut child: ManagedChild,
    started: Instant,
    _timeout_seconds: u64,
) {
    let _ = child.terminate_tree();
    record_terminal(&record, JobState::OutcomeUnknown, None, started);
}

fn record_terminal(record: &Arc<JobRecord>, state: JobState, code: Option<i32>, started: Instant) {
    // First terminal verdict wins. A later observation (for example a cancel
    // landing after the process already exited and was reaped) must not
    // overwrite a real recorded outcome with a weaker or contradictory one.
    //
    // The three fields are published together under one lock so a concurrent
    // poll can never read a state that disagrees with its exit code.
    let mut slot = record.terminal.lock().unwrap();
    if !matches!(slot.state, JobState::Running) {
        return;
    }
    slot.exit_code = code;
    slot.duration_ms = Some(Instant::now().duration_since(started).as_millis() as u64);
    slot.state = state;
}

// ===========================================================================
// Slice B: read-only LSP surface
//
// Reuses the accepted brokered `LspSupervisor` + `execute_lsp_operation`. The
// model never supplies a language-server executable, arguments, initialization
// command, workspace/executeCommand, or authority root: authority derives solely
// from the selected registered project, and all file paths are confined to it.
// ===========================================================================

fn lsp_run(app: &App, request: RunnerLspRequest, path: &str) -> Result<Value, (i64, &'static str)> {
    // Path confinement: the document must resolve inside the registered project.
    checked_path(&app.root, path, true).map_err(|_| {
        (
            -32602,
            "PATH_OUTSIDE_PROJECT: document path must resolve inside the registered project",
        )
    })?;
    let payload = RunnerLspPayload {
        project_id: app.project.id.clone(),
        request,
    };
    let supervisor = app.lsp.lock().unwrap();
    let deadline = Instant::now() + LSP_OPERATION_TIMEOUT;
    let envelope = execute_lsp_operation(app.root.clone(), &supervisor, &payload, deadline);
    if envelope.success {
        Ok(json!({
            "success": true,
            "status": "SUCCESS",
            "result": envelope.result.unwrap_or(Value::Null),
        }))
    } else {
        let err = envelope.error;
        let code = err
            .as_ref()
            .map(|e| e.code.clone())
            .unwrap_or_else(|| "LSP_UNAVAILABLE".to_string());
        let message = err.as_ref().map(|e| e.message.clone()).unwrap_or_default();
        Ok(json!({
            "success": false,
            "status": "LSP_UNAVAILABLE",
            "code": code,
            "message": message,
            "path": path,
        }))
    }
}

fn spawn_python(
    app: &App,
    args: &[&str],
    cwd: PathBuf,
    timeout: Duration,
) -> Result<Value, (i64, &'static str)> {
    let deadline = Instant::now() + timeout;
    let spec = SpawnSpec::new(&app.python, cwd.clone(), app.authority.plan())
        .args(args.iter().copied())
        .env(EnvPolicy::Minimal)
        .env_var("PATH", "/opt/homebrew/bin:/usr/bin:/bin")
        .env_var("HOME", &cwd)
        .env_var("TMPDIR", &cwd)
        .stdin(StreamPolicy::Null)
        .stdout(StreamPolicy::Piped)
        .stderr(StreamPolicy::Piped);
    let roots = app.authority.toolchain_roots_for(&app.python);
    let mut child = spawn_broker(&spec, &roots).map_err(|e| (-32000, broker_failure(&e)))?;
    let value = capture_child(&mut child, deadline)?;
    Ok(value)
}

fn spawn_broker(
    spec: &SpawnSpec,
    roots: &[webcodex_process::execution_broker::TrustedToolchainRoot],
) -> Result<webcodex_process::ManagedChild, BrokerError> {
    #[cfg(target_os = "macos")]
    {
        ExecutionBroker::new().spawn_with_toolchain(spec, roots)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (spec, roots);
        Err(BrokerError::UnsupportedPlatform)
    }
}

fn checked_path(root: &Path, relative: &str, allow_missing: bool) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("unsafe relative path".into());
    }
    if path.components().any(|c| c.as_os_str() == ".git") {
        return Err("git internals unavailable".into());
    }
    let joined = root.join(path);
    let resolved = if allow_missing && !joined.exists() {
        let parent = joined
            .parent()
            .ok_or("no parent")?
            .canonicalize()
            .map_err(|_| "invalid parent")?;
        parent.join(joined.file_name().ok_or("no name")?)
    } else {
        joined.canonicalize().map_err(|_| "path unavailable")?
    };
    if !resolved.starts_with(root) {
        return Err("path escaped project".into());
    }
    let lower = relative.to_ascii_lowercase();
    if lower.split('/').any(|part| {
        part == ".env"
            || part.starts_with(".env.")
            || matches!(part, "id_rsa" | "id_ed25519" | "credentials" | "secrets")
    }) {
        return Err("sensitive path denied".into());
    }
    Ok(resolved)
}

const HELPER: &str = r#"import os,sys,json,hashlib,tempfile,stat
root=os.getcwd()
def fail(msg):
 code='POLICY_DENIED' if any(x in msg for x in ('unsafe path','path escaped','sensitive path','regular files only','.git')) else ('STALE_REVISION' if 'stale revision' in msg else 'OPERATION_FAILED')
 print(json.dumps({'success':False,'code':code,'error':msg},separators=(',',':')));sys.exit(2)
def path(rel,missing=False):
 if not isinstance(rel,str) or not rel or os.path.isabs(rel): raise ValueError('relative path required')
 parts=rel.replace('\\','/').split('/')
 if any(x in ('','.','..','.git') for x in parts): raise ValueError('unsafe path')
 low=rel.lower()
 if any(x=='.env' or x.startswith('.env.') or x in ('id_rsa','id_ed25519','credentials','secrets') for x in low.split('/')): raise ValueError('sensitive path')
 p=os.path.join(root,*parts); parent=os.path.realpath(os.path.dirname(p)); resolved=os.path.join(parent,os.path.basename(p)) if missing and not os.path.exists(p) else os.path.realpath(p)
 if os.path.commonpath((root,resolved))!=root: raise ValueError('path escaped project')
 resolved_rel=os.path.relpath(resolved,root).replace(os.sep,'/').lower().split('/')
 if any(x=='.git' for x in resolved_rel): raise ValueError('git internals unavailable: .git')
 if any(x=='.env' or x.startswith('.env.') or x in ('id_rsa','id_ed25519','credentials','secrets') for x in resolved_rel): raise ValueError('sensitive path denied')
 return resolved
def read_regular(p):
 fd=os.open(p,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK)
 try:
  st=os.fstat(fd)
  if not stat.S_ISREG(st.st_mode): raise ValueError('regular files only')
  if st.st_size>2097152: raise ValueError('file exceeds 2 MiB limit')
  with os.fdopen(fd,'rb',closefd=False) as f: data=f.read(2097153)
  if len(data)>2097152: raise ValueError('file exceeds 2 MiB limit')
  return data,stat.S_IMODE(st.st_mode)
 finally: os.close(fd)
try:
 req=json.loads(sys.argv[1]);op=req['op'];a=req['args']
 if op=='read':
  p=path(a['path']);data,_=read_regular(p)
  rev=hashlib.sha256(data).hexdigest();off=a['offset'];lim=min(a['limit'],1400)
  if off<0 or lim<1: raise ValueError('invalid range')
  part=data[off:off+lim];print(json.dumps({'success':True,'path':a['path'],'revision':rev,'offset':off,'content':part.decode('utf-8','replace'),'truncated':off+len(part)<len(data)}))
 elif op=='patch':
  p=path(a['path']);data,mode=read_regular(p)
  rev=hashlib.sha256(data).hexdigest()
  old=a['old_text'].encode();new=a['new_text'].encode()
  if not old: raise ValueError('old_text must not be empty')
  if rev!=a['revision']:
   # Bounded self-recovery context: report the current revision and whether the
   # intended old_text is still present, so the model can re-read and retry.
   print(json.dumps({'success':False,'code':'STALE_REVISION','status':'PATCH_CONFLICT','error':'stale revision','expected_revision':a['revision'],'current_revision':rev,'old_text_present':data.count(old)==1,'old_text_occurrences':data.count(old),'retry_guidance':'Re-read the file with files_read to obtain current_revision and current content, then retry files_apply_patch with that revision and the exact current old_text.','file_size':len(data)},separators=(',',':')));sys.exit(3)
  if data.count(old)!=1:
   print(json.dumps({'success':False,'code':'PATCH_CONFLICT','status':'PATCH_CONFLICT','error':'old_text must match exactly once','expected_revision':rev,'current_revision':rev,'old_text_present':False,'old_text_occurrences':data.count(old),'retry_guidance':'Re-read the file with files_read and choose an old_text snippet that occurs exactly once in the current content.','file_size':len(data)},separators=(',',':')));sys.exit(3)
  updated=data.replace(old,new,1)
  if len(updated)>2097152: raise ValueError('updated file exceeds 2 MiB limit')
  fd,tmp=tempfile.mkstemp(prefix='.webcodex-',dir=os.path.dirname(p))
  try:
   f=os.fdopen(fd,'wb');f.write(updated);f.flush();os.fsync(f.fileno());os.fchmod(f.fileno(),mode);f.close()
   latest,_=read_regular(p)
   if hashlib.sha256(latest).hexdigest()!=rev: raise ValueError('stale revision at commit')
   os.replace(tmp,p)
  finally:
   if os.path.exists(tmp): os.unlink(tmp)
  print(json.dumps({'success':True,'status':'SUCCESS','revision':hashlib.sha256(updated).hexdigest()}))
 elif op=='search':
  q=a['query'].encode();
  if not q: raise ValueError('query must not be empty')
  matches=[];seen=0;truncated=False;walk_error=[False]
  def on_walk_error(_): walk_error[0]=True
  for base,dirs,files in os.walk(root,followlinks=False,onerror=on_walk_error):
   dirs[:]=[d for d in dirs if d!='.git' and not os.path.islink(os.path.join(base,d))]
   for name in files:
    seen+=1
    if seen>1000: truncated=True;break
    p=os.path.join(base,name);rel=os.path.relpath(p,root)
    try:
     safe=path(rel);st=os.stat(safe,follow_symlinks=False)
     if not __import__('stat').S_ISREG(st.st_mode): continue
     data,_=read_regular(safe)
    except Exception:
     truncated=True
     continue
    for i,line in enumerate(data.splitlines(),1):
     if q in line:
      matches.append({'path':rel,'line':i,'text':line[:160].decode('utf-8','replace')})
      if len(matches)>=40: truncated=True;break
    if len(matches)>=40: break
   if seen>1000 or len(matches)>=40: break
  truncated=truncated or walk_error[0]
  result={'success':True,'matches':matches,'truncated':truncated,'files_scanned':min(seen,1000)}
  while len(json.dumps(result).encode())>10000 and matches: matches.pop();truncated=True
  result['truncated']=truncated
  print(json.dumps(result))
 else: raise ValueError('unknown helper operation')
except Exception as e: fail(str(e)[:160])
"#;
