# SPAWN_SURFACE_INVENTORY.md

**ROLE** = `WEBCODEX_EXECUTION_NORMALIZATION_ARCHITECT`
**TASK** = `P1B PREPARATION ONLY`
**MODE** = `INVENTORY / NO IMPLEMENTATION`
**Date** = 2026-10-02 (prepared) · **2026-10-02 updated by P1B Slice 1** (incremental, additive)
**Baseline** = branch `fix/webcodex-execution-normalization-p1-closure` @ `40a18b2bdfd0207406f4cbfe2f7ff92c67c82240`
**Production code modified (at preparation time)** = `NONE`
**Production code modified (P1B Slice 1)** = `project_overview.rs` (F1 fix) + `git_broker.rs` (spec extraction) + `normalization_p1_tests.rs` (F2 guard)
**Implementation / refactor / approval system** = `NONE`

> **Revision note (P1B Slice 1).** Sections 1.1, 8.1–8.3, and the F1/F2 status lines are
> updated in place to record the post-Slice-1 state. Every other section is preserved
> verbatim as prepared. Section 1.4 is new: it records the Slice-1 re-measurement,
> including a correction to the prepared headline (the prepared §1.2/§1.3 row counts did
> not sum to the prepared headline; §1.4 gives the reconciled, source-verified totals).

---

## 0. VERDICT

```
P1B_PREPARATION_STATUS   = COMPLETE
P1B_SLICE_1_STATUS       = COMPLETE (A=F1 routed, B=F2 guard closed, C=re-measured)
PRODUCTION_CODE_MODIFIED = YES (F1 routing only; no refactor, no architecture change)
INVENTORY_SCOPE          = FULL_REPO (crates/ + src/ + apps/desktop + build.rs)

MODEL_REACHABLE_LAUNCH_SITES_TOTAL           = 15   (unchanged by Slice 1)
MODEL_REACHABLE_ROUTED_VIA_EXECUTION_BROKER  = 5    (was 4; +project_overview)
MODEL_REACHABLE_BYPASS                       = 10   (was 11)
  of which STRUCTURALLY_INCOMPATIBLE          = 5    (SSH family + detached job family)
  of which REAL_GAP                           = 5    (was 6; -project_overview)

PRIORITY_0_FINDINGS = 3
  F1  project_overview.rs:360 — last unconfined model-reachable git execution
      -> STATUS: FIXED in Slice 1 (routed through git_broker; brokered; fail-closed)
  F2  BYPASS_PATTERNS does not pin into_command() — anti-bypass invariant is convention, not mechanism
      -> STATUS: FIXED in Slice 1 (into_command() + .spawn() pinned; exhaustive caller allowlist)
  F3  catalog.rs:361 run_git_bounded — no env_clear(), inherits full runner environment
      -> STATUS: RE-MEASURED. Model-reachable via ListProjects (see §1.4).
                 NOT fixed in Slice 1 (out of the three authorised tasks) -> CATALOG_ENV_HARDENING = FOLLOW_UP
```

### 0.1 Headline correction to the prior working hypothesis

The previously held belief was that `webcodex-workspace/src/project_context.rs` holds a
model-reachable unrouted git execution. **That belief is REFUTED.**

`project_context.rs` production code calls `git_broker::run_git_bounded` (`:610`) and is therefore
**already routed**. Its three grep hits are one doc comment plus two calls inside
`#[cfg(test)] mod tests` (starts `:1101`).

The **actual** remaining model-reachable bypass is `project_overview.rs:360`.

For contrast, the sibling file `git_broker.rs` has **zero** production `Command::new` — its single
hit is a `git init` fixture inside `#[cfg(test)] mod tests` (starts `:442`). It is the *caller* of
`ExecutionBroker`, not a spawner. `workspace_checkpoint.rs` is likewise already routed; its hits are
test fixtures (`:909`, `:914`).

### 0.2 Evidence base

| Item | Value |
|---|---|
| Repo | `/Users/songshiyao/Desktop/Projects/webcodex` |
| Branch / SHA | `fix/webcodex-execution-normalization-p1-closure` @ `40a18b2b` |
| Workspace members | 19 crates |
| `.rs` files (excl. `target/`) | 1009 |
| Rust lines | ~779,609 |
| Other | py 76 / ts 89 / js 18 / sh 29 / toml 23 |
| Patterns searched | `Command::new` · `.spawn()` · `tokio::process` · `std::process::Command` · `posix_spawn` · `libc::exec` · `execl` · `setsid` · `daemonize` |

**Explicit negative results** (searched, not found):

```
posix_spawn      → NOT FOUND anywhere
libc::exec/execl → NOT FOUND anywhere
daemonize()      → NOT FOUND anywhere
setsid           → FOUND only via pre_exec in detached_job.rs and the CLI log-writer
tokio::process   → FOUND, 9 production sites, all in the main server crate (src/);
                    every one awaited or supervised, none detached/daemonized
```

### 0.3 How to read the tables

**Reachability** — exactly one per row:

| Class | Meaning |
|---|---|
| `model reachable` | a model tool call can cause this execution |
| `user initiated` | only a human running a CLI subcommand or clicking in the desktop app |
| `internal only` | bootstrap, self-update, service install, tunnel bootstrap, first-run wiring |
| `test only` | `#[cfg(test)]`, `tests/`, `fake_*`, `test_support`, or a fixture binary |

**Broker** column:

| Value | Meaning |
|---|---|
| `ROUTED` | goes through `spawn_local_action` → `ExecutionBroker` |
| `BYPASS (gap)` | should be confined and is not — **this is the P1B work list** |
| `BYPASS (justified)` | unsandboxed, but a documented or structural exemption |
| `BYPASS (exempt)` | architecturally outside a local execution broker's remit |
| `AUTHORITY` | this *is* the broker or its process-ownership primitive |
| `n/a` | build-time, or `exec()` which replaces the process |

**Migration** — `trivial` / `moderate` / `hard`, each with a stated reason.

---

## 1. Model-reachable surfaces — the P1B core work list

15 model-reachable launch sites. **5 routed** (was 4), **10 bypass** (was 11).

### 1.1 ROUTED (5) — P1 closure (4) + P1B Slice 1 (1)

