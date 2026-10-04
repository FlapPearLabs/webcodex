use super::*;
use crate::runner_operation::RunnerPersistentShellOperation;
use crate::runner_protocol::{PersistentShellRequest, ShellJobContext};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use webcodex_persistent_shell::{
    BoundedBuffer, CompletionProgress, ControlFrame, ShellError, ShellIdentity, ShellTransport,
    TransportMetadata, WaitOutcome,
};

#[derive(Default)]
struct SpyState {
    writes: AtomicUsize,
    shutdowns: AtomicUsize,
}

struct SpyTransport {
    state: Arc<SpyState>,
    cwd: PathBuf,
    stdout: Arc<Mutex<BoundedBuffer>>,
    stderr: Arc<Mutex<BoundedBuffer>>,
}

impl SpyTransport {
    fn new(state: Arc<SpyState>, cwd: PathBuf) -> Self {
        Self {
            state,
            cwd,
            stdout: Arc::new(Mutex::new(BoundedBuffer::new(4096))),
            stderr: Arc::new(Mutex::new(BoundedBuffer::new(4096))),
        }
    }
}

impl ShellTransport for SpyTransport {
    fn set_expected_token(&self, _token: &str) {}

    fn write_command(&self, _command: &str, _token: &str) -> Result<(), ShellError> {
        self.state.writes.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn wait_for_completion(
        &self,
        token: &str,
        _timeout: Duration,
        progress: &mut CompletionProgress,
    ) -> WaitOutcome {
        progress.stdout_synced = true;
        progress.stderr_synced = true;
        let frame = ControlFrame {
            token: token.to_string(),
            status: 0,
            cwd: self.cwd.clone(),
        };
        progress.control = Some(ControlFrame {
            token: frame.token.clone(),
            status: frame.status,
            cwd: frame.cwd.clone(),
        });
        WaitOutcome::Frame(frame)
    }

    fn try_wait(&self) -> Option<std::process::ExitStatus> {
        None
    }

    fn interrupt(&self) {}

    fn shutdown(&self) {
        self.state.shutdowns.fetch_add(1, Ordering::SeqCst);
    }

    fn terminate_remaining_group_after_exit(&self) {}

    fn stdout(&self) -> &Arc<Mutex<BoundedBuffer>> {
        &self.stdout
    }

    fn stderr(&self) -> &Arc<Mutex<BoundedBuffer>> {
        &self.stderr
    }

    fn metadata(&self) -> Option<TransportMetadata> {
        Some(TransportMetadata {
            resource: Some("lab".to_string()),
            generation: Some(1),
        })
    }
}

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, RunnerPolicy) {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    let projects = temp.path().join("project-registry");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&projects).unwrap();
    let project_path = toml::Value::String(project.to_string_lossy().into_owned());
    let backslash_path = r"C:\work\demo";
    let encoded_backslash_path = toml::Value::String(backslash_path.to_owned()).to_string();
    let decoded: toml::Value =
        toml::from_str(&format!("path = {encoded_backslash_path}\n")).unwrap();
    assert_eq!(decoded["path"].as_str(), Some(backslash_path));
    std::fs::write(
        projects.join("demo.toml"),
        format!("id = \"demo\"\npath = {project_path}\n"),
    )
    .unwrap();
    let policy = RunnerPolicy {
        allow_raw_shell: true,
        allow_cwd_anywhere: false,
        allowed_roots: vec![project.clone()],
        max_timeout_secs: 30,
        max_output_bytes: 4096,
    };
    (temp, project, projects, policy)
}

fn operation(
    action: &str,
    shell_id: &str,
    ssh_resource: Option<&str>,
) -> RunnerPersistentShellOperation {
    RunnerPersistentShellOperation {
        request: PersistentShellRequest {
            action: action.to_string(),
            shell_id: shell_id.to_string(),
            workflow_session_id: format!("session-{shell_id}"),
            runtime_project_id: "agent:test:demo".to_string(),
            cwd: None,
            shell: Some("bash".to_string()),
            command: Some("printf should-not-run".to_string()),
            timeout_secs: Some(5),
            purpose: None,
        },
        job_context: Some(ShellJobContext {
            runtime_project_id: Some("agent:test:demo".to_string()),
            workflow_session_id: Some(format!("session-{shell_id}")),
            ssh_resource: ssh_resource.map(str::to_string),
            project_cwd: None,
            cwd: None,
            purpose: None,
            shell: Some("bash".to_string()),
            command_preview: String::new(),
            validation_steps: Vec::new(),
            validation: None,
            structured_execution: None,
        }),
    }
}

