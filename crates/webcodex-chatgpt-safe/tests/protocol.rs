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
            "files_search",
            "files_read",
            "files_apply_patch",
            "shell_run",
            "git_status",
            "git_diff"
        ]
    );
    for withheld in [
        "job_start",
        "job_poll",
        "job_cancel",
        "root",
        "network",
        "environment",
        "profile",
        "shell_interpreter",
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
#[ignore = "requires real macOS broker enforcement; ENV_BLOCKED must fail the harness"]
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
#[ignore = "requires real macOS broker enforcement; ENV_BLOCKED must fail the harness"]
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
#[ignore = "requires real macOS broker enforcement; ENV_BLOCKED must fail the harness"]
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
