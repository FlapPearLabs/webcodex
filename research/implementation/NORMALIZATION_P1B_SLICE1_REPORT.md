# NORMALIZATION_P1B_SLICE1_REPORT.md

> **当前权威入口（P1B最终结构分类）：** 此文保留原baseline的历史计划、库存及验收记录。当前逐项分类、原语与逻辑计数、触发来源及fingerprint以 [launch-inventory.json](p1b/launch-inventory.json) 为唯一真源；可读说明见 [launch-inventory.md](p1b/launch-inventory.md)。新清单明确区分模型业务权限与模型触发固定探测；本页旧口径不作为当前完整库存。分类接受与最终guard / native / exact-SHA验收状态分别记载。

**ROLE** = `WEBCODEX_P1B_MODEL_REACHABLE_NORMALIZATION_SLICE_1`
**MODE** = `SMALL_PRODUCTION_SLICE` · `EVIDENCE_FIRST` · `FAIL_CLOSED` · `NO_ARCHITECTURE_REWRITE` · `NO_APPROVAL`
**Branch** = `impl/webcodex-p1b-normalization-slice1`
**Base HEAD** = `40a18b2bdfd0207406f4cbfe2f7ff92c67c82240`
**Commit** = `impl: route project overview through execution broker`

This report contains **only** the sections the task author requested. No policy recommendation,
no scope expansion, no Slice-2/approval work.

---

## PROJECT_OVERVIEW_BEFORE

`crates/webcodex-workspace/src/project_overview.rs:360` performed a bare,
unconfined git execution on a model-reachable path:

```rust
let output = std::process::Command::new("git")
    .args(["ls-files", "-z"])
    .current_dir(root)
    .output()
    .ok()?;
```

Chain to the model surface (all steps verified):

```
ToolCall::ProjectOverview            tool_call.rs (model-visible enum)
  → src/tool_runtime/dispatch.rs     enqueued as op "project_overview"
  → runner files.rs:87              RunnerFileOperation::ProjectOverview
  → handle_project_overview_request → build_project_overview
  → project_overview.rs:141         git_tracked_index(canonical_root)   [default call shape, path empty]
  → project_overview.rs:360         BARE Command::new("git")
```

Authority defects (vs the brokered baseline in `git_broker.rs`):

- No `SandboxPlan`, no Seatbelt profile → child unconfined.
- No `EnvPolicy::Minimal` → inherits full caller env including `$HOME`; loses
  `HOME=<root>`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`.
- No byte budget / deadline → unbounded stdout collection.
- No `ManagedChild` → no process-group ownership / `terminate_tree`.
- No `check_cwd`; network unrestricted; bare `"git"` resolved via inherited `PATH`.

The model cannot control argv (`["ls-files","-z"]` is literal), only *whether* it runs;
the cwd is canonicalized and asserted `starts_with(root)`.

---

## PROJECT_OVERVIEW_AFTER

`project_overview.rs:396` now routes through the broker:

```rust
let capture = crate::git_broker::run_git_bounded(
    root,
    &["ls-files", "-z"],
    PROJECT_OVERVIEW_GIT_INDEX_BYTES,            // 8 MiB budget, owned by the caller
    std::time::Instant::now() + PROJECT_OVERVIEW_GIT_INDEX_TIMEOUT, // 5 s deadline
)
.ok()?;

if !capture.status.success() || !capture.complete {
    return None;   // fail-closed; falls back to the plain filesystem walk
}
```

Semantics carried by the broker:

- `SandboxPlan::Confined` + `NetworkPolicy::Deny` + `EnvPolicy::Minimal`
  (`HOME=<root>`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`).
- Fail-closed: broker refusal / invalid root / profile-or-launch failure / timeout /
  truncation / non-zero git status → `None` → existing filesystem walk.
- **No retry. No path from here to an unconfined `Command`.**
- `git_broker.rs` gained `brokered_git_spec()` (spec extraction) so the authority and
  environment of a brokered git can be asserted without launching a process; the
  `git apply` stdin path (`Piped` when input present) is preserved and tested.

