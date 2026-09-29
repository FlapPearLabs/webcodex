# SPAWN_INTEGRATION_NOTES.md

Stage A of the execution-broker spike: find the real execution seam in
WebCodex, and establish where `ExecutionBroker::spawn` should attach.

Baseline: `FlapPearLabs/webcodex @ eda5a659df0710b84de5becc9b3bebbb3fc51575`
(branch `spike/webcodex-execution-broker-sandbox`).

---

## 1. A correction to the research branch, stated first

The architecture study (`OSS_RESEARCH_EVIDENCE.md §2.1a`, and
`IMPLEMENTATION_PLAN.md` P1a) records:

> "≥ 10 non-test direct `Command::spawn()` sites, several model-reachable on Unix"

**That count is right in spirit and wrong in detail. On this baseline there
are 20 production sites that spawn a process outside `ManagedChild`, not
"zero".** A first pass at this measurement concluded "zero" and that conclusion
was wrong — the search used the literal token `Command::spawn`, which only
matches fully-qualified calls, and every real bypass in this tree is written as
a method call on a local binding (`command.spawn()`, `cmd.spawn()`,
`.spawn()` at the tail of a builder chain). The corrected measurement is in
§2 and it changes the normalisation plan materially.

So the record stands as: the previous study's *direction* — the spawn surface is
finite, nameable, and therefore enumerable, and a broker is worth building — is
supported. Its *count* was wrong. Both facts are kept.

The correction that matters most is the negative one. **`ManagedChild` is not
the universal chokepoint it was described as.** On Unix, several
model-reachable surfaces deliberately bypass it (`ssh.rs:1142`,
`remote_shell.rs:109`, `detached_job.rs:1924`), because they need
`Child::id()` immediately and manage process groups themselves. This is exactly
the "forgotten surface runs unconfined" failure mode the per-action broker is
supposed to eliminate, and it is present in the current code today.

I did not edit the research documents in this spike: this branch changes code,
not the prior study's claims.

---

## 2. What actually spawns processes

Measured by walking every `.rs` file under `crates/`, discarding `#[cfg(test)]`
items and files that are themselves test-only (`#[cfg(test)] #[path=...]`
modules, `tests/` dirs, `fake_*.rs` helpers). Two families remain.

### Family 1 — through `ManagedChild` (20 production sites)

`webcodex-process` owns process creation for these.
`crates/webcodex-process/src/unix.rs:43`:

```rust
pub fn spawn(command: &mut Command) -> io::Result<Self> {
    Self::spawn_with_options(command, SpawnOptions::default())
}

pub fn spawn_with_options(command: &mut Command, _options: SpawnOptions) -> io::Result<Self> {
    command.process_group(0);
    let child = command.spawn()?;
    let pgid = child.id();
    Ok(Self { child, pgid, tree_exited: AtomicBool::new(false) })
}
```

| File | Sites | Surface |
|---|---|---|
| `webcodex-runner/.../shell.rs` | 2 (1187, 3030) | `run_shell`, profile prepare — **model-reachable** |
| `webcodex-runner/.../job_manager.rs` | 3 (2981, 3246, 3439) | job execution |
| `webcodex-runner/.../detached_job.rs` | 2 (2677, 2973) | detached payload/supervisor |
| `webcodex-runner/.../ssh.rs` | 1 (1146, Windows only) | `ssh.exe` |
| `webcodex-runner/.../remote_shell.rs` | 1 (116, Windows only) | remote persistent shell |
| `webcodex-runner/.../mcp_gateway.rs` | 1 (622) | MCP provider |
| `webcodex-runner/.../plugin.rs` | 1 (1239) | plugin provider |
| `webcodex-runner/.../coding_agent.rs` | 1 (1465) | coding agent child |
| `webcodex-runner/.../external_tools.rs` | 1 (1129) | external tools |
| `webcodex-runner/.../projects/catalog.rs` | 1 (371) | project catalog |
| `webcodex-runner/.../validation/execute.rs` | 1 (53) | validation helper |
| `webcodex-runner/src/main.rs` | 1 (2299) | runner main |
| `webcodex-lsp/src/supervisor.rs` | 1 (214) | LSP server |
| `webcodex-persistent-shell/src/windows.rs` | 1 (280) | persistent shell (Windows) |
| `webcodex-browser/src/cdp.rs` | 1 (385) | browser CDP driver |
| `webcodex-process/src/execution_broker/mod.rs` | 1 (213) | **this spike** |

The runner's shell path is explicit about the ownership model
(`webcodex-runner/.../shell.rs`):

```rust
// The shell execution path owns its process tree through ManagedChild; do
// not add a process-group pre_exec here. ManagedChild creates the private
// group.
```

### Family 2 — direct `std::process::Command` spawn (18 production sites)

These do **not** go through `ManagedChild`. They are the reason the broker
cannot live only inside `ManagedChild`.

