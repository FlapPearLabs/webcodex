#![cfg(not(target_os = "macos"))]

use serde_json::json;
use std::process::{Command, Stdio};

#[test]
fn serve_fails_closed_without_emitting_tool_results() {
    let temp = tempfile::tempdir().expect("temporary fixture");
    let root = temp.path().join("registered-project");
    std::fs::create_dir(&root).expect("project directory");
    let registry = temp.path().join("registry.json");
    std::fs::write(
        &registry,
        json!({"id":"project-1","name":"Fixture","root":root}).to_string(),
    )
    .expect("registry file");

    let output = Command::new(env!("CARGO_BIN_EXE_webcodex-chatgpt-safe"))
        .args(["serve", "--profile", "chatgpt-safe", "--registry"])
        .arg(&registry)
        .env_remove("HOME")
        .stdin(Stdio::null())
        .output()
        .expect("start stdio gateway");

    assert!(
        !output.status.success(),
        "unsupported host must fail closed"
    );
    assert!(
        output.stdout.is_empty(),
        "startup failure must not emit a tool result: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