fn ssh_config() -> SshConfig {
    SshConfig {
        resources: [(
            "lab".to_string(),
            super::super::config::SshResourceConfig {
                host: "fixture-host".to_string(),
                default_cwd: Some("/tmp/p1c-project".to_string()),
            },
        )]
        .into_iter()
        .collect(),
    }
}

fn add_shell(
    manager: &PersistentShellManager,
    shell_id: &str,
    executor: &str,
    cwd: &Path,
) -> Arc<SpyState> {
    let state = Arc::new(SpyState::default());
    manager
        .processes
        .open_with_transport(
            ShellIdentity {
                shell_id: shell_id.to_string(),
                workflow_session_id: format!("session-{shell_id}"),
                runtime_project_id: "agent:test:demo".to_string(),
                executor: executor.to_string(),
                client_id: Some("test".to_string()),
            },
            "bash".to_string(),
            None,
            cwd.to_path_buf(),
            None,
            Box::new(SpyTransport::new(Arc::clone(&state), cwd.to_path_buf())),
        )
        .unwrap();
    state
}

fn assert_remote_deferred(result: &PersistentShellResult) {
    assert_eq!(
        result.error_code.as_deref(),
        Some("remote_durable_authority_unavailable"),
        "{result:?}"
    );
    assert_eq!(
        result.error.as_deref(),
        Some("remote durable authority backend unavailable"),
        "{result:?}"
    );
    assert!(!result.command_started, "{result:?}");
}

#[test]
fn named_remote_open_and_exec_fail_closed_and_cleanup_owned_transport() {
    let (_temp, project, projects, policy) = fixture();
    let shell = ShellConfig::default();
    let manager = PersistentShellManager::new(&shell);
    let ssh = ssh_config();

    let opened = manager.handle_operation(
        &policy,
        &shell,
        &ssh,
        1,
        &projects,
        "test",
        &operation("open", "remote-open", Some("lab")),
    );
    assert_remote_deferred(&opened);
    assert_eq!(manager.active_count(), 0, "remote open created a transport");

    let remote = add_shell(&manager, "remote-exec", "ssh", &project);
    let unrelated = add_shell(&manager, "unrelated-local", "agent", &project);
    let initial_writes = remote.writes.load(Ordering::SeqCst);
    let result = manager.handle_operation(
        &policy,
        &shell,
        &ssh,
        1,
        &projects,
        "test",
        &operation("exec", "remote-exec", Some("lab")),
    );
    assert_eq!(remote.writes.load(Ordering::SeqCst), initial_writes);
    assert_remote_deferred(&result);
    assert_eq!(remote.shutdowns.load(Ordering::SeqCst), 1);
    assert_eq!(manager.active_count(), 1);

    let repeated_close = manager.handle_operation(
        &policy,
        &shell,
        &ssh,
        1,
        &projects,
        "test",
        &operation("close", "remote-exec", Some("lab")),
    );
    assert!(repeated_close.already_closed, "{repeated_close:?}");
    assert_eq!(remote.shutdowns.load(Ordering::SeqCst), 1);

    let unrelated_initial_writes = unrelated.writes.load(Ordering::SeqCst);
    let status = manager.handle_operation(
        &policy,
        &shell,
        &ssh,
        1,
        &projects,
        "test",
        &operation("status", "unrelated-local", None),
    );
    assert_eq!(status.error_code, None, "{status:?}");
    assert_eq!(
        unrelated.writes.load(Ordering::SeqCst),
        unrelated_initial_writes
    );
    assert_eq!(unrelated.shutdowns.load(Ordering::SeqCst), 0);
}

#[test]
fn omitted_resource_marker_cannot_write_to_an_owned_ssh_transport() {
    let (_temp, project, projects, policy) = fixture();
    let shell = ShellConfig::default();
    let manager = PersistentShellManager::new(&shell);
    let remote = add_shell(&manager, "omitted-marker", "ssh", &project);
    let initial_writes = remote.writes.load(Ordering::SeqCst);

    let result = manager.handle_operation(
        &policy,
        &shell,
        &ssh_config(),
        1,
        &projects,
        "test",
        &operation("exec", "omitted-marker", None),
    );
    assert_eq!(remote.writes.load(Ordering::SeqCst), initial_writes);
    assert_remote_deferred(&result);
    assert_eq!(remote.shutdowns.load(Ordering::SeqCst), 1);
    assert_eq!(manager.active_count(), 0);
}

