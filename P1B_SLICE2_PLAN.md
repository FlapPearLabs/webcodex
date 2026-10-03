# P1B_SLICE2_PLAN.md

> **当前权威入口（P1B最终结构分类）：** 此文保留原baseline的历史计划、库存及验收记录。当前逐项分类、原语与逻辑计数、触发来源及fingerprint以 [launch-inventory.json](research/implementation/p1b/launch-inventory.json) 为唯一真源；可读说明见 [launch-inventory.md](research/implementation/p1b/launch-inventory.md)。新清单明确区分模型业务权限与模型触发固定探测；本页旧口径不作为当前完整库存。分类接受与最终guard / native / exact-SHA验收状态分别记载。

**ROLE** = `WEBCODEX_P1B_SLICE2_PLANNING`
**MODE** = `READ_ONLY_ANALYSIS_ONLY` · `NO CODE CHANGES`
**Baseline** = branch `impl/webcodex-p1b-normalization-slice1` @ `0701cce3bafb79e1aeb20378bb6aa836fc4ad7f0`
**Local HEAD == origin HEAD** = verified
**Inputs** = `SPAWN_SURFACE_INVENTORY.md`, `NORMALIZATION_P1B_SLICE1_REPORT.md`
**Code modified** = `NONE` · **Implementation started** = `NO` · **P1C started** = `NO` · **Approval started** = `NO`

```text
MODEL_REACHABLE_TOTAL = 15
MODEL_REACHABLE_ROUTED = 5
MODEL_REACHABLE_BYPASS = 10
```

---

## 0. Method — and why prior labels were not trusted

The brief says **do not trust old inventory labels**. Every one of the 10 was therefore
re-derived from source at `0701cce3`, independently of `SPAWN_SURFACE_INVENTORY.md`.

Two tool traps were found and neutralised first, both of which had already produced wrong
conclusions earlier in this work:

| Trap | Wrong result it produces | Method used instead |
|---|---|---|
| `grep -E 'a\|b'` alternation is inert in this sandbox | "no hits" read as "no dispatch" | Python regex over full file contents |
| Dispatch is written `Self::X`, not `ToolCall::X` | `ToolCall::`-prefixed search returns 0 → wrongly concluded `RegisterProject` had no model variant | parse the `ToolCall` enum body directly (179 variants, `tool_call.rs:1392-5244`) |
| First `#[cfg(test)]` line is not a code boundary (`shell.rs:111` is an out-of-line `mod desktop_mcp_env_tests;` declaration) | silently drops ~2000 production lines | brace-matched `#[cfg(...test...)]` item spans |

**Model-visibility was re-derived, not inherited.** All of these are confirmed members of the
model-visible `ToolCall` enum: `RunShell`, `RunProcess`, `RunDetachedProcess`, `OpenSessionShell`,
`SessionShellExec`, `SshResource`, `PluginTool`, `CodingAgentStart`, `LspStatus` + 7 more LSP
navigation tools, `ProjectOverview`, `ListProjects`, `RegisterProject`, `CreateProject`.

### 0.1 Independent re-verification of the 10

All 10 re-checked at the stated line, with brace-matched test-region classification:

| ID | File : line | Enclosing fn (re-derived) | Class |
|---|---|---|---|
| B1 | `webcodex-runner/.../projects/catalog.rs:361` | `run_git_bounded_with_program` (351) | **PROD** |
| B2 | `webcodex-runner/.../shell.rs:1532` | `capture_profile_env_snapshot` (1514) | **PROD** |
| B3 | `webcodex-runner/.../shell.rs:1729` | `native_command` (1694, in `impl PreparedExecutionEnvironment`) | **PROD** |
| B4 | `webcodex-lsp/src/supervisor.rs:203` | `LspCommand::spawn` (198) | **PROD** |
| B5 | `webcodex-persistent-shell/src/lib.rs:1924` | `spawn_shell_process` (1919) | **PROD** |
| B6 | `webcodex-persistent-shell/src/windows.rs:269` | `spawn_shell_process` (249) | **PROD** |
| B7 | `webcodex-runner/.../detached_job.rs:2391` | `run_accepted_payload` (2372) | **PROD** |
| B8 | `webcodex-runner/.../detached_job.rs:2677` | `run_accepted_payload` (2646) | **PROD** |
| B9 | `webcodex-runner/.../ssh.rs:1142` | `spawn_piped_ssh_child` (1139) | **PROD** |
| B10 | `webcodex-runner/.../remote_shell.rs:109` | `RemotePersistentShell::spawn` (82) | **PROD** |