New tests (all passing): `project_overview_has_no_direct_process_launch_in_production_code`,
`project_overview_routes_its_git_through_the_broker`,
`project_overview_index_budget_is_bounded_and_ordered`.

---

## INTO_COMMAND_PRODUCTION_CALLERS

`CommandBlueprint::into_command()` (`local_execution.rs:170`) is the documented control-plane
escape hatch out of the broker. F2 closed the guard gap: `.into_command()` and `.spawn()` are
now pinned needles, and every production caller is enumerated and allowlisted.

Exhaustive production callers (verified by static scan of `crates/webcodex-runner/src`,
test regions excluded via brace-matched `#[cfg(test)]` spans):

| File | Function | Classification |
|---|---|---|
| `webcodex_runner/shell.rs` | `configured_script_runtime_plan` | `CONTROL_PLANE_FIXED_PROBE` — `node --version`, program from config/profile, argv is a Runner-authored constant |
| `main.rs` | `validation_module_available` | `CONTROL_PLANE_FIXED_PROBE` — `python -I -c <constant PROBE> <module>`, probe body is a Rust constant, module from a configured validation step |

Both are fixed probes; neither is model-reachable. `into_command()` was **deliberately not
deleted** — it remains the control-plane escape hatch, now enumerated rather than assumed.

New tests (all passing): `p1b_into_command_production_callers_are_enumerated`,
`p1b_allowed_into_command_callers_are_fixed_control_plane_probes`,
`p1b_into_command_remains_available_for_control_plane_probes`,
`p1b_routed_paths_forbid_into_command_and_direct_spawn`. Standalone harness
(`/tmp/p1b_guard_harness.rs`) reproduces the guard and includes a negative control that
**catches** a smuggled `.into_command()` injected into a routed function — confirming the
guard is not a false-green.

---

## STRUCTURAL_GUARD_CHANGE

The anti-bypass guard in `normalization_p1_tests.rs` gained:

- Two needles: `BypassPattern { needle: ".into_command()" }` and
  `BypassPattern { needle: ".spawn()" }`, each with a why-string stating the broker owns
  process creation and a routed path must never reach them.
- `P1B_ALLOWED_INTO_COMMAND_CALLERS` — an exhaustive production allowlist with a count
  assertion, so a third production caller cannot appear without a deliberate list update.
- `p1b_routed_paths_forbid_into_command_and_direct_spawn` rejects `.into_command()` and
  `.spawn()` inside any function the P1 guard already certifies as routed.

`git_broker.rs` gained `brokered_git_spec()`, which is a **structural source guard** (not a
repository-wide grep ban): it makes the brokered git's `SpawnSpec` — program, plan, cwd, and
environment — directly assertable, including on a host whose kernel refuses the profile
(`ENV_BLOCKED`), because the spec is fully determined before any launch.

---

## MODEL_REACHABLE_TOTAL

```
MODEL_REACHABLE_TOTAL = 15
```

Unchanged by Slice 1. Count basis: distinct launch sites across `crates/` + `src/` + `apps/`,
test regions excluded via brace-matched `#[cfg(test)]` spans. (See §REMAINING_BYPASSES for the
correction to a prior internal count where table rows did not sum to the headline.)

---

## MODEL_REACHABLE_ROUTED

```
MODEL_REACHABLE_ROUTED = 5
```

| Site | How routed |
|---|---|
| `runner/shell.rs:3153` `execute_configured_command` | P1 — `spawn_local_action` → `ExecutionBroker` |
| `runner/job_manager.rs:2999` Shell Job step 1 | P1 — as above |
| `runner/job_manager.rs:3266` validation step | P1 — as above + `sanitize_snapshot` |
| `runner/validation/execute.rs:101` `run_bounded` | P1 — routed; fail-closed on refusal |
| `workspace/project_overview.rs:396` `git_tracked_index` | **Slice 1** — `git_broker::run_git_bounded` |