#[test]
fn named_remote_exec_with_wrong_session_does_not_close_owned_transport() {
    let (_temp, project, projects, policy) = fixture();
    let shell = ShellConfig::default();
    let manager = PersistentShellManager::new(&shell);
    let remote = add_shell(&manager, "wrong-session", "ssh", &project);
    let initial_writes = remote.writes.load(Ordering::SeqCst);
    let mut request = operation("exec", "wrong-session", Some("lab"));
    request.request.workflow_session_id = "another-session".to_string();

    let result = manager.handle_operation(
        &policy,
        &shell,
        &ssh_config(),
        1,
        &projects,
        "test",
        &request,
    );

    assert_remote_deferred(&result);
    assert_eq!(remote.writes.load(Ordering::SeqCst), initial_writes);
    assert_eq!(remote.shutdowns.load(Ordering::SeqCst), 0);
    assert_eq!(manager.active_count(), 1);
}

#[test]
fn private_remote_helpers_fail_closed_and_close_status_only_observe_owned_entries() {
    let (_temp, project, projects, policy) = fixture();
    let shell = ShellConfig::default();
    let manager = PersistentShellManager::new(&shell);
    let remote = add_shell(&manager, "helper-entry", "ssh", &project);
    let unrelated = add_shell(&manager, "helper-unrelated", "agent", &project);
    let remote_initial_writes = remote.writes.load(Ordering::SeqCst);
    let unrelated_initial_writes = unrelated.writes.load(Ordering::SeqCst);
    let remote_operation = operation("open", "helper-entry", Some("lab")).request;

    assert_remote_deferred(&manager.open_ssh(
        &policy,
        &ssh_config(),
        1,
        "test",
        &remote_operation,
        "lab",
        &RunnerProjectShellContext {
            id: "demo".to_string(),
            path: project.to_string_lossy().into_owned(),
            shell_profile: None,
        },
    ));
    assert_remote_deferred(&manager.exec_ssh(
        &policy,
        &ssh_config(),
        1,
        &operation("exec", "helper-entry", Some("lab")).request,
        "lab",
        &RunnerProjectShellContext {
            id: "demo".to_string(),
            path: project.to_string_lossy().into_owned(),
            shell_profile: None,
        },
    ));
    assert_eq!(remote.writes.load(Ordering::SeqCst), remote_initial_writes);

    let status = manager.handle_operation(
        &policy,
        &shell,
        &ssh_config(),
        1,
        &projects,
        "test",
        &operation("status", "helper-entry", Some("lab")),
    );
    assert_eq!(status.error_code, None, "{status:?}");
    assert_eq!(remote.writes.load(Ordering::SeqCst), remote_initial_writes);
    assert_eq!(remote.shutdowns.load(Ordering::SeqCst), 0);

    let closed = manager.handle_operation(
        &policy,
        &shell,
        &ssh_config(),
        1,
        &projects,
        "test",
        &operation("close", "helper-entry", Some("lab")),
    );
    assert_eq!(closed.shell_state, "closed", "{closed:?}");
    assert_eq!(remote.shutdowns.load(Ordering::SeqCst), 1);
    assert_eq!(
        unrelated.writes.load(Ordering::SeqCst),
        unrelated_initial_writes
    );
    assert_eq!(unrelated.shutdowns.load(Ordering::SeqCst), 0);
    assert_eq!(manager.active_count(), 1);
}

#[test]
fn runner_capabilities_do_not_advertise_remote_persistent_shell() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("runner.toml");
    std::fs::write(
        &config_path,
        format!(
            "server_url = \"http://localhost\"\ntoken = \"test-token\"\nclient_id = \"test\"\nproject_registry_dir = {}\n",
            toml::Value::String(temp.path().join("projects").to_string_lossy().into_owned())
        ),
    )
    .unwrap();
    let config = super::super::config::load_config(&config_path).unwrap();
    let capabilities = crate::runner_register_capabilities(&config);

    assert!(!capabilities.ssh_persistent_shell);
    assert!(capabilities.persistent_shell);
    assert_eq!(
        capabilities.ssh_shell,
        super::super::ssh::SshConnectionPool::is_available()
    );
}