**All 10 confirmed: production, correctly located, genuinely model-reachable.** No relabelling
was needed. A reverse sweep (72 production launch sites in the model-tool-layer crates) found
**no missing site**; every production site outside these 10 is accounted for in §D.

### 0.2 Two corrections to the Slice 1 report's *reasoning* (not its counts)

**Correction 1 — B7/B8's architectural blocker is the supervisor, not the payload.**
The Slice 1 report says the blocker is "`setsid` escapes the process group". Re-derived precisely:

- `detached_job.rs:3133` `make_new_session(&mut command)` and `:3136`
  `creation_flags(CREATE_BREAKAWAY_FROM_JOB)` apply to
  `detached_supervisor_command` — the **supervisor** layer, via `internal_mode_command`, which runs
  `current_exe()` with a Runner-authored internal mode and `env_clear()`.
- The **payload** at `:2407` only does `payload_command.process_group(tree_pid as i32)` — it
  *joins* the watchdog's process group. It has no `setsid` of its own.

So the payload spawn is *not* what makes the detached family unmigratable. The supervisor's need to
survive the runner's lifetime is. This matters for Slice 2 scoping: the payload is a candidate for
confinement, the supervisor is not, and they should not be bundled.

**Correction 2 — B4's environment already contains a closed network policy, by design.**
`supervisor.rs:209` layers `profile_for_kind(kind).process_env` over the child env. For `gopls`
(`language.rs:159+`) that is `GOPROXY=off`, `GOSUMDB=off`, `GOTOOLCHAIN=local`, plus
`HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY=http://127.0.0.1:0` and `NO_PROXY=localhost,127.0.0.1,::1`.
The in-source comment states this is deliberate: gopls invokes the Go command during workspace
loading and current builds may start a telemetry sidecar.

This **strengthens** the case for routing B4: `NetworkPolicy::Deny` in the Seatbelt profile is
strictly stronger than a proxy pointing at a dead port, and it expresses the same intent. The
migration must carry these 10+ named variables through `SpawnSpec::env_vars` (not `Inherit`), or
gopls loses its Go toolchain pinning.

---

## 1. Per-bypass analysis

### B1 — `catalog.rs:361` · `run_git_bounded_with_program`

- **Surface**: `ToolCall::ListProjects` — and, independently, every project-inventory push.
- **Dispatch chain**:
  `ToolCall::ListProjects` → runner-registry inventory →
  `transport/project_inventory.rs:231` `polling_projects_for_poll` →
  `RunnerProjectCache::get_with_shutdown` → `catalog.rs:739` `load_runner_project_summaries` →
  `catalog.rs:695` `runner_project_summary_with_shutdown(&p, updated_at, true, shutdown)`
  (**`include_git` is hardcoded `true` in production**) → `catalog.rs:565/:570/:575`
  `run_git_capture("rev-parse --abbrev-ref HEAD" | "log -1 --pretty=format:%h" | "status --short")`
  → `catalog.rs:361` `Command::new("git")`.
  Also reached from `transport.rs:269`, `lsp/adapter.rs:86` (**every model LSP tool call**), and
  `validation/mod.rs:182`.
- **Launch primitive**: `Command::new("git")` (`:361`) + `ManagedChild::spawn` (`:371`).
- **Current authority source**: hardcoded `"git"`; argv is Runner-authored literals; cwd =
  project root. `run_git_bounded_with_program` is a test seam — production always goes through
  `run_git_bounded` which passes `"git"`.
- **Risk**: **highest of the four migratable sites.** No `env_clear()` → the git child inherits the
  full runner environment, including any `GH_TOKEN` / `AWS_*` present. git itself shells out to
  pagers, hooks, credential helpers, `core.fsmonitor`, `diff.external` — so "it is only a read" is
  not a safe assumption. No byte budget.
- **Migration difficulty**: **moderate.** Two concrete obstacles found:
  1. `EnvPolicy::Minimal` supplies only `PATH=/usr/bin:/bin` and `HOME=<cwd>`
     (`execution_broker/mod.rs:569-575`). git reads `~/.gitconfig`; a bare `Minimal` changes
     observable behaviour. This is the same reasoning that made F1 `moderate` rather than `trivial`.
  2. `workspace_git_plan` (`git_broker.rs:75-103`) **refuses a non-absolute root**, but
     `catalog.rs:562-564` falls back to the un-canonicalized `project.path` when
     `canonicalize_existing` fails. Routing therefore requires an explicit fail-closed decision at
     that fallback, not a silent pass-through.