| # | Site | Program / argv | Current authority path | Broker | Migration |
|---|---|---|---|---|---|
| M1 | `runner/shell.rs:3153` `execute_configured_command` | configured shell program + **model command text as a single argv element** | config program (PATH lookup) + model text. Model may choose command, args and cwd; it can **never** choose workspace root, toolchain root, or network reach | **ROUTED** — `spawn_local_action` → `ExecutionBroker::spawn_with_toolchain`; Seatbelt profile + `check_cwd` + `EnvPolicy::Minimal`; fail-closed | done (P1) |
| M2 | `runner/job_manager.rs:2999` Shell Job first step | as M1 | as M1 | **ROUTED** | done (P1) |
| M3 | `runner/job_manager.rs:3266` each validation step | as M1 | as M1 | **ROUTED** — step env merged, then `sanitize_snapshot` allowlist | done (P1) |
| M4 | `runner/validation/execute.rs:101` `run_bounded` | pyright / node / python interpreter + fixed CLI shape | config program (absolute-path validated) + fixed args; env = `approved_inherited_env` allowlist | **ROUTED** — on refusal returns `spawn_error` and **does not fall back to a bare spawn** | done (P1) |
| **M4b** | **`workspace/project_overview.rs:396`** `git_tracked_index` | **`git ls-files -z` — literal argv, nothing model-interpolated** | **workspace root only (canonicalized + `starts_with` asserted at `:113-124`); model controls only *whether* it runs** | **ROUTED (Slice 1)** — `git_broker::run_git_bounded` → `SandboxPlan::Confined` + `NetworkPolicy::Deny` + `EnvPolicy::Minimal` (`HOME=<root>`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0`); byte budget 8 MiB + 5 s deadline; **fail-closed, no unconfined retry** | **done (Slice 1)** |

### 1.2 BYPASS — real gaps (7 rows / 5 sites after Slice 1)

> Slice 1 removed `project_overview.rs:360` (F1) from this table. The row count here is 7 while the
> reconciled site count is 5 — M8/M8b are one site split per platform, and the two
> config-driven sites (`mcp_gateway.rs:605`, `plugin.rs:1235`) are prose-only here because they
> are de-duplicated into B3 and the MCP exclusion in §1.4.2. **§1.4 is authoritative for counting.**

| # | Site | Program / argv | Current authority path | Broker | Migration |
|---|---|---|---|---|---|
| M5 | `runner/shell.rs:1532` `capture_profile_env_snapshot` | profile program + profile `init_script` path | config `profile.program` (PATH lookup) + **config** `init_script` — *not* model | **BYPASS (gap)** | `moderate` — snapshot semantics need env parity with a real shell; the broker's `Minimal` floor would change the captured result |
| M6 | `runner/shell.rs:1729` `PreparedShellProfile::native_command` (Tool Plugin launcher) | resolved inside prepared PATH | config `plugins[].command` | **BYPASS (gap)** | `moderate` — plugin has no workspace-authority anchor today |
| M7 | `runner/lsp/supervisor.rs:203` `LspCommand::spawn` | config `commands[kind]` → `WEBCODEX_LSP_*` env override → PATH profile default | config + env; **argv is not model-controlled** | **BYPASS (gap)** | `moderate` — `canonical_project_root` is already available as broker authority input |
| M8 | `runner/persistent-shell/lib.rs:1924` `spawn_shell_process` (unix) | `sh` / `bash` from a 2-element allowlist | model may pick `sh` vs `bash` **only**; args + env from `base_shell_env`; `initialization` = config `init_script` | **BYPASS (gap)** | `hard` — `pre_exec` dup2(FD7/8) FD-passing is incompatible with the broker's launcher re-exec |
| M8b | `persistent-shell/windows.rs:269` | as M8; must be PowerShell | as M8 | **BYPASS (gap)** | `hard` — same |
| M9 | `runner/external_tools.rs:1121` `McpConnection::spawn` | config `mcp_gateway.providers[].executable` (absolute-path enforced) | config; **not** model-controlled | **BYPASS (gap)** | `moderate` |

Also model-reachable, same family, config-driven: `runner/mcp_gateway.rs:605` and
`runner/plugin.rs:1235` `prepare_provider` — both `BYPASS`, `moderate`.

### 1.3 BYPASS — structurally incompatible with the broker (5)

These are **not** "pending migration". Their blocker is not effort; it is architectural.

| # | Site | Program / argv | Current authority path | Broker | Why blocked |
|---|---|---|---|---|---|
| M10 | `runner/detached_job.rs:2391` `run_accepted_payload` (unix) | **model argv verbatim** (`ShellProcessArgv`) | only `validate_process_argv` + cwd/stdin/env length caps; env is `env_clear()` then `launch.env` re-inserted with **no allowlist** | BYPASS | `setsid` escapes the process group → see §5.1 |
| M10b | `detached_job.rs:2652` (windows) | as M10 | as M10 | BYPASS (Job Object only; no Seatbelt, no `check_cwd`, no env allowlist) | `CREATE_BREAKAWAY_FROM_JOB` → §5.1 |
| M11 | `runner/ssh.rs:1139` `spawn_piped_ssh_child` (unix) | `ssh` (trusted) local; **remote command string model-controllable** | remote text via `shell_quote`/`remote_script`, or stdin on Windows | BYPASS | remote semantics cannot be expressed in a local Seatbelt profile |
| M11b | `runner/job_manager.rs:3459` ssh client via `ManagedChild::spawn` | `ssh` | as M11 | BYPASS | as M11 |
| M11c | `runner/remote_shell.rs:109` / `:116` `command.spawn()` | `ssh` | as M11 | BYPASS | as M11; P1 scope statement explicitly excludes SSH and remote_shell |

Constructors feeding the SSH family (no spawn of their own): `ssh.rs:514`
`prepare_persistent_shell_command`, `ssh.rs:593` `direct_ssh_command`, `ssh.rs:1006`
`ssh_command_with_config`.

`ssh.rs` shows 40 grep hits. The split: **10 unique production spawn points**, all local `ssh` /
`ssh.exe` clients with a hardcoded executable name; the remaining ~30 hits are inside
`#[cfg(all(test, unix))] mod tests` (from `:1475`) and `#[cfg(all(test, windows))] mod windows_tests`
(from `:3414`). **Model-controllable content is the remote command text, never the local argv.**

### 1.4 Slice-1 re-measurement — the exact remaining 10 (source-verified)

This section is the **Task C** deliverable. It is a re-measurement of the *current tree*, not a
restatement of §1.2/§1.3. Every row carries `surface → dispatch → function → spawn` evidence.

#### 1.4.0 Correction to the prepared counts

The prepared headline said `MODEL_REACHABLE_TOTAL = 15`, `BYPASS = 11`, split `6 + 5`. The prepared
**rows did not sum to that**: §1.2 listed 8 rows (M5, M6, M7, M8, M8b, M9, + two prose-only
config-driven sites) and §1.3 listed 5 rows (M10, M10b, M11, M11b, M11c) = 13, not 11.
The discrepancy is a **bookkeeping defect in the prepared inventory, not in the code**.

Re-measurement resolves it by counting **distinct launch sites**, not table rows (several rows are
the same site split per platform, and two sites were prose-only). Reconciled basis:

| Basis | Prepared | Reconciled (Slice 1) |
|---|---|---|
| Model-reachable **sites** | 15 | 15 (unchanged — Slice 1 moved one site from bypass to routed, it did not add or remove one) |
| — routed | 4 | **5** (+M4b `project_overview`) |
| — bypass | 11 | **10** |

**Method.** Enumerated every `Command::new` / `ManagedChild::spawn` / `.spawn()` / `.into_command()`
/ `tokio::process::Command::new` across `crates/`, `src/`, `apps/`, then excluded test regions using
**brace-matched `#[cfg(...test...)]` item spans** plus test-file naming. This matters: a naive
"first `#[cfg(test)]` line" rule truncates `shell.rs` at line 111 (an out-of-line
`mod desktop_mcp_env_tests;` declaration) and silently drops ~2000 production lines.
Runner production total after correct filtering: **46 raw launch sites → 11 distinct model-reachable
programs** (constructors, duplicates and control-plane probes de-duplicated).

#### 1.4.1 The 10 remaining model-reachable bypasses

| ID | File : line | Function | Launch primitive | Model-facing surface | Dispatch | Authority source | Env behaviour | Network | Filesystem | Difficulty | Recommended slice |
|---|---|---|---|---|---|---|---|---|---|---|---|
| **B1** | `webcodex-runner/src/webcodex_runner/projects/catalog.rs:361` (+ spawn `:371`) | `run_git_bounded_with_program` → `run_git_capture` | `Command::new("git")` + `ManagedChild::spawn` | `ListProjects` (and every project-inventory push) | `ToolCall::ListProjects` → runner-registry inventory → `transport/project_inventory.rs:231` `project_cache.get_with_shutdown` → `catalog.rs:739` `load_runner_project_summaries` → `:694` **`include_git = true` (hardcoded)** → `:565/:570/:575` `run_git_capture` (`rev-parse --abbrev-ref HEAD`, `log -1 --pretty=format:%h`, `status --short`) | **hardcoded `"git"`** — no config/env/program override; argv is Runner-authored literals | **inherits full runner env, no `env_clear()`** — this is exactly F3 | inherited (unrestricted) | inherited, cwd = project root | `moderate` | **Slice 2 (F3)** |
| **B2** | `webcodex-runner/src/webcodex_runner/shell.rs:1532` | `capture_profile_env_snapshot` | `Command::new(program)` + `run_prepare_command` → `ManagedChild::spawn` (`shell.rs:1325`) | `RunShell` / `OpenSessionShell` (profile initialisation) | model tool call → `RunnerOperation::RunShell`/`PersistentShell` → `shell.rs` profile preparation | config `profile.program` (PATH lookup) + **config** `init_script`; model picks neither | `env_clear()` then `initial_env` re-inserted key-by-key — *better* than B1, but `initial_env` itself is caller-derived and unfiltered by a positive allowlist | inherited | inherited, cwd = prepared cwd | `moderate` | Slice 2 |
| **B3** | `webcodex-runner/src/webcodex_runner/shell.rs:1729` (inside `impl PreparedExecutionEnvironment`, fn from `:1661`) | `PreparedShellProfile::native_command` (Tool Plugin launcher) | `Command::new(native)` (returns a `Command`; spawned by callers) | `PluginTool` → plugin-provided tool execution | `ToolCall::PluginTool(PluginToolCall)` → `RunnerOperation::PluginGateway` → `plugin.rs:1239` `ManagedChild::spawn` | resolved program from config `plugins[].command`, resolved inside the prepared PATH | `apply_env_snapshot(&mut command, &self.env_snapshot)` — snapshot-derived | inherited | inherited, cwd set | `moderate` | Slice 2 |
| **B4** | `crates/webcodex-lsp/src/supervisor.rs:203` (+ spawn `:214`) | `LspCommand::spawn` | `Command::new(&self.program)` + `ManagedChild::spawn` | `LspStatus`, `DocumentSymbols`, `DocumentDiagnostics`, `Hover`, `WorkspaceSymbols`, `GotoDefinition`, `FindReferences`, `CallHierarchy` | model LSP tool call → `RunnerOperation::Lsp` → `lsp/adapter.rs` → `webcodex-lsp/src/supervisor.rs` | config `commands[kind]` → `WEBCODEX_LSP_*` env override → PATH default. **argv is not model-controlled** | inherited | inherited | inherited | `moderate` (`canonical_project_root` already available as broker input) | Slice 2 |
| **B5** | `crates/webcodex-persistent-shell/src/lib.rs:1924` (unix) | `spawn_shell_process` | `Command::new(&launch.program)` + `.spawn()` (`:1965`) | `OpenSessionShell` / `SessionShellExec` | model tool call → `RunnerOperation::PersistentShell` → `persistent_shell.rs:682` → `webcodex-persistent-shell` | model may pick `sh` vs `bash` **only** (2-element allowlist); args + env from `base_shell_env`; `initialization` = config `init_script` | `base_shell_env` constructed by caller, not a positive-allowlist minimum | inherited | inherited | `hard` — `pre_exec` dup2(FD7/8) FD-passing is incompatible with the broker launcher re-exec | **P1C** |
| **B6** | `crates/webcodex-persistent-shell/src/windows.rs:269` | `spawn_shell_process` (windows) | `Command::new(&launch.program)` + `ManagedChild::spawn` (`:280`) | as B5 | as B5 | as B5 (must be PowerShell) | as B5 | inherited | inherited | `hard` — same as B5 | **P1C** |
| **B7** | `webcodex-runner/src/webcodex_runner/detached_job.rs:2391` (unix) | `run_accepted_payload` | `Command::new(&launch.process.executable)` + `.spawn()` (`:2409`) | **`RunDetachedProcess`** | `ToolCall::RunDetachedProcess` → `RunnerOperation::Job(StartDetachedProcess)` → `detached_job.rs` | **model argv verbatim** (`ShellProcessArgv`); only `validate_process_argv` + cwd/stdin/env length caps | `env_clear()` then `launch.env` re-inserted with **no allowlist** | inherited | inherited | `hard` — `setsid` escapes the process group → §5.1 | **P1C** |
| **B8** | `webcodex-runner/src/webcodex_runner/detached_job.rs:2677` (windows; constructor at `:2652`) | payload spawn (windows) | `ManagedChild::spawn(&mut payload_command)` | as B7 | as B7 | as B7 | as B7 | inherited | inherited | `hard` — Job Object only, no Seatbelt / `check_cwd` / env allowlist; `CREATE_BREAKAWAY_FROM_JOB` | **P1C** |
| **B9** | `webcodex-runner/src/webcodex_runner/ssh.rs:1142` / `:1146` | `spawn_piped_ssh_child` | `.spawn()` (unix) / `ManagedChild::spawn` (windows) | `SshResource` | `ToolCall::SshResource(SshResourceToolCall)` → `RunnerOperation::SshResource` → `ssh.rs` | local program `ssh`/`ssh.exe` (hardcoded name, trusted); **remote command string is model-controllable** via `shell_quote`/`remote_script` (stdin on Windows) | inherited | inherited + remote network | inherited | `hard` — remote semantics cannot be expressed in a local Seatbelt profile | **P1C** |
| **B10** | `webcodex-runner/src/webcodex_runner/remote_shell.rs:109` / `:116` | `command.spawn()` / `ManagedChild::spawn` | `ssh` client | `RunShell` remote mode | model tool call → remote-shell path | as B9 | inherited | inherited + remote | inherited | `hard` — as B9; P1 scope explicitly excludes SSH / `remote_shell` | **P1C** |

**Sibling in the same SSH family, also model-reachable and structurally blocked (counted inside B9/B10
rather than as separate sites, because they launch the same trusted local `ssh` client):**
`webcodex-runner/src/webcodex_runner/job_manager.rs:3459` `ManagedChild::spawn(&mut command)` — SSH
client via a Job. Same chain, same blocker.

#### 1.4.2 Sites deliberately *not* counted in the 10

| Site | Why excluded |
|---|---|
| `webcodex-runner/src/main.rs:2297` `blueprint.into_command()` + `:2303` | `CONTROL_PLANE_FIXED_PROBE` — `python -I -c <constant PROBE> <module>`. Probe body is a Rust constant; module comes from a configured validation step. Allowlisted in the Slice-1 F2 guard. |
| `webcodex-runner/src/webcodex_runner/shell.rs:1039` `probe_blueprint.into_command()` | `CONTROL_PLANE_FIXED_PROBE` — `node --version`, constant argv, program from config/profile. Allowlisted in the Slice-1 F2 guard. |
| `webcodex-runner/src/webcodex_runner/process_command.rs:37` / `:42` | constructors; `:42` is not reached in production (only non-test caller is `detached_job.rs:2652`, `#[cfg(windows)]`) |
| `webcodex-runner/src/webcodex_runner/coding_agent.rs:1455` | `CodingAgentStart` — an ACP session is user-initiated; also explicitly out of Slice-1 scope |
| `webcodex-runner/src/webcodex_runner/local_execution.rs:172` | **this *is* the broker escape hatch** (`CommandBlueprint::into_command`) — authority, not a bypass |
| `crates/webcodex-process/src/{unix,windows}.rs`, `execution_broker/mod.rs` | authority implementations (§3.1) |
| `detached_job.rs:1924`, `:1941`, `:2973`, `:3165` | internal supervisor/watchdog daemonization via trusted `current_exe()`; not model argv |
| `shell.rs:1325` `run_prepare_command` | the ManagedChild executor for B2, not a separate surface |
| `external_tools.rs:1121` / `mcp_gateway.rs:605` | **MCP provider** — `McpGatewayRequest` is config-driven, and MCP providers are explicitly out of Slice-1 scope |
| `plugin.rs:1239` | counted once, as B3 |
| `ssh.rs:259`, `:274`, `:514`, `:593`, `:1006` | SSH capability probes + ControlMaster constructors — trusted program, config-derived host, no model-controlled argv; they feed B9 |

#### 1.4.3 F3 (catalog env) — model reachability now **proven**

The prepared inventory classified F3 as internal-only and therefore out of the P1B model-reachable
work list. Slice 1 re-measurement **overturns that classification**. Verified chain:

```
ToolCall::ListProjects                       (model-visible, tool_call.rs:1392 enum, :1673 area)
  → runner-registry project inventory
  → transport/project_inventory.rs:231  polling_projects_for_poll
      → project_cache.get_with_shutdown(cfg, Some(shutdown))
  → catalog.rs:739  load_runner_project_summaries
  → catalog.rs:694  runner_project_summary_with_shutdown(&project, updated_at, true, shutdown)
                                                  ^^^^ include_git is HARDCODED true in production
  → catalog.rs:565 / :570 / :575  run_git_capture(... "rev-parse" / "log" / "status" ...)
  → catalog.rs:361  Command::new("git")   ← no env_clear(), inherits the full runner environment
```

`runner_project_summary()` at `:608` is `#[cfg(test)]` — but that is irrelevant to reachability: the
production path is `:694`, inside `load_runner_project_summaries_from_dir_with_shutdown`, and it passes
`true` unconditionally. `load_runner_project_summaries_from_dir` is additionally called from four
model-facing or model-observable places: `transport.rs:269`, `transport/project_inventory.rs:231`,
`lsp/adapter.rs:86` (resolves the project for **every model LSP tool call**), and
`validation/mod.rs:182`.

**Correction to a prior working note.** An earlier pass in this slice recorded
`RegisterProject` / `CreateProject` as having **no** model `ToolCall` variant. That was wrong, and it
was wrong because the dispatch uses `Self::`, not `ToolCall::`, so a `ToolCall::`-prefixed search
returns zero hits and reads as "no dispatch". Both variants **do** exist in the model-visible enum
(`tool_call.rs`: `RegisterProject`, `UnregisterProject`, `CreateProject`, `ListProjects`), and
`Register`/`Create` route to `handle_project_operation` at `dispatch.rs:686`. This changes the
project-surface picture but **does not add a site to the 10**: `handle_project_operation` reaches
`projects/lifecycle.rs`, which already uses `git_broker::run_git_bounded` (routed), whereas the
*unrouted* git is `catalog.rs:361`, reached via the inventory path above.

```
CATALOG_ENV_HARDENING = FOLLOW_UP
```

Not actioned in Slice 1: the three authorised tasks were F1, F2, and re-measurement. F3 is a real,
now-proven model-reachable gap and is the recommended **Slice 2** target.

### 1.5 What Slice 1 changed in the guard, not in the architecture

| Item | Before | After |
|---|---|---|
| `BYPASS_PATTERNS` needles | `ManagedChild::spawn(`, `Command::new(` | `+ .into_command()`, `+ .spawn()` |
| `into_command()` callers | unmonitored (convention) | **exhaustive production allowlist** — `configured_script_runtime_plan`, `validation_module_available`; both must self-declare `CONTROL_PLANE_FIXED_PROBE` |
| Routed-function escape | not structurally prevented | `p1b_routed_paths_forbid_into_command_and_direct_spawn` rejects `.into_command()` and `.spawn()` inside any function the P1 guard already certifies as routed |

`into_command()` was **not** deleted — it remains the documented control-plane escape hatch, now
enumerated rather than assumed.

---

## 2. User-initiated surfaces (17)

### 2.1 Main server crate `src/` (6)

| # | Site | Program / argv | Current authority path | Broker | Migration |
|---|---|---|---|---|---|
| U1 | `src/project_entry.rs:423` | `git status --porcelain` | hardcoded argv; cwd from the `--project` CLI flag | BYPASS | `trivial` — read-only probe |
| U2 | `src/project_entry_setup.rs:774` | `git -C <root> rev-parse --show-toplevel` | hardcoded argv; `-C` is a canonicalized user path | BYPASS | `trivial` |
| U3 | `src/project_entry_cloudflared.rs:651` | `tar -xzf <archive> -C <dir>` | hardcoded; archive from managed download, SHA-256 verified | BYPASS | `hard` — external-archive extraction is a classic escalation surface; needs a path-normalization policy the broker does not yet have |
| U3b | `project_entry_cloudflared.rs:127` | `npm config get <key>` | hardcoded `npm` + PATH; **gated by `WEBCODEX_NPM_WRAPPER=1`** | BYPASS | `moderate` — PATH-hijack surface; npm integrity not verified |
| U4 | `src/project_entry_share.rs:1037` → spawn `:1047` | `cloudflared tunnel --url <loopback>` | env override → PATH → managed download with SHA-256 + version pin | BYPASS | `moderate` — already has `process_group(0)` + SHA-256 + version check |
| U5 | `src/project_entry_client_handoff.rs:114` → spawn `:126` | `pbcopy` / `wl-copy` / `xclip -selection clipboard` / `xsel --clipboard --input` / `clip.exe` | hardcoded `&'static` table + PATH; **payload goes via stdin, not argv** | BYPASS | `trivial` |
| U6 | `client_handoff.rs:152` → spawn `:159` | `sh -c 'IFS= read -r line; [ -z "$line" ]'` | hardcoded | BYPASS | `moderate` — **the only production `Stdio::inherit()`**; the child directly holds the server process's TTY |
| U6b | `client_handoff.rs:174` → spawn `:181` | `open` / `xdg-open` / `gio open` with a source-constant URL | hardcoded const | BYPASS | `trivial` |

Trigger paths: `webcodex doctor` / `webcodex status` (U1), `webcodex setup` (U2),
`webcodex share --tunnel cloudflare` (U4), `webcodex share --copy-url` (U5, U6, U6b).

### 2.2 `webcodex-environment` — privilege and service planes (8)

| # | Site | Program / argv | Current authority path | Broker | Migration |
|---|---|---|---|---|---|
| U7 | `environment/src/privilege.rs:641` | `/usr/bin/sudo` (TTY) or `/usr/bin/pkexec` (non-TTY) + `cli environment __service-operation <req> <resp>` | program hardcoded absolute; **argv array, no string concat**; `cli` from persisted `SetupRequest.binaries.cli` | BYPASS | `hard` — see §4.2 |
| U7b | `privilege.rs:665` | `/usr/bin/osascript -e <script> -- <cli> <req> <resp>` | script hardcoded literal; 3 values passed as an argv array and escaped with AppleScript `quoted form of` | BYPASS | `hard` — see §4.2 |
| U8 | `environment/src/unified_update/installer.rs:485` | `/usr/sbin/installer -pkg <p> -target /` \| `/usr/bin/dpkg --install <p>` \| `/usr/bin/rpm --upgrade <p>` | 3-way hardcoded; `<p>` inside a root-private cache, sha256-verified | BYPASS | `hard` — system package managers writing system directories |
| U9 | `environment/src/upgrade.rs:2557` | `&artifact.path --build-info-json` | **downloaded artifact**; provenance + checksum + ELF/PE/Mach-O header check + post-exec digest recheck | BYPASS | `hard` — and it should **stay** bypass; see §4.3 |
| U10 | `environment/src/installer_unix.rs:411` | `cli environment __installer-child 3 {finish\|rollback} <frozen.root>` with `pre_exec` setuid/setgid/setgroups | frozen authorized receipt; digest verified pre-spawn | BYPASS | `moderate` — already a private privilege-drop broker; a shared abstraction gains nothing |
| U11 | `environment/src/service/linux.rs:66` | `/usr/bin/systemctl` \| `/bin/systemctl` + 15+ subcommands | absolute-path probe; argv internal literals + `{id}.service` | BYPASS | `moderate` |
| U12 | `environment/src/session_service.rs:400` | `schtasks.exe /Query /Create /Run /Delete /TN <id> /XML <path>` | **PATH lookup — the only one in this crate**; `plan.id` / `plan.program` from `HelperPlan` | BYPASS | `moderate` |
| U12b | `service/linux.rs:234`/`:250`, `service/macos.rs:49`/`:74`/`:176`, `session_service.rs:858`/`:863`/`:884`/`:894` | `/usr/bin/id -u`, `/usr/bin/getent group`, `/bin/launchctl print\|bootstrap\|bootout` | hardcoded absolute paths; argv internal literals | BYPASS | `trivial` |

### 2.3 Desktop and CLI (3)

| # | Site | Program / argv | Current authority path | Broker | Migration |
|---|---|---|---|---|---|
| U13 | `desktop/.../updates/install/prepare.rs:104`/`:153`/`:228`/`:315` | `/usr/bin/rpm -qp`, `/usr/bin/rpm2cpio`, `/usr/bin/cpio --extract --make-directories --no-absolute-filenames --quiet <patterns…>`, `/usr/sbin/pkgutil --expand-full` \| `/usr/bin/dpkg-deb --control` | program from a hardcoded allowlist; patterns from the rpm inventory, ≤50k entries | BYPASS | `moderate` — see §4.4 for the residual symlink gap |
| U14 | `desktop/.../updates/install/native.rs:119` (Linux pkexec) · `:316` (macOS `AuthorizationExecuteWithPrivileges`) · `:412` (Windows) | pkexec argv is a literal 11-element `OsString` vector, no shell; macOS passes literal argv; Windows uses `creation_flags(0x200\|0x8)` with an exclusive `share_mode(1)` handle held to `CreateProcess` | `cli` verified by `verify_installed_update_cli` (root-owned, non-world-writable, exact path match) | BYPASS | `hard` — see §4.4 C3 |
| U15 | `crates/webcodex-cli/.../service.rs:40` `RealProcessExecutor::execute` | `systemctl …` or `journalctl …`; `--user` optionally prepended | program resolved by absolute-PATH-only scan, Linux-gated; argv from hardcoded unit constants, unit overridable from the service-file name | BYPASS (exempt) | `hard` — privileged service plane; sandboxing `systemctl` breaks it by design. Correct answer is a documented exemption + audit logging |

**Also user-initiated, CLI connect/controller (4):**

| Site | Program | Broker | Migration |
|---|---|---|---|
| `cli/.../connect/process.rs:548` → spawn `:568` | `current_exe() __hosted-log-writer <state_dir>` + `setsid()` | BYPASS (justified) | `trivial` — deliberate detachment; a log writer must outlive the CLI |
| `connect/process.rs:635` → spawn `:655` `start_runner` | `<runner_bin> --config <config>` + `setsid()` | BYPASS (justified) | `trivial` — deliberate, so the Runner survives CLI exit |
| `cli/.../controller.rs:891` / `:915` / `:945` via `spawn_process` | `webcodex-server --stop-on-stdin-eof`, `webcodex-runner --config <c> --stop-on-stdin-eof`, `current_exe() server tunnel --provider openai --env-file <f> --json --stop-on-stdin-eof` | BYPASS (justified) | `moderate` — a supervisor launching its own long-lived children; a sandbox would break server needs (systemd, sockets, broad FS). Recommend an explicit exemption class. Note `remove_controller_tunnel_credentials` strips `CONTROL_PLANE_*` |
| `cli/.../service.rs:1705` `run_internal_binary` | `webcodex-server` / `webcodex-runner`, then `command.exec()` | n/a | `trivial` — `execve` replaces the process; there is no authority to confine |

---

## 3. Internal-only surfaces (14)

| # | Site | Program / argv | Current authority path | Broker | Migration |
|---|---|---|---|---|---|
| I1 | `src/project_entry.rs:800` → spawn `:862` | `webcodex-server` (no argv) + ~20 injected `WEBCODEX_*` env vars | `locate_companion_binary`: exe sibling/parent dir → PATH. **Env values partly from project config files** | BYPASS | `moderate` |
| I2 | `src/project_entry.rs:884` → spawn `:897` | `webcodex-runner --config <resolved_runner_config>` | `WEBCODEX_AGENT_BIN` env override → companion binary discovery | BYPASS | `moderate` — `remove_runner_parent_credentials()` strips `WEBCODEX_TOKEN` / `PAT` / `AGENT_TOKEN` |
| I3 | `src/project_entry_openai_tunnel.rs:99` → spawn `:119` | `tunnel-client run --mcp.server-url … --mcp.extra-headers 'Authorization: file:<path>' --health.listen-addr 127.0.0.1:0 …` | `WEBCODEX_TUNNEL_CLIENT_BIN` env → managed download (dual SHA-256 + version pin `0.0.12`); `env_remove` drops `OPENAI_ADMIN_KEY` / `OPENAI_API_KEY` / `WEBCODEX_TOKEN` | BYPASS | `hard` — long-lived privileged proxy; env scrubbing is the key control. Broker would need daemon lifecycle semantics |
| I4 | `project_entry_openai_tunnel.rs:174` → spawn `:187` | `tunnel-client doctor --health.listen-addr 127.0.0.1:0 --json` | as I3 | BYPASS | `moderate` |
| I5 | `project_entry_openai_tunnel.rs:227` → spawn `:244` | `tunnel-client admin tunnels get <CONTROL_PLANE_TUNNEL_ID>` | tunnel id from a required env var | BYPASS | `moderate` |
| I6 | `project_entry_openai_tunnel.rs:772` / `project_entry_cloudflared.rs:715` | `<binary> --version` | same as I3 / U4; output must match the pinned version | BYPASS | `trivial` |
| I7 | `desktop/.../updates/install/context.rs:53` / `:58` | `/usr/bin/dpkg-query -W -f=${db:Status-Status} webcodex`, `/usr/bin/rpm -q --quiet webcodex` | fully hardcoded, read-only | BYPASS | `trivial` |
| I8 | `desktop/.../platform/opener.rs:72` / `:55`, `platform/permissions.rs:98` / `:122`, `platform/windows.rs:43` | `/usr/bin/open`, `xdg-open`, `ShellExecuteW`, `explorer.exe` | target passes a `url()` allowlist (localhost / runtime / github.com/yyjeqhc/webcodex) or `canonicalize`; panes are 3 constants | BYPASS | `trivial` |
| I9 | `desktop/.../webcodex/cli.rs:695` | `Command::new(executable).args(args)` — the central CLI executor, ~20 logical commands | executable resolved + fingerprinted; args from user config and paired-server state, passed literally | BYPASS (justified) — `ManagedChild` for lifecycle only, no allowlist | `moderate` |
| I10 | `desktop/.../process/supervisor.rs:294` | the only production spawn point for 4 `ProcessKey`s: LocalServer / LocalRunner / QuickShare / RegularTunnel | commands built by `adapter.rs:318/327/354/382` from verified binaries | BYPASS (justified) — `ManagedChild` only | `hard` — see §4.1 |
| I11 | `cli/.../connect/process.rs:246` / `:291` | `ps -p <pid> -o lstart=`, `ps -p <pid> -o command=` | hardcoded `ps`; pid from a `RunnerState` the CLI itself wrote | BYPASS (justified) | `trivial` — read-only introspection of a pid the CLI owns; classify as an allowlisted introspection primitive |
| I12 | `runner/shell.rs:1039` (`configured_script_runtime_plan`) | `node --version` | config program + PATH; argv fixed | BYPASS (justified) — via `into_command()`, `ManagedChild` for process group | `trivial` |
| I13 | `runner/main.rs:2297` (`validation_module_available`) | `python -I -c <fixed PROBE> <module>` | config program + **config** `step.args` module; probe body is a hardcoded constant | BYPASS (justified) — via `into_command()`; comment states it intentionally stays outside the P1 sandbox path | `trivial` |
| I14 | `runner/ssh.rs:259`/`:263`/`:274`/`:861`/`:893`/`:914`/`:930`/`:971` | `ssh -V` capability probes; `ssh -o … -N -f <host>` ControlMaster; `-O check` / `-O stop` / `-O exit` | trusted program; host from config | BYPASS | `trivial`–`moderate` — transport layer |
| I15 | `runner/projects/catalog.rs:361` `run_git_bounded_with_program` | `git` + Runner-constructed literal argv (`init`, `rev-parse --end-of-options`, `worktree add --detach`) | **hardcoded `"git"`**; args include project-derived `base_ref` / `destination` | BYPASS (gap) | `moderate` — **no `env_clear()`**; see F3 |
| I16 | `runner/shell.rs:1325` `run_prepare_command` | consumes a caller-supplied `cmd`; creates none | n/a | BYPASS | follows I12 / M5 |
| I17 | `runner/process_command.rs:37` | `%SystemRoot%\system32\cmd.exe /d /s /v:off /c <quoted>` (Windows only) | `GetSystemDirectoryW` — trusted, not PATH; `windows_batch_command_line` rejects `" % ! ^`, trailing backslash, 8000 UTF-16 cap | BYPASS (constructor) | `hard` — batch `raw_arg` semantics cannot be expressed via broker `args()` |
| I18 | `runner/process_command.rs:42` | `Command::new(program)` + args verbatim | verbatim passthrough, **no local validation** | BYPASS (constructor) | `moderate` — currently **not reached in production**: the only non-test caller is `detached_job.rs:2652`, which is `#[cfg(windows)]` |
| I19 | `runner/detached_job.rs:1924` `handoff_first_platform`, `:2973` `spawn_watchdog`, `:3165` | `current_exe()` + `--internal-supervisor <job_dir> <execution_id> <birth>` / `--internal-watchdog`; unix `make_new_session` (setsid), Windows `CREATE_BREAKAWAY_FROM_JOB` | **trusted hardcoded** `current_exe()`; job_dir / execution_id from durable state | BYPASS | `hard` — daemonization; same family as §5.1 |
| I20 | `runner/coding_agent.rs:1455` `run_turn` | `Command::new(&provider.config.executable)` + config args, `env_clear()`, `current_dir(&request.project_root)` | config `acp.agents[].executable` — **absolute path enforced**, ≤1024 bytes, no NUL | BYPASS | `moderate` — an ACP session is user-initiated; may need `NetworkPolicy::Allow`, which the current compiler rejects |

### 3.1 Authority implementations (5) — these *are* the enforcement path

| # | Site | Program | Notes |
|---|---|---|---|
| A1 | `process/src/execution_broker/mod.rs:553` `build_command_with_toolchain` (macOS) | `/usr/bin/sandbox-exec -p <compiled sbpl> -D<defs>… -- <program> <args…>` | `SANDBOX_EXEC` is a hardcoded const (`:96`). Inner program/args come from `SpawnSpec`, but plan compilation + `check_cwd` + the `--` terminator all fail closed **before** the process exists. `mod.rs:697` is inside `#[cfg(test)] pub(crate) mod testing` and is absent from release builds |
| A2 | `process/src/unix.rs:57` `ManagedChild::spawn_with_options` | caller-assembled `&mut Command` + `process_group(0)` | `ManagedChild` is the process-ownership primitive, **not a security boundary** |
| A3 | `process/src/windows.rs:168` | same, plus `CREATE_SUSPENDED` + Job Object assignment | as A2 |
| A4 | `workspace/src/git_broker.rs` | zero production `Command::new` | the *caller* of `ExecutionBroker`; establishes `EnvPolicy::Minimal` + `HOME=<root>` + `GIT_CONFIG_NOSYSTEM=1` + `GIT_TERMINAL_PROMPT=0` (`:210-218`) and `NetworkPolicy::Deny` (`:101`) |
| A5 | `runner/local_execution.rs:172` `into_command` | `std::process::Command::new(&program)` | the broker escape hatch itself — see F2 |

### 3.2 Build-time (3 sites, 4 calls)

| Site | Program / argv | Broker | Migration |
|---|---|---|---|
| `core/build.rs:129` `watch_git_dirty_inputs` | `git diff-index --name-only -z HEAD --` \| `git ls-files -z` | n/a | n/a — the broker does not exist in the build-script context |
| `core/build.rs:170` `git_dirty_from_git` | `git diff --no-ext-diff --quiet HEAD --` | n/a | n/a |
| `core/build.rs:182` `command_stdout` | `git rev-parse --short=12 HEAD` / `git show -s --format=%ct HEAD` / `git symbolic-ref --quiet HEAD` / `git rev-parse --git-path <name>` | n/a | n/a |

`apps/desktop/src-tauri/build.rs` contains only `tauri_build::build()` — **no build-time Command**.

### 3.3 Auxiliary binaries (2) — built, not installed

| Binary | Spawns | Class | Shipping |
|---|---|---|---|
| `process/src/bin/process_tree_helper.rs` | only itself (`:63`, `:100` → `self_exe`) | `test only` | built by `cargo build --release` (it is a `[[bin]]`), consumed only by `process/tests/managed_child.rs` via `CARGO_BIN_EXE_process-tree-helper`. **No install or packaging path copies it.** Inert unless invoked |
| `process/src/bin/seatbelt_ae_probe.rs` | `/usr/bin/nc -w 2 127.0.0.1 <port>` (`:419`, positive control), `/usr/bin/sandbox-exec -p "(version 1)(allow default)(deny network*)" /usr/bin/true` (`:533`, backend precheck) | `internal only` — human-run from Terminal.app | **not shipped, not installed**. Driven by `research/spikes/native-seatbelt-ae.sh`, which hard-refuses to run unless it can prove it is outside a sandbox wrapper. Everything else in it goes through the **production** broker + profile compiler |

---

## 4. Security observations

Ordered by how much they should change P1B scope. **No policy decisions are made here.**

### 4.1 The desktop and server layers have no broker at all

`ExecutionBroker` / `SandboxPlan` / `LocalExec` are **zero-reference** in both
`apps/desktop/src-tauri/` and `src/`. All broker work happened Runner-side.

| Layer | Production spawns | Broker routing |
|---|---|---|
| `crates/webcodex-runner` | 32 | 4 routed, 28 bypass |
| `apps/desktop/src-tauri` | 17 | 0 routed — 14 bare `Command`, 3 `ManagedChild` only |
| `src/` (main server) | 15 | 0 routed — **all 15 bypass** |
| `crates/webcodex-environment` | 14 | 0 routed |

**Consequence for planning:** in these three layers, migration is not "adapt to the broker" — there is
no broker infrastructure to adapt to. The `ManagedChild` that is present is a **lifecycle primitive**
(`process_group(0)` on unix, Job Object on Windows), not a security boundary: it has no allowlist, no
env scrubbing, no seccomp, no rlimits.

### 4.2 macOS privilege escalation goes through a root shell

`webcodex-environment/src/privilege.rs:664-666`:

```applescript
on run argv
  set cmd to quoted form of item 1 of argv & " environment __service-operation " & quoted form of item 2 of argv & " " & quoted form of item 3 of argv
  do shell script cmd with administrator privileges
end run
```

**This is not an exploitable injection.** The script is a hardcoded literal with no model or network
input, and all three variable values arrive as an `osascript` argv array and are escaped with
`quoted form of`, which is correct POSIX single-quote escaping.

It is still the most fragile architectural point in the crate:

- it is the **only** place in the whole repo that concatenates into a shell, while every other
  escalation path (`sudo`, `pkexec`, `AuthorizationExecuteWithPrivileges`) uses a pure argv array;
- `cli` comes from persisted `SetupRequest.binaries.cli`, and `trusted_legacy_cli()` — the
  root-owned-ancestor-directory check — is `#[cfg(target_os = "linux")]` **only**. `sudo`/`pkexec`
  have no equivalent check on any platform;
- any future edit that appends a value without `quoted form of` becomes immediate root shell injection.

Mitigations that do hold: `elevated()` + `verify_requester()` (program name must match `webcodex*`),
`open_exchange` with `O_NOFOLLOW` / `nlink == 1` / `mode & 0o077 == 0`, request-file owner must equal
requester UID, and the authorization dialog itself is macOS-provided.

### 4.3 Executing a downloaded artifact is verified — but not signed

`environment/src/upgrade.rs:2557` runs `&artifact.path --build-info-json`. The chain is:

1. `verify_upgrade_candidate()` — per-file sha256
2. `verify_published_provenance()` — re-downloads the authoritative manifest from
   `github.com/{repo}/releases/download/`, requires `hex(bytes) == candidate.manifest_sha256`
3. `verify_candidate_executable_headers()` — ELF/PE/Mach-O magic + CPU architecture
4. the probe must echo `build_info` matching the manifest
5. after execution, `digest(&artifact.path) != artifact.sha256` is re-checked

**Not "unverified execution".** Two real caveats:

- **No code-signing verification.** Provenance is anchored in HTTPS + GitHub release assets, and the
  checksum derives from that same unsigned manifest. A compromised publishing account passes every check.
- `upgrade_preflight_development()` explicitly skips provenance (`development_build = true`) while
  still doing checksum. Its name and docs state it is never represented as verified release provenance.

**Recommendation: keep this bypass.** A generic sandbox broker would break the design — the probe must
run as the future root identity and self-report. Rely on the provenance chain instead.

### 4.4 Desktop update pipeline

**Correctly hardened — preserve these during any migration:**

- `pkexec --disable-internal-agent` — prevents pkexec from starting its own auth agent.
- macOS **re-verifies** the CLI path after authorization, before `ExecuteWithPrivileges`
  (`native.rs:258`/`:306`): root-owned, non-world-writable, exact path match.
- Windows holds an exclusive `share_mode(1)` handle to the verified file through `CreateProcess`,
  eliminating the hash-then-reopen TOCTOU.
- `resolver` refuses to let inherited `DESKTOP_MCP_*` variables become a credential source.
- All argv is literal. **The entire `src-tauri` tree has no production `sh -c` and no `cmd /C`.**

**Open items:**

- **C1 — root helper consumes a user-writable path (TOCTOU).** `native.rs:119` (Linux pkexec) and
  `:316` (macOS) hand `--installer-file <user-writable download cache>` to a root-identity helper. The
  desktop-side sha256 check and the helper's `open()` have a window. macOS re-verifies the *CLI*, not
  the *installer file*, and the Linux path has no second check.
  **UNCERTAIN:** whether the helper re-verifies sha256/provenance under root identity — that code is
  `webcodex-environment`'s `installer-apply`, outside the desktop scope audited here. Cross-referencing
  U8 suggests it does (`installer.rs:485` verifies the frozen package), but this was not confirmed at
  the helper's actual read site.
- **C2 — `bound_tree` silently allows symlinks.** `prepare.rs:275-277` returns `Ok(())` on a symlink
  entry. The rpm path is immune because `rpm_candidate_patterns_from_inventory` rejects any non-`d`/`-`
  mode and any non-empty `FILELINKTOS` **before** cpio runs. **`pkgutil --expand-full` and
  `dpkg-deb --control` have no equivalent pre-filter**, so real symlinks can land inside the private
  directory, caught only later by `verify_upgrade_candidate` and the final `canonicalize`.
- **C3 — debug-build shell escape hatch.** `webcodex/cli.rs:671-698`:
  `WEBCODEX_DESKTOP_STUCK_OPERATION_FIXTURE=1` makes `bounded_command()` replace the target program
  entirely with `/bin/sh -c "sleep 120 & …"` (or `powershell.exe`). The gate is a runtime `if` inside
  `#[cfg(debug_assertions)]`, so release builds compile it out and **the shipped artifact is
  unaffected**. But anyone running a debug build with that variable set gets an arbitrary shell with
  full desktop privileges. It exists only as a timeout-test hook, and those tests are already
  `#[cfg(test)]` — **recommend deleting it.**
- **C4 — deprecated privileged API.** `native.rs:226`/`:316` uses
  `AuthorizationExecuteWithPrivileges`, which Apple has deprecated. The code comment acknowledges this.
  Current risk containment (restricted to an existing protected CLI, literal argv, no persisted
  authorization reference) is an acceptable transition, but it should not be long-term.

### 4.5 PATH-hijack exposure on internal binary discovery

`cli/.../system.rs:384-403` scans `PATH` for an **absolute** dir containing `webcodex-server` /
`webcodex-runner`; `discover_sibling_binary` (`:367`) checks `current_exe()`'s directory. A writable
`PATH` entry, or a writable install directory, means the CLI and desktop supervisor will execute an
attacker-named binary. This is conventional for supervisor CLIs and no existing threat-model note was
found. Flagged, not asserted as a defect — severity depends on install-layout guarantees
(e.g. whether `/usr/local/bin` is root-owned in the shipped packages) that were not verified.

### 4.6 Unit-name argv hygiene — UNCERTAIN

`cli/.../service.rs:220` `service_unit_name()` derives the systemd unit from the **service file name**,
and `server.rs:320`/`:569` plus `runner_service.rs:93` accept a `--service-file` flag. The argv is
operator-supplied and not model-reachable, but whether validation constrains the resulting unit string
to a safe character set was **not confirmed**. If a crafted name could inject a leading `-`,
`systemctl` would parse it as a flag. `profiles.rs:106-110` `validate_service_file_scope` is the place
to check.

---

## 5. Findings F1 / F2 / F3 in detail

### F1 — `project_overview.rs:360` is the last unconfined model-reachable git execution

> **STATUS: FIXED in P1B Slice 1.** The bare spawn below was replaced by a
> `git_broker::run_git_bounded` call at `project_overview.rs:396`, with an 8 MiB byte budget, a 5 s
> deadline, and fail-closed semantics (refusal / timeout / truncation / non-zero status → `None`,
> which falls back to the filesystem walk; there is **no** retry and **no** path to an unconfined
> `Command`). The analysis below is preserved as the record of *why* it mattered.

**Severity: real but bounded.** It is a *confusion-of-authority* bug, not arbitrary command execution.

Verified source (`crates/webcodex-workspace/src/project_overview.rs:360-363`, at the P1 baseline):

```rust
let output = std::process::Command::new("git")
    .args(["ls-files", "-z"])
    .current_dir(root)
    .output()
    .ok()?;
```

**Trace (all steps verified):**

1. `src/tool_runtime/file_tools.rs:54` — `ToolCall::ProjectOverview { project, path, max_depth, limit }`
   is a **model tool call**.
2. → `src/tool_runtime/dispatch.rs:70` → enqueued to the owning Runner as `op: "project_overview"`.
3. → `crates/webcodex-runner/src/webcodex_runner/files.rs:87` `RunnerFileOperation::ProjectOverview(_)`
   → `handle_project_overview_request` (`:252`) → `build_project_overview(...)` (`:278`).
4. → `crates/webcodex-workspace/src/project_overview.rs:141` `git_tracked_index(&canonical_root)`,
   reached when `path.is_empty()` — i.e. **the default call shape**.
5. → **`:360` the bare spawn.**

**The model cannot control the argv.** It is the literal `["ls-files", "-z"]`; nothing is interpolated.
The model controls only *whether* it runs, and the cwd — and the cwd is constrained:
`canonical_root` / `canonical_scope` are canonicalized at `:113-123`, then
`canonical_scope.starts_with(&canonical_root)` is asserted at `:124` with an explicit
`"path is outside project directory"` rejection.

**The real exposure is unconfined process authority:**

| Exposure | Consequence |
|---|---|
| Bare `Command`, no `SandboxPlan`, no Seatbelt profile | the child is unconfined |
| **Inherits the full caller environment including `$HOME`** | contrast `git_broker.rs:210-218`, which deliberately sets `HOME=<root>`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_TERMINAL_PROMPT=0` precisely so git cannot read global config, credentials, or hooks. This path loses all of that |
| git shells out to **pagers, hooks, credential helpers, `core.fsmonitor`, `diff.external`** | the repo's own header at `git_broker.rs:8-11` states exactly this: *"git itself shells out to pagers, hooks and credential helpers, so 'this is just a read' is not a safe assumption about what the process tree does"* |
| No byte budget, no deadline | `git ls-files -z` on a huge monorepo is unbounded stdout into a collection — memory amplification. `git_broker::run_git_bounded` exists specifically to bound it (`git_broker.rs:123-138`) |
| No `ManagedChild` | no process-group ownership, no `terminate_tree`; a hung git cannot be killed as a tree |
| No `check_cwd` | `execution_broker/mod.rs:551` is skipped |
| Network unrestricted | `workspace_git_plan` sets `NetworkPolicy::Deny` (`git_broker.rs:101`) |
| No toolchain-root grant, and bare `"git"` resolves via the **inherited** `PATH` | a poisoned `PATH` entry wins |

**Why it is very likely an oversight:** it is the **only** remaining production `Command::new` in
`webcodex-workspace`. Its sibling `project_context.rs` was migrated (documented at its `:585`),
`git_broker.rs` and `workspace_checkpoint.rs` were migrated. This one file was missed.

**Migration: `moderate`.** Route `:360` through
`git_broker::run_git_bounded(canonical_root, &["ls-files","-z"], <budget>, deadline)`. The
`Option` fallback semantics at `:365-367` / `:383-385` (non-zero exit or empty index → `None` →
filesystem walk) map cleanly onto `status.success()` and the existing `complete` / `truncated` result.
The one genuine design question is the byte-budget/deadline policy, because the current fallback
semantics depend on getting the *whole* index.

### F2 — the anti-bypass guard does not pin `into_command()`

> **STATUS: FIXED in P1B Slice 1.** `.into_command()` and `.spawn()` were added as needles; an
> exhaustive production-caller allowlist was added; and each allowed caller must now self-declare
> `CONTROL_PLANE_FIXED_PROBE`. `into_command()` itself was deliberately **not** removed. The
> pre-Slice-1 analysis is preserved below.

Verified from `crates/webcodex-runner/src/webcodex_runner/normalization_p1_tests.rs` (at the P1 baseline):

```rust
const P1_ROUTED_FUNCTIONS: &[(&str, &str)] = &[
    ("shell.rs", "configured_shell_command"),
    ("shell.rs", "configured_prepared_shell_command"),
    ("shell.rs", "configured_explicit_shell_command"),
    ("shell.rs", "configured_shell_job_command"),
    ("shell.rs", "configured_prepared_shell_job_command"),
    ("shell.rs", "configured_validation_job_command"),
    ("shell.rs", "configured_process_command"),
    ("shell.rs", "execute_configured_command"),
    ("local_execution.rs", "spawn_local_action"),
];

const BYPASS_PATTERNS: &[BypassPattern] = &[
    BypassPattern { needle: "ManagedChild::spawn(", ... },
    BypassPattern { needle: "Command::new(",       ... },
];
```

These are the **only two needles** (`:997`, `:1001`).

`into_command()` (`local_execution.rs:170`) is a **third** way for a P1-routed function to reach an
unbrokered spawn, and it is **not** in `BYPASS_PATTERNS`. It is already used by two production
control-plane probes — `shell.rs:1039` (`node --version`) and `main.rs:2297`
(`validation_module_available`) — plus `shell_tests.rs` and `job_manager_tests.rs`.

The guard's own doc comment scopes it narrowly and deliberately ("the narrow guard the P1 scope
actually implies"), and it already carves out an exception for `local_execution.rs` +
`Command::new(`. So this is a gap in an intentionally narrow guard, not a defect in it.

**Consequence:** P1's central anti-bypass invariant currently rests on **convention rather than
mechanism**. Today `into_command()` has exactly 2 production call sites, both fixed-argv probes. A
third call added tomorrow would not fail any test.

**Migration: `trivial`, highest leverage in this document.** Add `into_command(` as a needle (with the
same `local_execution.rs` carve-out), and add an assertion pinning the production call-site count to
the 2 known probe functions.

### F3 — `run_git_bounded` inherits the full runner environment

Verified from `crates/webcodex-runner/src/webcodex_runner/projects/catalog.rs:361-372`:

```rust
let mut command = Command::new(program);
command
    .args(args)
    .current_dir(path)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
// ManagedChild owns the whole Git process tree ...
let mut child = match ManagedChild::spawn(&mut command) { ... };
```

**No `env_clear()`.** The git child inherits the entire runner environment, including `GH_TOKEN`,
`AWS_*`, and anything else the runner holds. This directly contradicts the positive-list principle P1
established on `LocalEnv`, and it is **structurally identical** to the P1-I fix already applied to
`validation/execute.rs`.

Program is trusted (hardcoded `"git"`; `_with_program` is a test seam), and the argv is
Runner-constructed literals plus project-derived `base_ref` / `destination`. The gap is
environmental, not argv.

Triggers (prepared analysis, at the P1 baseline): project register / create / worktree —
`projects/lifecycle.rs:681`, `managed_worktree.rs:91,103,870`, dispatch `:686,697`.

> **SLICE-1 CORRECTION.** The trigger list above is incomplete. Slice 1 proved a second, and
> simpler, production trigger: `catalog.rs:694` passes `include_git = true` **unconditionally** in
> `load_runner_project_summaries_from_dir_with_shutdown`, which is reached from the project-inventory
> push (`transport/project_inventory.rs:231`), from `transport.rs:269`, and from
> `lsp/adapter.rs:86` on **every model LSP tool call**. So F3 is reached far more often than project
> registration. Full proof in §1.4.3. `CATALOG_ENV_HARDENING = FOLLOW_UP`.

**Migration: `moderate`, not `trivial`** — git reads `~/.gitconfig`, so `HOME` semantics matter; a bare
`EnvPolicy::Minimal` would break it. The fix needs a controlled profile snapshot, in the same shape as
`shell.rs`'s prepared-profile path. Applying F1's `git_broker` route to this call would close F1 and F3
together.

---

## 5.1 Why the detached job family cannot simply be migrated

Three independent reasons, each sufficient on its own:

**(a) It escapes process ownership.** `detached_job.rs:3132-3133` → `make_new_session` (`:2217-2230`)
→ `pre_exec` `libc::setsid()`: the supervisor becomes a new session leader, **leaving the runner's
process group**. Windows uses `CREATE_BREAKAWAY_FROM_JOB` (with an `ERROR_ACCESS_DENIED` fallback at
`:1927-1951`). But `ManagedChild`'s entire termination semantics *is* "private process group /
kill-on-close Job Object" (`process/src/unix.rs:99`, `windows.rs:232`). A post-`setsid` process is
**not in the runner's group**, so `terminate_tree` cannot reach it.

**(b) It outlives the runner.** The broker's design comment is explicit
(`execution_broker/mod.rs:413-415`): *"The broker is a stateless function of a SpawnSpec… never
itself sandboxed, which is what keeps the control plane able to create a different profile for the
next action."* The entire purpose of a detached job is to **survive runner restart**
(`reconcile_after_runner_restart`, `:385`). Its durable state machine
(`Prepared → SupervisorStarted → OwnershipAccepted → Running`) replaces process ownership, and the
broker has no way to learn which `SandboxPlan` belongs to a runner that no longer exists.

**(c) Seatbelt profiles cannot be re-attached across processes.** On macOS the profile is applied by
`sandbox-exec` re-execing the target (`execution_broker/mod.rs:597-601`). A supervisor must start,
handshake, and commit durable state **before** spawning the payload — so the payload must **inherit**
the supervisor's confinement rather than receive a fresh profile. Today the payload is a bare
`Command::spawn` with zero profile. Fixing this requires confining the supervisor itself, which is
`current_exe()` and must daemonize, which is (a). Circular.

`normalization_p1_tests.rs:1079-1084` pins this state deliberately: it asserts the string
`"failed to spawn detached payload"` must remain in the source, recorded as
`BLOCKED_PROCESS_OWNERSHIP` rather than complete.

**Planning implication:** record this family as needing an **independent execution authority** (a
daemon path with its own confinement contract), not as "pending migration to `ExecutionBroker`". The
latter is an unachievable goal.

One thing *is* achievable independently of the daemonization question: M10's env is `env_clear()`
followed by re-inserting `launch.env` with **no allowlist**, and `launch.env` originates from
`DetachedStartRequest.env`. Routing that through `sanitize_snapshot` is `moderate` and closes a real
gap without touching daemonization.

---

## 6. Test-only surfaces (excluded from all production counts)

Confirmed test-only, not deep-audited:

| Area | Files |
|---|---|
| `src/` | `project_entry_tests.rs`, `project_entry_cloudflared_tests.rs`, `project_entry_openai_tunnel_tests.rs`, `project_entry_client_handoff_tests.rs`, `project_entry_windows_migration_tests.rs`, `mcp_tests/runtime_tools.rs`, `runtime_http_tests.rs`, `tool_runtime/tests/**` (~30 files), `server_listener.rs:369` (inside `#[cfg(test)] mod tests` from `:295`), `tool_runtime/helpers.rs:24`/`:146`/`:255` (all `#[cfg(test)]`; **no production caller**) |
| runner | `fake_plugin.rs`, `fake_claude_mcp.rs`, `shell_tests.rs`, `plugin_tests.rs`, `plugin_check_tests.rs`, `external_tools_tests.rs`, `mcp_gateway_tests.rs`, `runner_instruction_tests.rs`, `runner_skills_tests.rs`, `job_manager_tests.rs`, `main_tests.rs`, `main_tests/**`, `detached_job/tests.rs`, `detached_job.rs:3145`, `validation/execute.rs:505`/`:972`, `validation/validation_tree_helper.rs:140` |
| workspace | `project_context.rs:1166`/`:1169`, `project_overview.rs:1504`/`:1548`/`:1617`, `git_broker.rs:527`, `workspace_checkpoint.rs:909`/`:914` |
| lsp | `test_support.rs`, `fake_server.rs`, `tests.rs`, `navigation_test_support.rs`, `navigation_tests.rs` |
| process | `tests/managed_child.rs` (9 hits), `tests/execution_broker.rs`, `execution_broker/mod.rs:697` (inside `#[cfg(test)] pub(crate) mod testing`) |
| cli | `controller.rs:2020` (inside `#[cfg(test)] mod tests` from `:1833`), `connect/process.rs:600`, `tests/server/service.rs` |
| desktop | `process/tests/*`, `state.rs`, `state/reconfiguration_tests.rs`, `state/mcp_providers/tests.rs`, `connections/tests.rs`, `desktop_data_dir.rs`, `mcp_providers/tests.rs`, `tunnel_config/tests.rs`, `updates/install/prepare/native_tests.rs`, `webcodex/cli.rs` tests including `tasklist.exe` at `:1282` |
| computer | `windows_uia_tests.rs` |
| fixtures | `tests/fixtures/process_argv_helper.rs`, `lsp/src/fake_server.rs` (rustc-compiled standalone fixture) |

**Two prior candidate entries were corrected as test-only, not production:**

- `src/tool_runtime/helpers.rs` — all 5 hits are `#[cfg(test)]`. The `Command::new(shell)` at `:24`
  takes its shell from `test_shell()` (`:244`/`:249`, both `#[cfg(all(test, …))]`), returning a
  hardcoded `sh` / `sh.exe`. **It has no production caller**, including at
  `tool_runtime/files.rs:12` and `files/search.rs:2305`/`:2539`, which are all behind `#[cfg(test)]`.
- `desktop/.../webcodex/cli.rs:1282` `tasklist.exe` — inside `#[cfg(test)] mod tests` (from `:936`),
  used only by an `#[ignore]` real-process-tree test. Production uses
  `ManagedChild::wait_tree_exit` and spawns nothing.

---

## 7. `UNCERTAIN` register

Recorded rather than guessed. Each names the missing evidence.

| # | Question | Missing evidence |
|---|---|---|
| U1 | Does the helper re-verify `--installer-file` sha256/provenance under root identity? | requires reading `webcodex-environment`'s `installer-apply` at its actual read site; cross-referencing U8 suggests it does, but this was not confirmed there |
| U2 | Is `service_unit_name()` constrained to a safe character set (no leading `-`, no systemd metacharacters)? | requires reading `profiles.rs:106-110` `validate_service_file_scope` and the `--service-file` handlers |
| U3 | Are `/usr/local/bin` and the desktop install directories root-owned in the shipped packages? | packaging layout was not audited; determines whether §4.5 is a real exposure |
| U4 | Is the browser/CDP launch (`browser/src/cdp.rs:371`) a deliberate, recorded exemption? | **no ADR or security-invariant entry was found.** The `--remote-debugging-address=127.0.0.1` binding and absent profile *suggest* intentionality. Treat exemption status as **unconfirmed** |
| U5 | Does `CodingAgentStartRequest.project_root` originate from user selection or from a model-influenced path? | the operation-construction side lives outside the three audited crates (likely `webcodex-core`) |
| U6 | Does `skill_resource_execution` land on `execute_configured_command`, or does it have an independent spawn path? | `dispatch.rs:314` groups it with `run_process`/`run_script`/`run_internal_posix_script`, and `start_skill_resource_job` (`dispatch.rs:241`) was not traced end to end. **If it has its own spawn path, an unlisted site may exist** |
| U7 | Does `run_native_shell_or_internal_search` (`dispatch.rs:628`) try multiple interpreters before falling back to the native selector? | `dispatch.rs:625-627` comments imply multi-path routing; intermediate attempts were not fully expanded. Likely all converge on M1 |
| U8 | Does `webcodex-lsp` spawn anywhere outside `supervisor.rs`? | `.spawn()`/`.output()` hit only `supervisor.rs:203`, `fake_server.rs:365`, `test_support.rs:73`. Indirect probes in `language.rs` callbacks were not exhaustively checked; leaning none |
| U9 | Can any remote control surface trigger `controller.rs` component restarts? | no `TcpListener` in `controller.rs`, and no external supervisor forwarding was audited. Classified `user initiated` on that basis |
| U10 | Is `PLUGIN_MAX_COMMAND_BYTES` bounded as expected, and can plugin config inject arbitrary args? | the constant's value and the full `prepare_provider` config struct were not read |

---

## 8. Counts

> **Slice-1 note.** The `model reachable` row is updated: 4 routed → 5 routed, 11 bypass → 10 bypass.
> F3 (`catalog.rs`) moved from `internal only` to `model reachable` — see §1.4.3 for the proof.
> The `user initiated` row is unchanged. **Total production is unchanged at 56**: Slice 1 changed
> the *routing* of one site and the *classification* of another; it added and removed no site.

### 8.1 By reachability class

| Class | Production sites | Of which routed |
|---|---|---|
| `model reachable` | **15** | **5** |
| `user initiated` | **17** | 0 |
| `internal only` | **23** (was 24 — `catalog.rs` reclassified) | 0 |
| `test only` | excluded | — |
| **Total production** | **56** (unchanged) | **5** |

Breakdown of the 24 internal-only: 13 in `src/` + `webcodex-environment`, 4 desktop, 6 runner, 1 auxiliary binary.
Authority implementations (5) and build-time sites (3) are counted separately in §3.1 / §3.2.

### 8.2 By broker routing (production sites only)

| Status | Count | Notes |
|---|---|---|
| `ROUTED` | **5** | all model-reachable, all in runner + workspace (was 4) |
| `BYPASS (gap)` | **11** | the P1B work list (was 12; −`project_overview`) |
| `BYPASS (justified)` | **11** | documented or structural exemptions |
| `BYPASS (exempt)` | **6** | privilege / service / daemon planes |
| `n/a` (exec, build-time, authority) | 23 | see §3.1, §3.2, U15 |
| **Not using the broker at all** | **52 / 56** | the broker exists in one layer only |

### 8.3 Model-reachable sites — the short list (post Slice 1)

| Site | Status | Complexity |
|---|---|---|
| `shell.rs:3153` `execute_configured_command` | ROUTED | done (P1) |
| `job_manager.rs:2999` Shell Job step 1 | ROUTED | done (P1) |
| `job_manager.rs:3266` validation step | ROUTED | done (P1) |
| `validation/execute.rs:101` `run_bounded` | ROUTED | done (P1) |
| `workspace/project_overview.rs:396` `git_tracked_index` | **ROUTED (Slice 1)** | done |
| `projects/catalog.rs:361` (F3 — env inheritance) | gap | `moderate` — **Slice 2** |
| `shell.rs:1532` `capture_profile_env_snapshot` | gap | `moderate` |
| `shell.rs:1729` Tool Plugin launcher | gap | `moderate` |
| `webcodex-lsp/src/supervisor.rs:203` | gap | `moderate` |
| `persistent-shell/lib.rs:1924` + `windows.rs:269` | gap | `hard` |
| `external_tools.rs:1121` + `mcp_gateway.rs:605` + `plugin.rs:1235` | gap | `moderate` |
| `detached_job.rs:2391` + `:2677` | structurally blocked | `hard` — §5.1 |
| `ssh.rs:1142` + `job_manager.rs:3459` + `remote_shell.rs:109`/`:116` | structurally blocked | `hard` |

**Authoritative per-site detail, with `surface → dispatch → function → spawn` evidence for each of the
10 remaining bypasses, is §1.4.1.** This table is a summary only; where the two differ, §1.4 wins.

---

## 9. Observations, not recommendations on policy

Stated as constraints. **Choosing among them is a scope decision for the ticket author, not for this
document.**

1. **F2 is the cheapest real improvement** — `trivial`, and it converts P1's most important invariant
   from convention into mechanism.
2. **F1 and F3 share a fix.** Both are git executions with the wrong authority model; routing both
   through `git_broker` closes them together. F1 is model-reachable, F3 is not, but the env-inheritance
   defect is identical.
3. **The desktop and server layers need a broker built, not adapted.** 32 production spawns across
   `src/` + `src-tauri/` have no enforcement point to migrate onto. Whether P1B should build one, or
   explicitly exempt those layers with recorded rationale, is a scope decision.
4. **The privilege and daemon planes are candidates for documented exemption, not migration.**
   `webcodex-environment`'s escalation paths, `upgrade.rs`'s provenance probe, and the detached job
   family each have a structural reason a local Seatbelt broker is the wrong tool. Recording them as
   explicit exemptions with rationale would be honest; routing them would be theatre.
5. **`environment` and `session_service` unit/plist writes are authority surfaces without any spawn.**
   `service/linux.rs:293` writes `/etc/systemd/system/{id}.service`, `service/macos.rs:248` writes
   `/Library/LaunchDaemons/org.webcodex.{id}.plist`, `session_service.rs:806` writes a user LaunchAgent
   plist, `session_service.rs:519` writes temp XML for `schtasks /Create`. All are root-gated
   (`geteuid() != 0` rejection) with `ensure_safe_new_path()` (`create_new` + refuse existing + refuse
   symlinked parent). Rendering escapes properly: `encode_arg` (`linux.rs:520`), `encode_path`
   (`linux.rs:535`), `xml()` (`macos.rs:361`). **No injection found** — but these are privilege
   boundaries and a spawn-only inventory would miss them entirely.
6. **One debug-build shell escape hatch should probably just be deleted** (§4.4 C3). It is not a
   production exposure, and it is the only place in the desktop tree that runs `sh -c`.

---

## 10. Boundary statement

```
Production code modified          = NONE
Files created                     = SPAWN_SURFACE_INVENTORY.md (this file)
Implementation performed           = NONE
Refactor performed                = NONE
Approval / policy system added    = NONE
P1b implementation started        = NO
P2 started                        = NO
Existing P1 conclusions reopened  = NO
```

**Claims explicitly NOT made:**

- That any site is safe. Absence of a sandbox is recorded as an observation with its authority path,
  not scored.
- That `HOST_UNAVAILABLE` or `ENV_BLOCKED` implies anything here. No native gate was run for this
  inventory; P1 native status remains `PARTIAL`, `P1B_REQUIRED=YES`, `P1_NATIVE_ALL_PASS=false`.
- That the `UNCERTAIN` items in §7 are resolved. They are open.
- Any policy decision about what *should* be migrated. §9 states constraints; the ticket author decides.

**Verification commands used** (read-only):

```bash
git rev-parse --short HEAD                       # 40a18b2b
grep -rn 'into_command' crates/ --include=*.rs   # 2 production callers + tests
grep -n 'needle:' .../normalization_p1_tests.rs  # 2 needles only
sed -n '355,370p' .../project_overview.rs        # F1 verified at source
sed -n '358,375p' .../projects/catalog.rs        # F3 verified: no env_clear
grep -rn 'build_project_overview' crates/        # F1 call chain verified
grep -rn 'RunDetachedProcess' .../tool_call.rs   # detached model-reachability verified
```
