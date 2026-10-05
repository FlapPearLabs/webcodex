#![cfg(target_os = "macos")]

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

struct Fixture {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    registry: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temporary fixture");
        let root = temp.path().join("registered-project");
        std::fs::create_dir(&root).expect("project directory");
        let registry = temp.path().join("registry.json");
        std::fs::write(
            &registry,
            json!({"id":"project-1","name":"Fixture","root":root}).to_string(),
        )
        .expect("registry file");
        Self {
            _temp: temp,
            root,
            registry,
        }
    }

    fn call(&self, requests: &[Value]) -> Vec<Value> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_webcodex-chatgpt-safe"))
            .args(["serve", "--profile", "chatgpt-safe", "--registry"])
            .arg(&self.registry)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start stdio gateway");
        {
            let stdin = child.stdin.as_mut().expect("child stdin");
            for request in requests {
                writeln!(stdin, "{request}").expect("write request");
            }
        }
        let output = child.wait_with_output().expect("gateway exit");
        assert!(
            output.status.success(),
            "gateway failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("UTF-8 JSON-RPC lines")
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON response"))
            .collect()
    }
}

#[test]
fn tools_list_is_the_exact_closed_safe_surface() {
    let fixture = Fixture::new();
    let responses = fixture.call(&[json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})]);
    let tools = responses[0]["result"]["tools"]
        .as_array()
        .expect("tools array");
    let names: Vec<_> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
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
            "lsp_diagnostics"
        ]
    );
    // Authority-widening and hidden-capability fields must never appear in any
    // tool schema, including the new Jobs and LSP surfaces.
    for withheld in [
        "root",
        "network",
        "environment",
        "profile",
        "shell_interpreter",
        "authority",
        "workspace_root",
        "server",
        "executable",
        "server_path",
        "command_path",
        "args",
        "server_args",
        "initialize_command",
        "executeCommand",
        "execute_command",
        "codeAction",
        "code_action",
        "detach",
        "session",
        "durable",
        "approval",
        "full_access",
    ] {
        assert!(
            tools
                .iter()
                .all(|tool| tool["inputSchema"]["properties"].get(withheld).is_none()),
            "withheld field appeared: {withheld}"
        );
    }
    assert!(tools
        .iter()
        .all(|tool| tool["inputSchema"]["additionalProperties"] == true
            || tool["inputSchema"]["additionalProperties"] == false));
    assert!(tools
        .iter()
        .all(|tool| tool["inputSchema"]["additionalProperties"] == false));
}

#[test]
fn job_surface_rejects_authority_widening_and_unknown_job_ids() {
    let fixture = Fixture::new();
    let responses = fixture.call(&[
        // Job arguments must not be able to claim an unregistered project.
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"job_start","arguments":{"project_id":"unregistered","cwd":".","command":"echo hi","timeout_seconds":60}}}),
        // Unknown/stale job id is a bounded business rejection, not a panic.
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"job_poll","arguments":{"project_id":"project-1","job_id":"wcjob-does-not-exist","stdout_cursor":0,"stderr_cursor":0}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"job_cancel","arguments":{"project_id":"project-1","job_id":"wcjob-does-not-exist"}}}),
        // Authority-widening extra keys are rejected by exact schema validation.
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"job_start","arguments":{"project_id":"project-1","cwd":".","command":"echo hi","timeout_seconds":60,"environment":"inherit"}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"job_start","arguments":{"project_id":"project-1","cwd":"/","command":"echo hi","timeout_seconds":60}}}),
        // An unwaited-out job id belonging to another project is denied.
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"job_poll","arguments":{"project_id":"unregistered","job_id":"wcjob-anything","stdout_cursor":0,"stderr_cursor":0}}}),
    ]);
    assert_eq!(responses[0]["error"]["code"], -32000);
    assert_eq!(responses[1]["error"]["code"], -32602);
    assert_eq!(responses[2]["error"]["code"], -32602);
    assert_eq!(
        responses[3]["error"]["code"], -32602,
        "extra key must fail closed"
    );
    assert_eq!(
        responses[4]["error"]["code"], -32602,
        "absolute cwd must fail closed"
    );
    assert_eq!(responses[5]["error"]["code"], -32000);
}