(Up from 4: `project_overview` moved routed.)

---

## MODEL_REACHABLE_BYPASS

```
MODEL_REACHABLE_BYPASS = 10
  of which STRUCTURALLY_INCOMPATIBLE = 5   (SSH family + detached job family)
  of which REAL_GAP                   = 5
```

| ID | File : line | Function | Launch primitive | Model-facing surface | Dispatch | Authority source | Environment behaviour | Network | Filesystem | Difficulty | Recommended slice |
|---|---|---|---|---|---|---|---|---|---|---|---|
| B1 | `projects/catalog.rs:361` (+`:371`) | `run_git_bounded_with_program` → `run_git_capture` | `Command::new("git")` + `ManagedChild::spawn` | `ListProjects` (and every project-inventory push) | `ToolCall::ListProjects` → `transport/project_inventory.rs:231` → `catalog.rs:739` → `:694` **`include_git=true` (hardcoded)** → `:565/:570/:575` | hardcoded `"git"`, argv Runner-authored literals | **inherits full runner env, no `env_clear()`** (F3) | inherited | inherited, cwd=root | moderate | Slice 2 |
| B2 | `shell.rs:1532` | `capture_profile_env_snapshot` | `Command::new(program)` + `run_prepare_command`→`ManagedChild::spawn`(`:1325`) | `RunShell`/`OpenSessionShell` profile init | model tool → `RunnerOperation::RunShell`/`PersistentShell` | config `profile.program` + **config** `init_script`; model picks neither | `env_clear()` then `initial_env` re-inserted key-by-key | inherited | inherited, cwd=prepared | moderate | Slice 2 |
| B3 | `shell.rs:1729` (`impl PreparedExecutionEnvironment`, fn `:1661`) | `PreparedShellProfile::native_command` | `Command::new(native)` (returns `Command`) | `PluginTool` execution | `ToolCall::PluginTool` → `RunnerOperation::PluginGateway` → `plugin.rs:1239` | resolved program from config `plugins[].command` | `apply_env_snapshot` | inherited | inherited, cwd set | moderate | Slice 2 |
| B4 | `webcodex-lsp/src/supervisor.rs:203` (+`:214`) | `LspCommand::spawn` | `Command::new(&self.program)` + `ManagedChild::spawn` | `LspStatus`/`DocumentSymbols`/… | model LSP tool → `RunnerOperation::Lsp` → `lsp/adapter.rs` | config `commands[kind]` → `WEBCODEX_LSP_*` → PATH; argv not model-controlled | inherited | inherited | inherited | moderate | Slice 2 |
| B5 | `webcodex-persistent-shell/src/lib.rs:1924` (unix) | `spawn_shell_process` | `Command::new(&launch.program)` + `.spawn()`(`:1965`) | `OpenSessionShell`/`SessionShellExec` | model tool → `RunnerOperation::PersistentShell` | model may pick `sh` vs `bash` **only**; args/env from `base_shell_env` | `base_shell_env`, not positive-allowlist | inherited | inherited | hard | P1C |
| B6 | `webcodex-persistent-shell/src/windows.rs:269` | `spawn_shell_process` (windows) | `Command::new(&launch.program)` + `ManagedChild::spawn`(`:280`) | as B5 | as B5 | as B5 (PowerShell) | as B5 | inherited | inherited | hard | P1C |
| B7 | `detached_job.rs:2391` (unix) | `run_accepted_payload` | `Command::new(&launch.process.executable)` + `.spawn()`(`:2409`) | `RunDetachedProcess` | `ToolCall::RunDetachedProcess` → `RunnerOperation::Job(StartDetachedProcess)` | **model argv verbatim**; only argv/cwd/stdin/env length caps | `env_clear()` then `launch.env` re-inserted, **no allowlist** | inherited | inherited | hard | P1C |
| B8 | `detached_job.rs:2677` (windows; ctor `:2652`) | payload spawn (windows) | `ManagedChild::spawn` | as B7 | as B7 | as B7 | as B7 | inherited | inherited | hard | P1C |
| B9 | `ssh.rs:1142`/`:1146` | `spawn_piped_ssh_child` | `.spawn()`(unix)/`ManagedChild::spawn`(windows) | `SshResource` | `ToolCall::SshResource` → `RunnerOperation::SshResource` | local `ssh`/`ssh.exe` (hardcoded, trusted); **remote command string model-controllable** | inherited | inherited + remote | inherited | hard | P1C |
| B10 | `remote_shell.rs:109`/`:116` | `command.spawn()`/`ManagedChild::spawn` | `ssh` client | `RunShell` remote mode | model tool → remote-shell path | as B9 | inherited | inherited + remote | inherited | hard | P1C |