- **Reusable**: `git_broker::run_git_bounded` already provides plan + `EnvPolicy::Minimal` +
  `HOME=<root>` + `GIT_CONFIG_NOSYSTEM=1` + `GIT_TERMINAL_PROMPT=0` + byte budget + deadline, and
  `catalog.rs:455-460` already honours a truncated capture (`stdout_capped → None`). The shape of
  the fix is "call the existing broker", plus the two obstacles above.

### B2 — `shell.rs:1532` · `capture_profile_env_snapshot`

- **Surface**: `RunShell` / `OpenSessionShell` profile initialisation.
- **Dispatch chain**: model tool call → `RunnerOperation::RunShell` / `PersistentShell` →
  `shell.rs` profile preparation → `capture_profile_env_snapshot` → `Command::new(program)` →
  `run_prepare_command` (`:1325`) → `ManagedChild::spawn`.
- **Launch primitive**: `Command::new(program)` + `run_prepare_command`.
- **Current authority source**: config `profile.program` (PATH lookup) + **config** `init_script`.
  The model chooses neither.
- **Risk**: moderate. Already better than B1 — `env_clear()` at `:1536` then `initial_env` is
  re-inserted key-by-key. The gap is that `initial_env` is caller-derived and not filtered by a
  positive allowlist, and the profile program is PATH-resolved.
- **Migration difficulty**: **moderate, and semantically delicate.** This function's *purpose* is to
  capture an environment snapshot by running a shell. Under `EnvPolicy::Minimal` the captured
  snapshot would change content, which propagates into every later `B3` plugin launch and every
  routed shell invocation (`apply_env_snapshot`). Routing it is defensible; routing it *naively* is
  a behaviour regression. The plan needs a controlled profile snapshot, not a blanket `Minimal`.

### B3 — `shell.rs:1729` · `PreparedExecutionEnvironment::native_command`

- **Surface**: `ToolCall::PluginTool` (action `Call`).
- **Dispatch chain**: `ToolCall::PluginTool(PluginToolCall)` → server `src/plugin_gateway.rs`
  (`PluginToolCall` handlers) → `PluginGatewayRequest::ToolsCall` →
  `dispatch.rs:416` `RunnerOperation::PluginGateway` → `plugin.rs` →
  `prepare_provider` (`:1154`) → `plugin.rs:1209` `environment.native_command(&config.command, …)`
  → **`shell.rs:1729` returns a `Command`** → `plugin.rs:1239` `ManagedChild::spawn`.
- **Launch primitive**: `Command::new(native)` — **note it returns a `Command`; it does not spawn.**
  The spawn is at `plugin.rs:1239`.
- **Current authority source**: resolved program from config `plugins[].command`, resolved inside the
  prepared PATH via `env_lookup(&self.env_snapshot, "PATH")`.
- **Risk**: the plugin has **no workspace-authority anchor today** — nothing ties the plugin process
  to the project root. Config-driven program, so not arbitrary, but unconstrained.
- **Migration difficulty**: **moderate, but the blocker is structural plumbing, not the broker.**
  Because `native_command` *returns* a `Command` rather than spawning, the authority
  (canonical project root, plan) is not in scope where the `Command` is built — it is in scope at
  `plugin.rs:1239`, which currently receives only a `Command`. Threading a `SpawnSpec`/authority
  parameter from the dispatch layer down to `native_command` is required. This is the "authority
  threading" pattern, but it is a **single-crate, ~2-function** change, not a redesign.

### B4 — `webcodex-lsp/src/supervisor.rs:203` · `LspCommand::spawn`

- **Surface**: `LspStatus`, `DocumentSymbols`, `DocumentDiagnostics`, `Hover`, `WorkspaceSymbols`,
  `GotoDefinition`, `FindReferences`, `CallHierarchy` (9 model tools).
- **Dispatch chain**: model LSP tool → `RunnerOperation::Lsp` → `lsp/adapter.rs`
  `resolve_runner_project` (`:86`) → `webcodex-lsp/src/supervisor.rs` → `LspCommand::spawn`.
- **Launch primitive**: `Command::new(&self.program)` (`:203`) + `ManagedChild::spawn` (`:214`).
- **Current authority source**: config `commands[kind]` → `WEBCODEX_LSP_*` env override → PATH
  default. **argv is not model-controlled** (that part is already sound).
