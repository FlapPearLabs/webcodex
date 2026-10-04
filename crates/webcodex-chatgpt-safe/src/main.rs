use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
#[cfg(unix)]
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::process::ExitStatus;
use std::thread;
use std::time::{Duration, Instant};
use webcodex_process::execution_broker::{
    BrokerError, EnvPolicy, ExecutionBroker, SpawnSpec, StreamPolicy, WorkspaceAuthority,
};
use webcodex_workspace::git_broker::run_git_bounded_read;

const MAX_REQUEST: usize = 64 * 1024;
const MAX_HELPER_REQUEST: usize = 32 * 1024;
const MAX_OUTPUT: usize = 12 * 1024;
const GIT_TIMEOUT: Duration = Duration::from_secs(15);
const SHELL_TIMEOUT_MAX: u64 = 30;
const DRAIN_TAIL: Duration = Duration::from_millis(250);
const PYTHON: &str = "/opt/homebrew/bin/python3";
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
const SAFE_TOOLS: [&str; 8] = [
    "project_list",
    "project_select",
    "files_search",
    "files_read",
    "files_apply_patch",
    "shell_run",
    "git_status",
    "git_diff",
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
}

fn main() {
    if let Err(error) = run() {
        eprintln!("webcodex-chatgpt-safe: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
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
        "serve" if profile.as_deref() == Some("chatgpt-safe") => serve(&app)?,
        "serve" => return Err("serve requires --profile chatgpt-safe".into()),
        _ => return Err("command must be serve, status, or doctor".into()),
    }
    Ok(())
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
        ("files_search", "Search literal text within the registered project. Results are bounded; if any path is unreadable, results are partial and truncated is true.", json!({"type":"object","properties":{"project_id":{"type":"string"},"query":{"type":"string","minLength":1,"maxLength":512}},"required":["project_id","query"],"additionalProperties":false})),
        ("files_read", "Read a bounded range from a project-relative file.", json!({"type":"object","properties":{"project_id":{"type":"string"},"path":{"type":"string","minLength":1,"maxLength":512},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":1400}},"required":["project_id","path","offset","limit"],"additionalProperties":false})),
        ("files_apply_patch", "Replace exactly one unique text occurrence only after verifying the current file SHA-256 revision; does not create files. After a timeout or incomplete result, read the current revision before retrying to avoid repeating an effect.", json!({"type":"object","properties":{"project_id":{"type":"string"},"path":{"type":"string","minLength":1,"maxLength":512},"revision":{"type":"string","pattern":"^[a-f0-9]{64}$"},"old_text":{"type":"string","maxLength":8192},"new_text":{"type":"string","maxLength":8192}},"required":["project_id","path","revision","old_text","new_text"],"additionalProperties":false})),
        ("shell_run", "Run a foreground command in the project through the deny-network process broker; output is bounded. Success covers the direct child, owned process group, and captured streams. Timeout is not a whole-family deadline; daemonized/setsid processes and durable execution are unsupported. Timeout or incomplete results may already have effects; inspect state before retrying.", json!({"type":"object","properties":{"project_id":{"type":"string"},"cwd":{"type":"string"},"command":{"type":"string","maxLength":16384},"timeout_seconds":{"type":"integer","minimum":1,"maximum":30}},"required":["project_id","cwd","command","timeout_seconds"],"additionalProperties":false})),
        ("git_status", "Read fixed bounded Git status and branch/head metadata.", json!({"type":"object","properties":{"project_id":{"type":"string"}},"required":["project_id"],"additionalProperties":false})),
        ("git_diff", "Read fixed bounded Git diff without external diff or text conversion.", json!({"type":"object","properties":{"project_id":{"type":"string"}},"required":["project_id"],"additionalProperties":false})),
    ];
    json!({"tools":defs.into_iter().map(|(name,description,input_schema)|json!({"name":name,"description":description,"inputSchema":input_schema,"annotations":{"readOnlyHint":!matches!(name,"files_apply_patch"|"shell_run"),"destructiveHint":matches!(name,"files_apply_patch"|"shell_run"),"idempotentHint":!matches!(name,"files_apply_patch"|"shell_run"),"openWorldHint":false}})).collect::<Vec<_>>()})
}

fn tool_call(app: &App, params: &Value) -> Result<Value, (i64, &'static str)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| SAFE_TOOLS.contains(n))
        .ok_or((-32602, "unknown tool"))?;
    exact_keys(params, &["name", "arguments"])?;
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
fn check_project(app: &App, args: &Value) -> Result<(), (i64, &'static str)> {
    if args.get("project_id").and_then(Value::as_str) == Some(&app.project.id) {
        Ok(())
    } else {
        Err((-32000, "unknown project identity"))
    }
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

fn shell(app: &App, args: &Value) -> Result<Value, (i64, &'static str)> {
    let cwd_rel = args["cwd"]
        .as_str()
        .ok_or((-32602, "cwd must be project-relative"))?;
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
    let command = args["command"]
        .as_str()
        .ok_or((-32602, "command must be a string"))?;
    if command.is_empty() || command.len() > 16 * 1024 {
        return Err((-32602, "command length is invalid"));
    }
    let timeout = args["timeout_seconds"]
        .as_u64()
        .ok_or((-32602, "timeout_seconds must be an integer"))?
        .clamp(1, SHELL_TIMEOUT_MAX);
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
  if rev!=a['revision']: raise ValueError('stale revision')
  old=a['old_text'].encode();new=a['new_text'].encode()
  if not old or data.count(old)!=1: raise ValueError('old_text must match exactly once')
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
  print(json.dumps({'success':True,'revision':hashlib.sha256(updated).hexdigest()}))
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