Sibling in the SSH family counted inside B9/B10 (same trusted local `ssh`):
`job_manager.rs:3459` `ManagedChild::spawn` (SSH client via a Job).

---

## REMAINING_BYPASSES

The 10 rows in §MODEL_REACHABLE_BYPASS are the remaining model-reachable bypasses after Slice 1.
Each carries `surface → dispatch → function → spawn` evidence above.

Sites deliberately **not** counted in the 10:

- `main.rs:2297` / `shell.rs:1039` — `into_command()` control-plane probes, allowlisted (F2).
- `process_command.rs:37`/`:42` — constructors; `:42` not reached in production.
- `coding_agent.rs:1455` — `CodingAgentStart`, user-initiated, out of Slice-1 scope.
- `local_execution.rs:172` — the broker escape hatch itself (authority, not a bypass).
- `process/*`, `execution_broker/mod.rs` — authority implementations.
- `detached_job.rs:1924`/`:1941`/`:2973`/`:3165` — internal supervisor/watchdog daemonization.
- `shell.rs:1325` — `ManagedChild` executor for B2 (not a separate surface).
- `external_tools.rs:1121` / `mcp_gateway.rs:605` — MCP provider (config-driven, out of scope).
- `plugin.rs:1239` — counted once, as B3.
- `ssh.rs:259`/`:274`/`:514`/`:593`/`:1006` — SSH capability probes / ControlMaster constructors.

---

## P1C_REQUIRED

```
P1C_REQUIRED = YES
```

The 5 structurally-incompatible sites (B5, B6, B7, B8, B9/B10) cannot be migrated to the local
Seatbelt broker: `setsid`/`CREATE_BREAKAWAY_FROM_JOB` escape process ownership, and the SSH
family's remote semantics cannot be expressed in a local profile. `ENV_BLOCKED` on this host
(`sandbox_apply: Operation not permitted` for any `(deny …)` rule) makes a broker-fidelity
PASS unreachable here; it is recorded as `ENV_BLOCKED`, **never** as a pass.

```
CATALOG_ENV_HARDENING = FOLLOW_UP
```

F3 (`catalog.rs:361`, B1) is **proven model-reachable via `ListProjects`** (overturning the
prior "internal-only" classification). It was **not** fixed in Slice 1 — the three authorised
tasks were F1, F2, and re-measurement. It is the recommended **Slice 2** target. Proof:

```
ToolCall::ListProjects
  → transport/project_inventory.rs:231  polling_projects_for_poll
      → project_cache.get_with_shutdown
  → catalog.rs:739  load_runner_project_summaries
  → catalog.rs:694  runner_project_summary_with_shutdown(&p, updated_at, true, shutdown)  // include_git HARDCODED true
  → catalog.rs:565/570/575  run_git_capture(... "rev-parse"/"log"/"status" ...)
  → catalog.rs:361  Command::new("git")  // no env_clear(), inherits full runner env
```

`runner_project_summary()` at `:608` is `#[cfg(test)]`, but the production path is `:694`, which
passes `true` unconditionally. `load_runner_project_summaries_from_dir` is additionally reached
from `transport.rs:269`, `transport/project_inventory.rs:231`, `lsp/adapter.rs:86` (every model
LSP tool call), and `validation/mod.rs:182`.