#[test]
fn lsp_surface_cannot_be_given_an_executable_or_execute_command() {
    let fixture = Fixture::new();
    let responses = fixture.call(&[
        // Caller cannot choose the language-server executable.
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"lsp_symbols","arguments":{"project_id":"project-1","path":"src/lib.rs","limit":10,"executable":"/bin/sh"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"lsp_definition","arguments":{"project_id":"project-1","path":"src/lib.rs","line":1,"column":1,"limit":10,"server_path":"/tmp/evil"}}}),
        // Caller cannot request workspace/executeCommand.
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"lsp_references","arguments":{"project_id":"project-1","path":"src/lib.rs","line":1,"column":1,"include_declaration":true,"limit":10,"executeCommand":"workspace/executeCommand"}}}),
        // Paths outside the registered project are refused before any server start.
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"lsp_diagnostics","arguments":{"project_id":"project-1","path":"/etc/passwd","limit":10}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"lsp_symbols","arguments":{"project_id":"project-1","path":"../../escape.rs","limit":10}}}),
        // Unregistered project has no LSP authority at all.
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"lsp_symbols","arguments":{"project_id":"unregistered","path":"src/lib.rs","limit":10}}}),
        // Positions are bounded and 1-based.
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"lsp_definition","arguments":{"project_id":"project-1","path":"src/lib.rs","line":0,"column":1,"limit":10}}}),
    ]);
    for response in responses.iter().take(5) {
        assert_eq!(
            response["error"]["code"], -32602,
            "LSP authority widening was not rejected at the schema/path gate: {response}"
        );
    }
    // An unregistered project has no LSP authority at all: -32000, not a schema error.
    assert_eq!(responses[5]["error"]["code"], -32000);
}

#[test]
fn project_selection_and_tool_gateway_reject_unknown_authority() {
    let fixture = Fixture::new();
    let responses = fixture.call(&[
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"project_select","arguments":{"project_id":"unregistered"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"shell","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project_list","arguments":{"root":"/"}}}),
    ]);
    assert_eq!(responses[0]["error"]["code"], -32000);
    assert_eq!(responses[1]["error"]["code"], -32602);
    assert_eq!(responses[2]["error"]["code"], -32602);
}

#[test]
fn optional_mcp_metadata_is_ignored_without_changing_tool_authority() {
    let fixture = Fixture::new();
    let responses = fixture.call(&[
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"project_list","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"project_list","arguments":{},"_meta":{"progressToken":"synthetic-progress","openai/locale":"zh-CN","openai/subject":{"root":"/synthetic/root","environment":"synthetic-environment","project_id":"unregistered"}}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"project_select","arguments":{"project_id":"project-1"},"_meta":{"project_id":"unregistered","registered":true,"root":"/synthetic/root","environment":"synthetic-environment"}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"project_select","arguments":{"project_id":"unregistered"},"_meta":{"project_id":"project-1","registered":true}}}),
    ]);

    let expected_projects = json!({"projects":[{"project_id":"project-1","name":"Fixture"}]});
    assert_eq!(
        responses[0]["result"]["structuredContent"],
        expected_projects
    );
    assert_eq!(
        responses[1]["result"]["structuredContent"],
        expected_projects
    );
    assert_eq!(
        responses[2]["result"]["structuredContent"],
        json!({"selected":"project-1","authority_changed":false})
    );
    assert_eq!(responses[3]["error"]["code"], -32000);
}

#[test]
fn malformed_metadata_and_extra_authority_fields_fail_closed() {
    let fixture = Fixture::new();
    let malformed_metadata = [
        Value::Null,
        json!([]),
        json!("synthetic"),
        json!(7),
        json!(false),
    ];
    let mut requests: Vec<Value> = malformed_metadata
        .into_iter()
        .enumerate()
        .map(|(index, meta)| {
            json!({"jsonrpc":"2.0","id":index+1,"method":"tools/call","params":{"name":"project_list","arguments":{},"_meta":meta}})
        })
        .collect();
    requests.extend([
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"project_list","arguments":{},"root":"/synthetic/root","_meta":{}}}),
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"project_select","arguments":{"project_id":"project-1","_meta":{}},"_meta":{}}}),
        json!({"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"project_select","arguments":{"project_id":"project-1","root":"/synthetic/root"},"_meta":{}}}),
        json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"project_list"}}),
        json!({"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"project_list","_meta":{}}}),
        json!({"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"job_start","arguments":{},"_meta":{"registered":true}}}),
    ]);

    let responses = fixture.call(&requests);
    assert_eq!(responses.len(), requests.len());
    for response in &responses {
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
}

#[test]
fn rpc_version_and_unknown_methods_fail_closed() {
    let fixture = Fixture::new();
    let responses = fixture.call(&[
        json!({"jsonrpc":"1.0","id":1,"method":"ping"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"files_read","arguments":{"project_id":"project-1","path":"x","offset":0,"limit":1,"root":"/"}}}),
    ]);
    assert_eq!(responses[0]["error"]["code"], -32600);
    assert_eq!(responses[1]["error"]["code"], -32602);
}

#[test]
fn brokered_file_gate_rejects_git_and_external_paths_and_stale_edits() {
    let fixture = Fixture::new();
    let file = fixture.root.join("sample.txt");
    std::fs::write(&file, "needle\nold\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/passwd", fixture.root.join("escape.txt")).unwrap();
    let responses = fixture.call(&[
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"files_search","arguments":{"project_id":"project-1","query":"needle"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"files_read","arguments":{"project_id":"project-1","path":".git/config","offset":0,"limit":10}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"files_read","arguments":{"project_id":"project-1","path":"escape.txt","offset":0,"limit":10}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"files_apply_patch","arguments":{"project_id":"project-1","path":"sample.txt","revision":"0000000000000000000000000000000000000000000000000000000000000000","old_text":"old","new_text":"new"}}}),
    ]);
    let search = &responses[0]["result"]["structuredContent"];
    assert_eq!(
        search["success"], true,
        "ENV_BLOCKED is not a pass: {}",
        responses[0]
    );
    assert_eq!(search["matches"][0]["path"], "sample.txt");
    for denied in [&responses[1], &responses[2]] {
        assert_eq!(
            denied["result"]["isError"], true,
            "denied path was accepted: {denied}"
        );
        assert_eq!(denied["result"]["structuredContent"]["success"], false);
    }
    assert_eq!(
        responses[3]["result"]["structuredContent"]["success"],
        false
    );
    assert_eq!(
        responses[3]["result"]["structuredContent"]["outcome"],
        "STALE_REVISION"
    );
}