| File:line | Function | Why it bypasses | Model-reachable |
|---|---|---|---|
| `webcodex-runner/.../ssh.rs:1142` | `spawn_piped_ssh_child` | `#[cfg(unix)]` arm; needs `Child::id()` | yes (remote sessions) |
| `webcodex-runner/.../remote_shell.rs:109` | `spawn` | `#[cfg(unix)]` arm | yes |
| `webcodex-runner/.../detached_job.rs:1924` | `handoff_first_platform` | supervisor handoff | yes |
| `webcodex-runner/.../detached_job.rs:1941` | `handoff_first_platform` | Windows breakaway fallback | yes |
| `webcodex-runner/.../detached_job.rs:2409` | `run_accepted_payload` | payload needs `process_group` | yes |
| `webcodex-workspace/src/workspace_checkpoint.rs:358` | `git_output` | needs piped stdin/stdout | yes (checkpoints) |
| `webcodex-workspace/src/workspace_checkpoint.rs:398` | `git_apply` | needs piped stdin | yes (**applies patches**) |
| `webcodex-workspace/src/project_context.rs:598` | `bounded_git_output` | bounded reader | yes |
| `webcodex-persistent-shell/src/lib.rs:1965` | `spawn_shell_process` | uses `SpawnedChildGuard` | yes |
| `webcodex-environment/src/process.rs:42` | `output_with_limits` | self-managed `process_group(0)` | no (self-update) |
| `webcodex-environment/src/installer_unix.rs:442` | `spawn_owner_child` | privilege drop | no |
| `webcodex-environment/src/upgrade.rs:2563` | `verify_candidate_executables` | verification | no |
| `webcodex-environment/src/unified_update/installer.rs:491` | `apply_verified_installer` | installer | no |
| `webcodex-cli/.../connect/process.rs:568` | (chain) | CLI tunnel | no (control plane) |
| `webcodex-cli/.../connect/process.rs:655` | `start_runner` | CLI handoff | no (control plane) |
| `webcodex-cli/.../controller.rs:767` | (chain) | CLI | no (control plane) |
| `webcodex-process/src/unix.rs:57` | `ManagedChild::spawn_with_options` | the chokepoint itself | n/a |
| `webcodex-process/src/windows.rs:168` | `ManagedChild::spawn_with_options` | the chokepoint itself | n/a |

**Consequence for the design.** `ManagedChild` is a *partial* chokepoint: on
Unix it covers the shell path but not SSH, not the persistent shell, not the
workspace git helpers. A broker placed only inside `ManagedChild` would confine
`run_shell` and silently leave `git apply` and `ssh` unconfined. Any real
implementation must route **both** families through the broker. That is the
single most important finding of Stage A.

---

## 3. The real model-triggered call path

Traced from the MCP surface down to the spawn:

```
ChatGPT Web
  -> MCP tool call  "run_shell"
  -> src/tool_runtime/dispatch.rs:1717   check_runtime_tool_scope(auth, "run_shell")
  -> src/tool_runtime/dispatch.rs:1733   ToolCall::from_tool_name("run_shell", arguments)
  -> (authority decision, once, server-side, pre-mutation)
  -> RunnerOperation::RunShell(RunnerShellOperation)
       defined at crates/webcodex-core/src/runner_operation.rs:632
       wire_kind() == "run_shell"  (runner_operation.rs:661)
  -> crates/webcodex-runner/src/webcodex_runner/dispatch.rs:594
       RunnerOperation::RunShell(operation) => { ... }
  -> crates/webcodex-runner/src/webcodex_runner/shell.rs:323/339
       let mut cmd = Command::new(program);
  -> ManagedChild::spawn / spawn_with_options
       crates/webcodex-process/src/unix.rs:43
  -> std::process::Command::spawn
```

**`ManagedChild::spawn_with_options` is a chokepoint on this path, but not a
global one.** The evidence for calling it a chokepoint *here*:

1. It is one function in one crate, reached from the shell, job, detached-job,
   SSH, plugin, and LSP surfaces alike.
2. It already takes a `SpawnOptions` parameter, so it has an extension point
   that does not change existing spawn semantics.
3. The code comments state the ownership model explicitly, so a broker at this
   point is compatible with existing tree-termination behaviour.

And the limit of that claim, measured in §2: 18 production sites do not pass
through it.

---

## 4. Where `ExecutionBroker::spawn` attaches

Two candidate attachment points, and why the second is the one used.

### Rejected: at the `Command::new` call sites

Each surface builds its own `Command` and some apply their own `pre_exec`
(`shell.rs` and `job_manager` both deliberately do *not*, deferring to
`ManagedChild`). A broker at the call site would need to be threaded through
each of them, and any surface that forgot would silently run unconfined —
precisely the failure mode that makes the runner-level UNION design dangerous.

### Chosen: a `SandboxPlan`-carrying spawn in front of both families

```rust
// crates/webcodex-process/src/execution_broker/mod.rs
pub fn spawn(&self, command: &mut Command, cwd: &Path, plan: &SandboxPlan)
    -> Result<ManagedChild, BrokerError>
```

Implementation on macOS: build the SBPL string, then `exec` the command under
`/usr/bin/sandbox-exec -p <profile> <program> <args>`, and hand the launcher to
`ManagedChild::spawn_with_options` as usual. Consequences:

- the result is a `ManagedChild`, so `wait`, `try_wait`, `terminate_tree`, and
  process-group ownership are unchanged;
- `sandbox-exec` applies its profile in the child **before `exec`**, and the
  profile is inherited by every descendant — this is the inheritance property
  Test C targets;
- the broker is a zero-sized value, so the process that calls it (the control
  plane) is never itself placed inside a profile.

In this spike the broker is called from exactly one place — its own tests — so
that its behaviour can be observed without touching production behaviour. The
attachment point demonstrated here is the seam a future implementation would
use; §2's Family-2 table is the work list for actually routing all of them.

`SpawnOptions` is deliberately left as the extension point for a future
"a plan is mandatory" refusal, the way the research plan's P1a(1b) describes. It
is **not** implemented in this spike.

---

## 5. What the spike does NOT cover

No production code calls the broker:

```bash
$ grep -rn "ExecutionBroker" --include=*.rs src crates | grep -v execution_broker/
(no matches — the module is defined, exported, and tested, but unwired)
```

And per §2, a broker living only inside `ManagedChild` would still leave 16
model-reachable or otherwise interesting spawn sites unconfined. The
normalisation table is in
`research/spikes/EXECUTION_BROKER_SPIKE_RESULTS.md`.