- **Risk**: unconfined process with a full inherited environment; 9 model tools reach it.
- **Migration difficulty**: **moderate → lower than the inventory recorded.** Two enabling facts
  found:
  1. `canonical_project_root()` already exists at `supervisor.rs:1326` and
     `spawn(&self, project_root: &Path, kind)` already receives a canonical root, using it as
     `current_dir`. The broker's required authority input is **already in scope**.
  2. The `process_env` network lockdown (§0.2) means `NetworkPolicy::Deny` matches existing
     intent rather than fighting it — provided the named variables are carried explicitly.
  Obstacle: `webcodex-lsp` is a separate crate; it must gain a dependency on the broker, and the
  per-profile `process_env` must be forwarded via `env_vars` rather than inherited.

### B5 / B6 — persistent shell (unix / windows)

- **Surface**: `OpenSessionShell`, `SessionShellExec`.
- **Dispatch chain**: model tool → `RunnerOperation::PersistentShell` → `persistent_shell.rs:682`
  → `webcodex-persistent-shell` → `spawn_shell_process`.
- **Launch primitive**: `Command::new(&launch.program)` + `.spawn()` (unix `:1965`) /
  `ManagedChild::spawn` (windows `:280`).
- **Current authority source**: model may pick `sh` vs `bash` **only** (2-element allowlist); args
  and env from `base_shell_env`; `initialization` = config `init_script`. Environment is already
  `env_clear()` + `envs(&launch.env)`.
- **Risk**: an interactive long-lived shell with a bidirectional control channel. Confinement
  would break the session semantics the feature exists to provide.
- **Migration difficulty**: **hard — architecturally blocked, not effort-blocked.** Verified at
  `lib.rs:1932-1963`: the unix `pre_exec` closure performs `setsid()` **and**
  `dup2(control_write_fd, CONTROL_FD)`, `dup2(STDOUT_FILENO, STDOUT_SYNC_FD)`,
  `dup2(STDERR_FILENO, STDERR_SYNC_FD)`, plus three `fcntl(F_SETFD, 0)` calls. The broker works by
  re-execing under `/usr/bin/sandbox-exec` (`execution_broker/mod.rs:592-600`), which does not
  preserve this hand-built FD wiring. Also: an interactive session is unbounded in lifetime, so
  the "one `SpawnSpec`, one child" shape does not fit.

### B7 / B8 — detached job payload (unix / windows)

- **Surface**: `ToolCall::RunDetachedProcess`.
- **Dispatch chain**: `ToolCall::RunDetachedProcess` → `RunnerOperation::Job(StartDetachedProcess)`
  → `detached_job.rs` → `run_accepted_payload`.
- **Launch primitive**: `Command::new(&launch.process.executable)` (`:2391`) + `.spawn()` (`:2409`,
  unix) / `ManagedChild::spawn` (`:2677`, windows).
- **Current authority source**: **model argv verbatim** (`ShellProcessArgv`); only
  `validate_process_argv` + cwd/stdin/env length caps. `env_clear()` then `launch.env` re-inserted
  with **no allowlist**.
- **Risk**: the highest-severity site in the set — model-controlled argv *and* an unfiltered
  environment. `DetachedStartRequest.env` can carry arbitrary variables including credentials.
- **Migration difficulty**: **split by layer, and this is the key planning insight.**
  - *Payload* (`:2391`/`:2409`/`:2677`): model argv is real, but the process is a normal child that
    joins the watchdog group. Confinement is **architecturally conceivable** — the obstacle is
    that the supervisor's lifetime contract must be preserved.
  - *Supervisor* (`:3123-3138` `detached_supervisor_command` → `make_new_session` /
    `CREATE_BREAKAWAY_FROM_JOB`): **cannot** be brokered. It must outlive the runner; the broker is
    a stateless `SpawnSpec` → `sandbox-exec` → child function with no re-attach semantics, and a
    Seatbelt profile cannot be attached to an already-running process.
  Do not treat B7/B8 as one migration unit.

### B9 / B10 — SSH family