#[test]
fn brokered_file_gate_rechecks_resolved_sensitive_paths_and_keeps_safe_links() {
    let fixture = Fixture::new();
    let git = fixture.root.join(".git");
    std::fs::create_dir(&git).unwrap();
    std::fs::write(fixture.root.join(".env"), "dummy-env-value").unwrap();
    std::fs::write(git.join("config"), "dummy-git-config").unwrap();
    std::fs::write(fixture.root.join("ordinary.txt"), "safe project data").unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(".env", fixture.root.join("env-alias.txt")).unwrap();
        std::os::unix::fs::symlink("ordinary.txt", fixture.root.join("ordinary-link.txt")).unwrap();
        std::os::unix::fs::symlink(".git/config", fixture.root.join("git-alias.txt")).unwrap();
        std::os::unix::fs::symlink("env-alias.txt", fixture.root.join("env-chain.txt")).unwrap();
    }
    let responses = fixture.call(&[
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"files_read","arguments":{"project_id":"project-1","path":"env-alias.txt","offset":0,"limit":100}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"files_read","arguments":{"project_id":"project-1","path":"env-chain.txt","offset":0,"limit":100}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"files_read","arguments":{"project_id":"project-1","path":"git-alias.txt","offset":0,"limit":100}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"files_read","arguments":{"project_id":"project-1","path":"ordinary-link.txt","offset":0,"limit":100}}}),
    ]);
    for denied in [&responses[0], &responses[1], &responses[2]] {
        let content = &denied["result"]["structuredContent"];
        assert_eq!(
            content["success"], false,
            "sensitive alias accepted: {denied}"
        );
        assert_eq!(content["outcome"], "POLICY_DENIED");
    }
    assert_eq!(
        responses[3]["result"]["structuredContent"]["success"], true,
        "ordinary in-project symlink should remain readable: {}",
        responses[3]
    );
    assert_eq!(
        responses[3]["result"]["structuredContent"]["content"],
        "safe project data"
    );
}

#[test]
fn brokered_search_reports_unreadable_paths_as_partial() {
    let fixture = Fixture::new();
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "HOST_UNAVAILABLE: chmod permission checks are ineffective when running as root"
    );

    use std::os::unix::fs::PermissionsExt;
    let unreadable_dir = fixture.root.join("unreadable-dir");
    std::fs::create_dir(&unreadable_dir).unwrap();
    std::fs::write(unreadable_dir.join("needle.txt"), "dummyneedle").unwrap();
    let positive = fixture.call(&[json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"files_search","arguments":{"project_id":"project-1","query":"dummyneedle"}}})]);
    assert_eq!(
        positive[0]["result"]["structuredContent"]["success"], true,
        "search should return its structured result: {}",
        positive[0]
    );
    assert_eq!(
        positive[0]["result"]["structuredContent"]["truncated"], false,
        "a readable clean fixture must complete without truncation: {}",
        positive[0]
    );
    assert_eq!(
        positive[0]["result"]["structuredContent"]["matches"][0]["path"],
        "unreadable-dir/needle.txt",
        "search should find the readable fixture file: {}",
        positive[0]
    );

    std::fs::set_permissions(&unreadable_dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let partial_dir = fixture.call(&[json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"files_search","arguments":{"project_id":"project-1","query":"dummyneedle"}}})]);
    std::fs::set_permissions(&unreadable_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        partial_dir[0]["result"]["structuredContent"]["success"], true,
        "partial search should return its structured result: {}",
        partial_dir[0]
    );
    assert_eq!(
        partial_dir[0]["result"]["structuredContent"]["truncated"], true,
        "an unreadable directory must make search partial: {}",
        partial_dir[0]
    );

    let unreadable = fixture.root.join("unreadable.txt");
    std::fs::write(&unreadable, "dummy restricted data").unwrap();
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
    let partial_file = fixture.call(&[json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"files_search","arguments":{"project_id":"project-1","query":"not-present"}}})]);
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        partial_file[0]["result"]["structuredContent"]["success"], true,
        "partial search should return its structured result: {}",
        partial_file[0]
    );
    assert_eq!(
        partial_file[0]["result"]["structuredContent"]["truncated"], true,
        "an unreadable file must make search partial: {}",
        partial_file[0]
    );
}