- **Surface**: `SshResource` (B9); `RunShell` remote mode (B10).
- **Dispatch chain**:
  B9: `ToolCall::SshResource` → `RunnerOperation::SshResource` → `ssh.rs` →
  `spawn_piped_ssh_child` (`:1139`) → `command.spawn()` (unix `:1142`) / `ManagedChild::spawn`
  (windows `:1146`).
  B10: model tool → `RunnerOperation::PersistentShell` remote path → `remote_shell.rs:82`
  `RemotePersistentShell::spawn` → `command.spawn()` (`:109`) / `ManagedChild::spawn` (`:116`).
  Sibling in the same family: `job_manager.rs:3459` `ManagedChild::spawn` (SSH client via a Job).
- **Launch primitive**: local `ssh` / `ssh.exe` client, hardcoded executable name.
- **Current authority source**: the **remote command string is model-controllable** (via
  `shell_quote` / `remote_script`, or stdin on Windows). The local argv is not.
- **Risk**: the confinement decision that matters most happens on the **remote** host, where a
  local Seatbelt profile has no authority. A local sandbox would constrain the ssh client while
  leaving the remote execution — the actual risk — unchanged.
- **Migration difficulty**: **hard — architecturally blocked.** A local execution broker cannot
  express remote semantics. This is a different problem class (remote policy), not a migration.

---

## 2. Classification into A / B / C / D

### A. Can migrate using the existing `ExecutionBroker`

All four have: a literal program, a canonical project root already in scope, no session-lifetime
requirement, and no dependence on FD-passing or process re-attach.

| ID | Site | Why it fits the existing broker | Difficulty |
|---|---|---|---|
| **B1** | `catalog.rs:361` | literal `"git"`; `run_git_bounded` is the *same shape*; already handles truncated captures | moderate |
| **B4** | `supervisor.rs:203` | `canonical_project_root()` already in scope; `NetworkPolicy::Deny` matches the existing `process_env` intent | moderate |
| **B2** | `shell.rs:1532` | literal config program; already `env_clear()` | moderate (snapshot semantics) |
| **B3** | `shell.rs:1729` | literal config program; but authority is not in scope where the `Command` is built | moderate (needs plumbing) |

**Ordering within A: B1 → B4 → B2 → B3.** B1 first (largest risk reduction, existing prior art,
self-contained). B4 second (9 model tools, enabling authority already present). B2 third (snapshot
semantics need a decision, not just a code change). B3 last (depends on the plumbing that B1/B4 do
not need, so it carries more design surface).

### B. Requires authority threading redesign

**None.** This category is empty, and that is a finding, not an omission.

The one candidate was B3, and on re-derivation it does not need a *redesign*: `native_command`
returns a `Command` to `plugin.rs:1239`, so what is required is threading an authority parameter
from the dispatch layer to one call site inside the same crate. That is parameter plumbing, and B3
is listed in **A** with the plumbing noted.

No site requires changing what the broker *is*. `SandboxPlan` having exactly one variant
(`Confined`) is not currently a limitation for any site in A.

### C. Detached / durable architecture issue

| ID | Site | Precise blocker |
|---|---|---|
| B7 | `detached_job.rs:2391` (payload) | supervisor must outlive the runner; only the *supervisor* is unbrokerable, payload is conceivable — do not bundle |
| B8 | `detached_job.rs:2677` (payload, windows) | as B7 |
| — | `detached_job.rs:3123-3138` (supervisor) | `setsid` (`:3133`) / `CREATE_BREAKAWAY_FROM_JOB` (`:3136`): must survive runner exit; Seatbelt profiles cannot be attached to a running process; broker is stateless |
| B5 | `persistent-shell/src/lib.rs:1924` | `pre_exec` `setsid` + `dup2` FD7/8 wiring + `fcntl` (`:1932-1963`) is incompatible with the broker's `sandbox-exec` re-exec; unbounded interactive lifetime |
| B6 | `persistent-shell/src/windows.rs:269` | as B5 |

Note B5/B6 are not *durable-process* problems in the detached-job sense — they are in C because
they share the same root cause class: the process must outlive or outlive-the-orchestrator, and the
broker's one-spec-one-child model plus re-exec launcher cannot express that.

### D. Not actually model reachable

**Zero of the 10.** Every one re-verified as production and model-reachable.

However, the reverse sweep did confirm the Slice 1 *exclusion* list, which is worth recording
because "excluded" is a claim that also needs evidence:

| Site | Why excluded (re-verified) |
|---|---|
| `main.rs:2297` `into_command()` | `CONTROL_PLANE_FIXED_PROBE` — `python -I -c <constant PROBE> <module>`; allowlisted by the Slice-1 F2 guard with a count assertion |
| `shell.rs:1039` `into_command()` | `CONTROL_PLANE_FIXED_PROBE` — `node --version`, constant argv; same allowlist |
| `coding_agent.rs:1455` | **genuinely model-reachable** (`CodingAgentStart` → `dispatch.rs:399` → `run_turn` `:1421` → spawn `:1455`). Excluded from the 10 by *scope* (Slice-1 DO-NOT-TOUCH), **not** by reachability. If ACP coding-agent execution is ever in scope, this becomes an 11th site. |
| `mcp_gateway.rs:605` | `McpGatewayRequest` has **0 occurrences** in `tool_call.rs` → not a model `ToolCall` variant. `ClaudeCodeMcpConfig` is operator/tool-provider configuration. |
| `external_tools.rs:1121` | `McpConnection::spawn` (`:788` `start`) — same operator-configured MCP provider family, not a model tool surface. |
| `plugin.rs:1239` | counted once, as B3. |
| `process_command.rs:37/:42` | constructors; `:42` is reached only from the windows detached path. |
| `local_execution.rs:172` | this **is** the broker escape hatch (authority, not a bypass). |
| `process/*`, `execution_broker/mod.rs` | authority implementations. |
| `detached_job.rs:1924/:1941/:2973/:3165` | internal supervisor/watchdog, `current_exe()` + Runner-authored internal mode, `env_clear()`; not model argv. |

---

## 3. Recommended next implementation slice

**Recommendation: Slice 2 = B1 + B4 only. Two sites, both category A, both moderate.**

Rationale:

1. **Maximum risk reduction per unit of change.** B1 is the highest-severity *migratable* site
   (unfiltered environment inheritance on a path reached by `ListProjects` and by every LSP tool
   call) and it already has prior art in the same shape — `git_broker::run_git_bounded`, used by
   `project_context`, `workspace_checkpoint`, and (from Slice 1) `project_overview`.
2. **B4 covers 9 model tools with one change.** The authority input already exists; the `process_env`
   network lockdown already matches the broker's semantics.
3. **B2 and B3 each need a decision before code.** B2 changes observable snapshot content;
   B3 needs authority plumbed to a call site that does not currently receive it. Doing them in the
   same slice as B1/B4 would mix a mechanical fix with two semantic decisions.
4. **Keeping B1+B4 out of C avoids re-opening P1C.** Both are plain `SandboxPlan::Confined` +
   `ManagedChild` sites. Detached/SSH/persistent-shell stay untouched, so P1 scope is not widened.

Explicitly **not** recommended for Slice 2, and why:

- **B2** — needs a decision on what a captured profile snapshot is *allowed* to contain under
  `EnvPolicy::Minimal`. That is a policy question, not a code change.
- **B3** — needs authority threading into `native_command`; larger design surface, same category.
- **B5/B6/B7/B8/B9/B10** — category C. Not migratable by decision, not by effort.

Carry into the Slice 2 ticket as preconditions rather than discoveries:

- The `catalog.rs:562-564` non-canonical fallback must be made **fail-closed** before routing, since
  `workspace_git_plan` refuses a non-absolute root.
- `EnvPolicy::Minimal` supplies only `PATH` + `HOME`; anything git or an LSP server needs must be
  named explicitly in `env_vars`.
- A negative-control test is expected per site (Slice 1 established this pattern): prove the guard
  catches a re-introduced bare spawn, so a green result is not a false green.
- `ENV_BLOCKED` on this host (`sandbox_apply: Operation not permitted` for any profile containing a
  `(deny …)` rule) means broker-fidelity evidence cannot be produced here. As in Slice 1, that is
  recorded as `ENV_BLOCKED` and **never** counted as a pass; the structural spec assertions
  (`brokered_git_spec`-style) carry the authority claim instead.

---

## 4. Boundary statement

```
Code modified                = NONE
Implementation started       = NO
P1C started                  = NO
Approval system started      = NO
Slice 2 implementation       = NO
Detached / SSH / persistent-shell touched = NO
Prior findings reopened      = NO
```

**Not claimed:**

- That any site is safe. Migration difficulty is an engineering estimate, not a security verdict.
- That category C sites *should* be exempted. That is a policy decision for the ticket author; this
  document only states the structural blocker.
- That `coding_agent.rs:1455` is unreachable. It is reachable and excluded by scope; it becomes an
  11th site if ACP coding-agent execution is ever in scope.
- That the 10 are exhaustive for all future work. They are exhaustive for the model-tool-layer
  crates at `0701cce3`, by the method in §0.
