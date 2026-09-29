# OSS_RESEARCH_EVIDENCE.md

Evidence ledger for the research on this branch.

Every claim in `SECURITY_INVARIANTS.md`, `COMPONENT_REUSE_MATRIX.md`,
`REFERENCE_ARCHITECTURE.md`, `IMPLEMENTATION_PLAN.md`, and
`SECURITY_TEST_VECTORS.md` must be traceable to a row here.

Format:

```
SOURCE      = <repo URL>
COMMIT      = <full SHA>            (or "local worktree @ <HEAD SHA>")
FILE        = <path>
SYMBOL      = <function/type/constant>
OBSERVATION = <what was actually read, not inferred>
```

Local clones used (all read-only):

| Alias | Path | HEAD |
|---|---|---|
| `webcodex` | `~/Desktop/Projects/webcodex` | `7301186b98527cb4ebc0191f6033474e9abf7c20` |
| `codex` | `~/Desktop/Projects/oss-research/codex` | `69f7140559180269e2eb8f5be6e0c20eb37b0c85` |
| `opencode` | `~/Desktop/Projects/oss-research/opencode` | `3c893f0a166cfc433819b4eff65d2e6c7696a1c9` |
| `hermes` | `~/.hermes/hermes-agent` | `79dbb1450ec404a2240529f5554526da4cfec498` |
| `pi` | `~/Desktop/Projects/oss-research/pi-mono` | `11894012dd461232eb075bc890538b6866860a10` |

---

## 1. Project identity verification (mandatory per §6 of the brief)

Star counts are recorded as **background only** and are not used as quality
evidence.

| Project | REPO_URL | OWNER | LICENSE | LANGUAGE | SOURCE_READ_AT | SOURCE_MATCH_CONFIDENCE |
|---|---|---|---|---|---|---|
| WebCodex | `https://github.com/yyjeqhc/webcodex` | `yyjeqhc` | Apache-2.0 | Rust | commit `7301186b…` | **HIGH** |
| Codex | `https://github.com/openai/codex` | `openai` | Apache-2.0 (+`NOTICE`) | Rust | commit `69f71405…` | **HIGH** |
| OpenCode | `https://github.com/anomalyco/opencode` | `anomalyco` | MIT | TypeScript | commit `3c893f0a…` | **HIGH** — resolves from `sst/opencode`, which **redirects** |
| Hermes | `https://github.com/NousResearch/hermes-agent` | `NousResearch` | MIT | Python | local worktree commit `79dbb145…` (`v0.21.5+3172.g79dbb14`) | **HIGH** — see note below |
| Pi | `https://github.com/earendil-works/pi` | `earendil-works` | MIT | TypeScript | commit `11894012…` | **HIGH for the project, with a naming caveat — see below** |

> **Provenance note (adversarial review rounds 1, 2, and 3, CHECK 11).** Star
> counts, exact `pushedAt` timestamps, and an "active" activity judgement have all
> been **removed**. They were real observations, but (a) they were not part of
> the raw fact list handed to the reviewer, so they were not independently
> cross-checkable from the submission, and (b) star counts and recency invite the
> popularity-as-quality inference the brief forbids. What remains is the repo
> identity, the owner, the licence, the language, and the **commit actually read**
> — all of which are checkable. Note in particular that "active" was a judgement
> about upstream maintenance that the submitted facts do not establish; the
> columns now state only what was read.
>
> **Hermes provenance correction.** An earlier revision stated Hermes was
> "installed from git" on the strength of `hermes --version` reporting
> `Install method: git`. That output is not in the raw fact list and is weaker
> than it looks (it reports the installer's own method, not a provenance chain).
> What *is* independently checkable is the local worktree: `git remote -v` shows
> `origin = NousResearch/hermes-agent` plus user forks, and `git log -1` gives
> the HEAD SHA recorded above. The claim is narrowed to that.

### 1.1 Naming collisions resolved (the brief warned about these)

**`sst/opencode` → `anomalyco/opencode`.** `gh repo view sst/opencode` returned
`{"nameWithOwner":"anomalyco/opencode", ...}`. The owner has changed; the
`anomalyco` slug is the current canonical one. A naive citation of
`sst/opencode` still resolves but mis-states the owner.

**`badlogic/pi-mono` → `earendil-works/pi`.** `gh repo view badlogic/pi-mono`
returned `{"nameWithOwner":"earendil-works/pi","url":"https://github.com/earendil-works/pi", ...}`.
Both the owner and the repository name differ from the colloquial name used in
the ecosystem (many third-party extensions still say "pi coding agent" and
"pi-mono"). The package scope confirms the new identity:
`@earendil-works/pi-coding-agent`
(`pi @ 11894012 :: packages/coding-agent/examples/extensions/dirty-repo-guard.ts:8`).

**"Pi" is a crowded namespace.** A `gh search repos "pi agent"` returned, among
others, `Dicklesworthstone/pi_agent_rust` (a separate Rust implementation),
`smallnest/pigo`, `vastsa/PI-Desktop`, `abcwyc/pi-agent-desktop`,
`Pinvou/pinvou-agent`. None of these is the P1 target. The P1 target is
identified by the extension ecosystem referencing it by name, e.g.
`Dwsy/pi-session-manager` ("Related project: https://github.com/badlogic/pi-mono")
and `qualisero/awesome-pi-agent` ("…for the pi coding agent (pi-mono)").

**"GrokCode" and "zcode" were not researched.** The brief placed them in P2 and
forbade name-based guessing. `STOP_OSS_EXPANSION = YES` was reached on
P0+P1 coverage (see §7), so no identity work was spent on them. Recorded as
`IDENTITY_UNVERIFIED` and **not** used as design input.

---

## 2. WebCodex — current-state evidence

All rows: `SOURCE = https://github.com/yyjeqhc/webcodex`, `COMMIT = 7301186b98527cb4ebc0191f6033474e9abf7c20`.

### 2.1 Execution surface

| # | FILE | SYMBOL | OBSERVATION |
|---|---|---|---|
| W-01 | `crates/webcodex-core/src/runner_operation.rs:632` | `enum RunnerOperation` | Single enum covering `RunShell`, `RunProcess`, `RunScript`, `RunInternalPosixScript`, `RunSkillResource`, `Job`, `File`, `Project`, `Computer`, `Browser`, `PlanProjectValidation`, `Validation`, `Lsp`, `PersistentShell`, `McpGateway`, `PluginGateway`, `CodingAgent`, `Skill`, `SshResource`, `RunnerConfig`, `RunnerInstruction`. This is the unified execution entry. |
| W-02 | `crates/webcodex-core/src/runner_operation.rs:663` | `RunnerOperation::wire_kind` | Wire names per operation (`run_shell`, `run_process`, `run_script`, `start_job`, `start_process_job`, `start_detached_process_job`, `persistent_shell`, `mcp_gateway`, `plugin_gateway`, `coding_agent`, `ssh_resource`, …). |
| W-03 | `crates/webcodex-runner/src/webcodex_runner/dispatch.rs:104` | `run_native_shell_or_internal_search` | Dispatch entry for `RunnerShellOperation`; routes to `run_shell_with_profiles_and_execution_state`. |
| W-04 | `crates/webcodex-runner/src/webcodex_runner/shell.rs:319` | `configured_shell_command` | Builds `std::process::Command` for the configured shell. Comment: "The shell execution path owns its process tree through ManagedChild; do not add a process-group pre_exec here." |
| W-05 | `crates/webcodex-runner/src/webcodex_runner/shell.rs:1187` | `ManagedChild::spawn(...)` | Actual shell spawn site. |
| W-06 | `crates/webcodex-runner/src/webcodex_runner/shell.rs:3030` | `ManagedChild::spawn(&mut cmd)` | Second shell spawn site. |
| W-07 | `crates/webcodex-runner/src/webcodex_runner/job_manager.rs:2981,3246,3439` | `ManagedChild::spawn` | Three job spawn sites. |
| W-08 | `crates/webcodex-runner/src/webcodex_runner/detached_job.rs:2677,2973` | `ManagedChild::spawn` | Detached-job spawn sites. |
| W-09 | `crates/webcodex-runner/src/webcodex_runner/external_tools.rs:1129` | `ManagedChild::spawn` | External-tool (MCP) spawn. |
| W-10 | `crates/webcodex-runner/src/webcodex_runner/mcp_gateway.rs:622` | `ManagedChild::spawn` | MCP gateway spawn. |
| W-11 | `crates/webcodex-runner/src/webcodex_runner/plugin.rs:1239` | `ManagedChild::spawn` | Plugin spawn. |
| W-12 | `crates/webcodex-runner/src/webcodex_runner/coding_agent.rs:1465` | `ManagedChild::spawn` | Nested coding-agent (e.g. ACP) spawn. |
| W-13 | `crates/webcodex-runner/src/webcodex_runner/validation/execute.rs:53` | `ManagedChild::spawn` | Validation/build spawn. |
| W-14 | `crates/webcodex-runner/src/webcodex_runner/remote_shell.rs:116` | `ManagedChild::spawn` | Remote shell spawn. |
| W-15 | `crates/webcodex-runner/src/webcodex_runner/ssh.rs:1146` | `ManagedChild::spawn` | SSH resource spawn. |
| W-16 | `crates/webcodex-runner/src/webcodex_runner/projects/catalog.rs:371` | `ManagedChild::spawn` | Project catalog spawn. |
| W-17 | `crates/webcodex-runner/src/main.rs:2299` | `ManagedChild::spawn` | Runner main spawn. |
| W-18 | `crates/webcodex-persistent-shell/src/windows.rs:280` | `ManagedChild::spawn` | Persistent shell (Windows). |
| W-19 | `crates/webcodex-lsp/src/supervisor.rs:214` | `ManagedChild::spawn` | LSP server spawn. |
| W-20 | `crates/webcodex-browser/src/cdp.rs:385` | `ManagedChild::spawn` | Browser (CDP) spawn. |
| W-21 | `apps/desktop/src-tauri/src/webcodex/cli.rs:455` | `spawn_with_options` | Desktop spawns the CLI. |
| W-22 | `apps/desktop/src-tauri/src/process/supervisor.rs:294` | `spawn_with_options` | Desktop sidecar supervisor. |

> **CORRECTION (adversarial review round 1, CHECK 4 — a prior claim in this
> document was FALSE).**
>
> An earlier revision of this section stated: *"17 production call sites and
> **zero** direct `.spawn()` / `.output()` / `.status()` calls inside the
> runner."* The second half of that claim was **wrong**. It came from a `grep`
> invocation that silently matched nothing because of a shell glob-expansion
> failure (`--include=*.rs` under `zsh`), and a structural conclusion was
> wrongly drawn from the empty result. Re-verification with a working search
> found multiple direct `Command::spawn()` sites, including inside the runner.
>
> **`ManagedChild::spawn` is therefore a *common* path, not a single
> chokepoint.** There is **no** single-bottleneck spawn function in WebCodex
> today. This is recorded as a finding, and it changes `IMPLEMENTATION_PLAN.md`
> P1 (a normalization step is now a prerequisite).

### 2.1a Direct `Command::spawn()` sites that bypass `ManagedChild`

Non-test, non-fake sites (receiver is a `std::process::Command`):

| # | FILE | CONTEXT | Model-reachable? |
|---|---|---|---|
| W-23a | `crates/webcodex-runner/src/webcodex_runner/remote_shell.rs:109` | `#[cfg(unix)] let mut child = command.spawn()` — the **unix** remote/persistent shell path; Windows uses `ManagedChild` | **Yes** |
| W-23b | `crates/webcodex-runner/src/webcodex_runner/ssh.rs:1142` | `spawn_piped_ssh_child`: `#[cfg(unix)] { command.spawn() }`; Windows uses `ManagedChild` | **Yes** (SSH resource) |
| W-23c | `crates/webcodex-runner/src/webcodex_runner/detached_job.rs:1924` | detached supervisor, first attempt | **Yes** |
| W-23d | `crates/webcodex-runner/src/webcodex_runner/detached_job.rs:1941` | detached supervisor, Windows breakaway fallback | **Yes** |
| W-23e | `crates/webcodex-runner/src/webcodex_runner/detached_job.rs:2409` | detached payload: `payload_command.process_group(tree_pid)` then `.spawn()` — sets a process group **without** `ManagedChild` | **Yes** |
| W-23f | `crates/webcodex-persistent-shell/src/lib.rs:1965` | unix persistent shell, preceded by an unsafe `pre_exec` (setsid + fd juggling) at `:1939` | **Yes** |
| W-23g | `src/tool_runtime/helpers.rs:146` | server-side helper: `command.process_group(0)` then `command.spawn()` — process-group ownership without `ManagedChild` | **Yes** (control plane) |
| W-23h | `crates/webcodex-workspace/src/workspace_checkpoint.rs:358,398` | checkpoint helper spawn | to be classified |
| W-23i | `crates/webcodex-workspace/src/project_context.rs:598` | project-context helper spawn | to be classified |
| W-23j | `crates/webcodex-cli/src/webcodex_cli/connect/process.rs:568,655` | CLI connect | no (operator CLI) |
| W-23k | `crates/webcodex-cli/src/webcodex_cli/controller.rs:767` | CLI controller | no (operator CLI) |
| W-23l | `crates/webcodex-environment/src/{installer_unix.rs:442,upgrade.rs:2563,unified_update/installer.rs:491,process.rs:42}` | installer/upgrade | no (operator CLI) |
| W-23m | `apps/desktop/src-tauri/src/{updates/install/prepare.rs:118,162,244,325, updates/install/native.rs:127,418, platform/opener.rs:84, platform/windows.rs:43}` | desktop updater / URL opener | no (desktop app) |
| W-23n | `src/project_entry*.rs` (W-57…W-62) | tunnel/server bootstrap | no (bootstrap) |
| W-23o | `crates/webcodex-runner/src/webcodex_runner/fake_claude_mcp.rs:248`, `fake_plugin.rs:102,138,261`, `crates/webcodex-lsp/src/fake_server.rs:370`, `crates/webcodex-runner/src/webcodex_runner/validation/validation_tree_helper.rs:145` | test doubles / helpers | no (test) |

**Only two functions in the tree call `Command::spawn()` on behalf of the
process-ownership abstraction:** `crates/webcodex-process/src/unix.rs:57` and
`crates/webcodex-process/src/windows.rs:168`. Everything else is either a direct
call (above), a control-plane call, or a test.

**Consequence for the design:** the sandbox cannot be attached at one function
alone, and — corrected in review round 13, CHECK 4 — **this design does not claim
it eventually will be.** The text here previously read "only then attach at the
single function", which contradicted the two-part attach surface specified in
`REFERENCE_ARCHITECTURE.md §4` and the ≥10 unmanaged sites enumerated above. P1
must first **normalize** the model-reachable direct sites onto the managed path
(or attach the sandbox at each); the attach surface is then the *pair* {managed
path, shared hook}, and correctness depends on **coverage of all sites**, not on
the existence of one function. Convergence onto a single normalized function is a
stated goal of P0, not a precondition. The enumeration above is the input to that
normalization, and `SECURITY_TEST_VECTORS.md` CP-01/CP-02 are rewritten
accordingly.

**Residual uncertainty (stated honestly):** the classifier "model-reachable"
above is my judgement from file role and call site, not a proven call-graph
result. `W-23h`/`W-23i` are explicitly unclassified. Establishing reachability
by call graph is a P0 acceptance criterion.

### 2.1b `ManagedChild::spawn` sites (the managed path)

Re-verified enumeration of `ManagedChild::spawn` / `spawn_with_options` call
sites after excluding `*_tests.rs`, `tests/`, `fake_*`, and comment lines:

| # | FILE | Observations |
|---|---|---|
| W-05 | `crates/webcodex-runner/src/webcodex_runner/shell.rs:1187` | shell spawn |
| W-06 | `crates/webcodex-runner/src/webcodex_runner/shell.rs:3030` | shell spawn |
| W-07 | `crates/webcodex-runner/src/webcodex_runner/job_manager.rs` (3 sites) | job spawns |
| W-08 | `crates/webcodex-runner/src/webcodex_runner/detached_job.rs:2677,2973` | detached job (the *payload* at 2973 uses `ManagedChild`, unlike W-23e) |
| W-09 | `crates/webcodex-runner/src/webcodex_runner/external_tools.rs:1129` | external tool |
| W-10 | `crates/webcodex-runner/src/webcodex_runner/mcp_gateway.rs:622` | MCP gateway |
| W-11 | `crates/webcodex-runner/src/webcodex_runner/plugin.rs:1239` | plugin |
| W-12 | `crates/webcodex-runner/src/webcodex_runner/coding_agent.rs:1465` | nested agent |
| W-13 | `crates/webcodex-runner/src/webcodex_runner/validation/execute.rs:53` | validation |
| W-14 | `crates/webcodex-runner/src/webcodex_runner/remote_shell.rs:116` | **Windows** branch only |
| W-15 | `crates/webcodex-runner/src/webcodex_runner/ssh.rs:1146` | **Windows** branch only |
| W-16 | `crates/webcodex-runner/src/webcodex_runner/projects/catalog.rs:371` | project catalog |
| W-17 | `crates/webcodex-runner/src/main.rs:2299` | runner main |
| W-18 | `crates/webcodex-persistent-shell/src/windows.rs:280` | **Windows** only |
| W-19 | `crates/webcodex-lsp/src/supervisor.rs:214` | LSP |
| W-20 | `crates/webcodex-browser/src/cdp.rs:385` | browser (CDP) |
| W-21 | `apps/desktop/src-tauri/src/webcodex/cli.rs:455` | desktop → CLI |
| W-22 | `apps/desktop/src-tauri/src/process/supervisor.rs:294` | desktop sidecar |

Note the platform asymmetry: `remote_shell`, `ssh`, and `persistent_shell` use
`ManagedChild` on **Windows** and a direct `Command::spawn()` on **Unix**
(W-23a/b/f). On the platform this research targets (macOS), the direct path is
the one taken. This asymmetry is a concrete, previously-unnoticed bypass.

**Count discipline:** the earlier "17" figure is withdrawn as imprecise. The
defensible statements are: the managed path is called from **~20 non-test sites
across 9 crates/binaries**; there are **≥ 10 non-test, non-fake direct
`Command::spawn()` sites**; and **no single function is the only spawn path.**

| # | FILE | SYMBOL | OBSERVATION |
|---|---|---|---|
| W-23 | `crates/webcodex-process/src/unix.rs:43` | `ManagedChild::spawn` | `spawn_with_options` sets `command.process_group(0)` then `command.spawn()` (line 57). Comment explains `pre_exec` is deliberately avoided to preserve normal `ENOEXEC` semantics. **This is one of the two sites on the sandbox-attach surface** — the managed path; the other is the shared hook P1a introduces at the direct sites (`REFERENCE_ARCHITECTURE.md §4`, corrected in review round 15, CHECK 4). It is not *the* attach point, and no document may call it the single one. |
| W-24 | `crates/webcodex-process/src/unix.rs:99` | `terminate_tree` | `kill(-pgid, SIGKILL)`; `ESRCH` treated as idempotent success. |
| W-25 | `crates/webcodex-process/src/unix.rs:1-6` | module doc | "The owned entity is the *tree*, not just the direct child… Descendants inherit the group." |
| W-26 | `crates/webcodex-process/src/lib.rs:1-25` | crate doc | Confirms tree semantics; Windows uses `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. |
| W-27 | `crates/webcodex-process/Cargo.toml` | deps | `libc` (unix) / `windows-sys` with `Win32_System_JobObjects` (windows). No sandbox dependency. |

### 2.2 Absence of an OS sandbox

| # | Search | Observation |
|---|---|---|
| W-28 | `grep -rni "seatbelt\|sandbox-exec\|landlock\|bubblewrap\|seccomp" src crates docs` | **Zero matches.** WebCodex has no OS sandbox implementation and no reference to one. |
| W-29 | `grep -rn "pre_exec" src crates` (excluding tests) | Only process-group/`setsid`-related hooks in `persistent-shell`, `cli/connect/process.rs`, `detached_job.rs`, `ssh.rs`, `webcodex-environment/installer_unix.rs`. None is a sandbox. |

### 2.3 Policy / permission layer (already present)

| # | FILE | SYMBOL | OBSERVATION |
|---|---|---|---|
| W-30 | `src/tool_runtime/permissions/model.rs:18` | `enum AuthorityMode` | Only two modes: `TrustedAgent`, `Restricted`. `DEFAULT = TrustedAgent`. Env: `WEBCODEX_AUTHORITY_MODE`; legacy `WEBCODEX_PERMISSION_MODE`. |
| W-31 | `src/tool_runtime/permissions/model.rs:85` | `new_permission_decision` | Builds `PermissionDecision` with `request_id = format!("wc_perm_{}", random_suffix::<12>())`. |
| W-32 | `src/tool_runtime/permissions/policy.rs:64` | `EffectiveAuthorityConfig::from_env` | Resolves mode from env; unknown non-empty value ⇒ `InvalidMode`. |
| W-33 | `src/tool_runtime/permissions/policy.rs:152-165` | `human_approval_required` / `auto_authorize` | `InvalidMode` ⇒ `human_approval_required = true`, `auto_authorize = false`, with the comment "Fail closed: do not advertise frictionless auto-authorization." **This is one branch of invariant I18 — and only one. Corrected in review round 20, CHECK 1: the previous wording, "This is invariant I18 already holding", asserted the global fail-closed property from a single observed branch. `InvalidMode` is one fallible input to one classifier; a valid-but-unmodelled mode, an unmapped tool name, or a permission rule that matches nothing would not reach it, and no survey of the classifier's decision branches was performed. I18 is therefore `PARTIAL`, and this row evidences one branch, not the invariant.** |
| W-34 | `src/tool_runtime/permissions/policy.rs:192` | `decide_for_required_tool` | `TrustedAgent` ⇒ `AutoApproved` (`trusted_agent_authority`); `Restricted` ⇒ `Denied` (`restricted_requires_human_authorization`); `InvalidMode` ⇒ `Denied`. |
| W-35 | `src/tool_runtime/permissions/policy.rs:234` | `authority_profile_payload_for` | Projects `project_write/shell/git/network/package_install/service_control` all equal to `auto`, and `release = "user_task_scoped"` when auto. **Network is currently collapsed into "auto" — invariant I13 not satisfied.** |
| W-36 | `src/tool_runtime/permissions/evaluator.rs:78` | `PermissionEvaluator::evaluate` | Evaluated **once** per permission-bearing request; returns `None` when the tool class does not require permission. |
| W-37 | `src/tool_runtime/permissions/risk.rs:15` | `tool_requires_permission` | Delegates to `runtime_tool_requires_permission`. |
| W-38 | `crates/webcodex-tool-contracts/src/metadata.rs:66` | `enum ToolApprovalPolicy` | `None`, `Standard`, `InheritFromStart`, `Unknown`; `requires_permission()` is true for `Standard`/`Unknown`. |
| W-39 | `crates/webcodex-tool-contracts/src/metadata.rs:2` | `enum ToolRisk` | `Read`, `ProjectWrite`, `SkillManage`, `MemoryManage`, `CommunicationManage`, `SessionCollaborate`, `WorkflowManage`, `CheckpointManage`, `RunControl`, `ComputerControl`, `BrowserControl`, `JobRun`, `Unknown`. |
| W-40 | `crates/webcodex-tool-contracts/src/metadata.rs:43` | `enum ToolEffect` | `Observe`, `Mutate`, `Execute`, `Unknown`. |
| W-41 | `crates/webcodex-tool-contracts/src/metadata.rs:150` | `struct ToolMetadata` | Includes `authority: ToolAuthorityPolicy`, `destructive`, `shell_like`. |
| W-42 | `src/tool_runtime/permissions/mod.rs:124` | `is_hard_denied_output` | Hard-deny classification. Structured kinds: `policy_rejected`, `session_guard_denied`, `unknown_session_id`, `session_project_mismatch`, `confirmation_required`, `job_not_found`, `job_project_mismatch`, `job_stop_forbidden`. **Plus substring matches on error prose**: `sensitive path`, `sensitive artifact path`, `path must be project-relative`, `path cannot contain parent traversal`, `absolute paths are not allowed`, `path traversal`. Comment: "Independent of authority mode: auto-authorization must never suppress these outcomes." **Invariant I9's result-classifier half is observed; the pre-effect floor is not. Corrected in review round 20, CHECK 8: the previous wording, "Invariant I9 holds", dropped the qualification this same row depends on. I9 states there is no central pre-effect floor, and a post-hoc classifier cannot be one — round 15 already ruled that a guaranteed kill is not a floor. This row evidences a hard-deny classifier operating on results; it does not evidence the invariant, and must not be cited for the pre-effect half. I7's prose dependence remains the gap.** |
| W-43 | `src/tool_runtime/permissions/mod.rs:53` | `evaluate_permission_for_tool` | Gate entry; `None`/`InheritFromStart` bypass the evaluator. |
| W-44 | `src/tool_runtime/permissions/mod.rs:156` | `permission_summary_from_events` | Counters for `auto_approved`, `approved`, `denied`/`expired`, `requested`, `hard_denied`. |
| W-45 | `crates/webcodex-core/src/workflow_session_contract.rs:503` | `enum PermissionOutcome` | `AutoApproved`(`auto_approved`), `Approved`(`approved`), `Denied`(`denied`/`expired`), `Pending`(`requested`/`pending`), `HardDenied`(`hard_denied`). **`Approved`/`Pending` exist as data-model values only — no runtime producer.** |
| W-46 | `crates/webcodex-core/src/workflow_session_contract.rs:538` | `struct PermissionDecision` | Fields: `required`, `policy`, `request_id`, `status`, `reason`, `risk`, `tool_name`, `project`. |
| W-47 | `src/tool_runtime/dispatch.rs:1886` | `evaluate_permission_for_tool(...)` | Gate call site #1. |
| W-48 | `src/tool_runtime/coding_task.rs:689,815` | `evaluate_permission_for_tool(...)` | Gate call sites #2, #3. |
| W-49 | `src/tool_runtime/specialized.rs:529` | `permission_execution_denied_result` | Gate call site #4 (specialized gateways). |

### 2.4 Absence of an approval flow (correction to the prior baseline)

| # | FILE | SYMBOL/QUOTE | OBSERVATION |
|---|---|---|---|
| W-50 | `docs/agent/permission-model.md` §2 | "Consequential runtime tools are **denied**… There is no separate Connector command-approval queue or host-side task approval namespace." | `restricted` is **deny-only**. There is no queue. |
| W-51 | `docs/agent/permission-model.md` §8 | "No approval UI or notification system for `trusted_agent`." / "No multi-person approval, RBAC, or distributed policy engine." | Approval UI is an explicit non-goal. |
| W-52 | `docs/agent/architecture-decisions.md:375` | "core execution cannot depend on Apps, MCP Tasks, MRTR, **elicitation**, progress extensions, or iframe state." | Elicitation is excluded by standing architecture decision. |
| W-53 | `src/mcp/protocol.rs:106` | `legacy_initialize_payload` | Advertises only `capabilities: { tools: { listChanged: false } }`. No elicitation, no prompts. |
| W-54 | `src/mcp/protocol.rs:14` | `MCP_INFO_METHODS` | `server/discover`, `initialize`, `ping`, `tools/list`, `tools/call`, `resources/list`, `resources/read`, `notifications/initialized`. No elicitation method. |
| W-55 | `src/mcp/protocol.rs:49` | `request_client_capabilities` | Client capabilities are parsed from `_meta` but no gate depends on them. |
| W-56 | `crates/webcodex-cli/src/lib.rs` (command literals) | `activate, agent-token(s), api, auth, bearer, check, client, connect, controller, create, create-local, describe, desktop, disconnect, doctor, enroll, environment, generate, init, install, install-service, linux, list, login, logout, logs, macos, managed-oauth, oauth, openai, ops, pairing, plugin, project(s), register, reload, restart, root, run, runner(s), runner-tokens, server, setup, share, shared-key, single-user, smoke-preflight, start, status, stop, system, token(s), tunnel, uninstall, user, webcodex` | **No `approve` / `deny` command exists.**

> **Correction to the prior baseline.** `WEBCODEX_EVALUATION.md` described a
> `webcodex task approve/deny <id>` entry point for the observed
> `permission.request_id`. That CLI does not exist in this revision. The
> observation "restricted rejects shell and emits a permission request" remains
> valid; the inferred *approval entry point* does not. This branch treats
> `CHATGPT_WEB_APPROVAL_TRANSPORT = UNSUPPORTED` as the source-backed position.

### 2.5 Control-plane spawns (reachability *intended* not model-facing, not proven)

> Heading corrected in review round 4. These paths are **intended** to be
> operator-initiated, but reachability from a tool call has **not** been proven
> for all of them. In particular `src/tool_runtime/helpers.rs:146` lives in
> server-side tool-handling code and was classified model-reachable in §2.1a.
> The claim "not model-reachable" must not be made categorically for `src/`
> until the P0 spawn inventory and the CP-04 manifest test establish it.

| # | FILE | OBSERVATION |
|---|---|---|
| W-57 | `src/project_entry.rs:423,800,884,862,897` | `git` inventory read; `webcodex-server` and `webcodex-runner` process launch. |
| W-58 | `src/project_entry_share.rs:1037,1047` | Cloudflare tunnel binary launch. |
| W-59 | `src/project_entry_cloudflared.rs:127,133,651,656,715` | `cloudflared`/`tar` launch and `--version` probe. |
| W-60 | `src/project_entry_openai_tunnel.rs:99,119,174,187,227,244,772,775` | Tunnel client launch and verify. |
| W-61 | `src/project_entry_client_handoff.rs:114,126,159,181` | Client handoff launches. |
| W-62 | `src/project_entry_setup.rs:774,778` | `git` execution during setup. |
| W-63 | `src/server_listener.rs:370` | Listener-side process probe. |

**What these rows do and do not establish (corrected in review round 5,
CHECK 11).** An earlier revision concluded "These paths are reachable only from
CLI/bootstrap flows, not from the MCP tool surface." The observations above
**do not** establish that. What they establish is only *where the spawns are* and
*what they launch*. Specifically:

- W-57…W-62 are in `src/project_entry*.rs`, whose call sites are bootstrap /
  connect flows — **consistent with** operator-only reachability, not proof of it.
- W-63 (`src/server_listener.rs:370`) is in the listener path.
- None of these rows was traced to a tool-dispatch call graph.

Therefore the correct statement is: **reachability from a tool call is
unestablished for all of W-57…W-63**, and establishing or refuting it is a P0
task (`IMPLEMENTATION_PLAN.md` P0 criterion 1) verified by the CP-04 manifest
test. The rows are recorded so the surface is explicit rather than assumed — not
to assert that it is safe.

---

## 3. Codex — sandbox, approval, auto-review

All rows: `SOURCE = https://github.com/openai/codex`, `COMMIT = 69f7140559180269e2eb8f5be6e0c20eb37b0c85`.

### 3.1 Sandbox

| # | FILE | SYMBOL | OBSERVATION |
|---|---|---|---|
| C-01 | `codex-rs/sandboxing/Cargo.toml` | deps | `codex-mxc-sandbox`, `codex-network-proxy`, `codex-protocol`, `codex-uds`, `codex-utils-absolute-path`, `codex-utils-path-uri`, `codex-utils-pty`, `codex-windows-sandbox`, `dunce`, `libc`, `serde_json`, `regex-lite`, `tokio`, `tracing`, `url`, `which`. Deep internal coupling. |
| C-02 | `codex-rs/sandboxing/src/lib.rs:1-56` | module list + re-exports | `seatbelt` (macOS), `landlock` + `bwrap` (Linux), `windows`/`windows_mxc`; exports `SandboxManager`, `SandboxType`, `spawn_process`, `SandboxViolationEvent`, `get_platform_sandbox`. |
| C-03 | `codex-rs/sandboxing/src/manager.rs:49` | `get_platform_sandbox` | macOS ⇒ `SandboxType::MacosSeatbelt`; Linux ⇒ `SandboxType::LinuxSeccomp`; Windows ⇒ optional `WindowsRestrictedToken`. |
| C-04 | `codex-rs/sandboxing/src/manager.rs:352` | `SandboxManager::transform` | Builds argv from `command.program` + `command.args`, then **rewrites** it per `SandboxType`. `None` ⇒ unchanged. |
| C-05 | `codex-rs/sandboxing/src/manager.rs:434-471` | macOS arm | Calls `create_seatbelt_command_args_with_profile`, then prepends `MACOS_PATH_TO_SEATBELT_EXECUTABLE` to argv. |
| C-06 | `codex-rs/sandboxing/src/seatbelt.rs:62` | `MACOS_PATH_TO_SEATBELT_EXECUTABLE` | `"/usr/bin/sandbox-exec"`. |
| C-07 | `codex-rs/sandboxing/src/seatbelt.rs:288-300` | unix-socket rules | `(allow network-bind (local unix-socket))`, `(allow network-outbound (remote unix-socket))`, plus `subpath`-parameterised variants. |
| C-08 | `codex-rs/sandboxing/src/seatbelt.rs:336-346` | network policy | `(allow network-bind (local ip "*:*"))`, `(allow network-outbound (remote ip "localhost:*"))`, `(allow network-outbound (remote ip "*:53"))`, `(allow network-outbound (remote ip "localhost:{port}"))`. **Network is an explicit, separate axis.** |
| C-09 | `codex-rs/sandboxing/src/seatbelt.rs:374` | open-network branch | `(allow network-outbound)\n(allow network-inbound)\n` when network is unrestricted. |
| C-10 | `codex-rs/sandboxing/src/seatbelt.rs:871` | `create_seatbelt_command_args` | Public argv construction. |
| C-11 | `codex-rs/sandboxing/src/seatbelt.rs:988,1059` | policy sections | `"; allow read-only file operations\n(allow file-read*)"`, `(allow file-read* (subpath "/Applications"))`. |
| C-12 | `codex-rs/sandboxing/src/bwrap.rs` | `find_system_bwrap_in_path` / `system_bwrap_warning` | Linux prefers a system `bubblewrap` binary; a warning is surfaced when absent. |
| C-13 | `codex-rs/linux-sandbox/Cargo.toml` | deps | `landlock`, `seccompiler`, `rustix`, `globset`, `codex-process-hardening`; separate `[[bin]] codex-linux-sandbox`. |
| C-14 | `codex-rs/sandboxing/src/unix` behaviour | process group | (Parallel to WebCodex) tree ownership exists; Codex combines it with the sandbox at the same spawn. |
| C-15 | size | LOC | `seatbelt.rs` 1125, `manager.rs` 816, `violation.rs` 300, `spawn.rs` 142 ⇒ **2,383 LOC** before transitive deps. |
| C-16 | `codex-rs/protocol/src/models.rs:422` | `enum PermissionProfile` | `Managed { file_system: ManagedFileSystemPermissions, network: NetworkSandboxPolicy }` / `Disabled` / `External { network }`. |
| C-17 | `codex-rs/protocol/src/models.rs:410-416` | built-in profiles | `:read-only`, `:workspace`, `:danger-full-access`. |
| C-18 | `codex-rs/protocol/src/models.rs:471-480` | `Default for PermissionProfile` | `Managed { file_system: Restricted{entries: []}, network: Restricted }`. |
| C-19 | `codex-rs/sandboxing/src/manager.rs:331` | `should_sandbox` | `SandboxablePreference::{Forbid, Require, Auto}`. |

### 3.2 Reuse feasibility

| # | CHECK | OBSERVATION |
|---|---|---|
| C-20 | `GET https://crates.io/api/v1/crates/codex-sandboxing` | `{"errors":[{"detail":"crate 'codex-sandboxing' does not exist"}]}` ⇒ **not a published library.** |
| C-21 | `GET https://crates.io/api/v1/crates/codex-protocol` | Exists, `default_version = "0.63.0"`, created `2025-12-11`, 1667 lifetime downloads, 1 version ⇒ **stale snapshot.** |
| C-22 | `codex-rs/Cargo.toml` | Workspace members use `version.workspace = true` + path deps ⇒ internal workspace, not a public API. |
| C-23 | `LICENSE` | Apache-2.0. |
| C-24 | `NOTICE` | "OpenAI Codex / Copyright 2025 OpenAI"; includes MIT-derived Ratatui code. Not itself a blocker to vendoring; the engineering surface is. |

**Conclusion recorded in `COMPONENT_REUSE_MATRIX.md §2` (scoped per option;
corrected in review rounds 1–2):** `DIRECT_DEPENDENCY via crates.io` and
`VENDOR_SUBSET of the whole crate` are `NOT_PRACTICAL` (**assessed**);
`SUBPROCESS_ADAPTER` + a small profile compiler is the recommended route. The
**pinned-git dependency** and **bounded vendor subset** options were **NOT
ASSESSED** and are not closed by the above. No blanket `NOT_PRACTICAL` verdict is
claimed.

### 3.3 Approval

| # | FILE | SYMBOL | OBSERVATION |
|---|---|---|---|
| C-25 | `codex-rs/protocol/src/protocol.rs:986` | `enum AskForApproval` | `UnlessTrusted` (`untrusted`), `OnRequest` (default, alias `on-failure`), `Granular(GranularApprovalConfig)`, `Never`. |
| C-26 | `codex-rs/protocol/src/protocol.rs:1012` | `struct GranularApprovalConfig` | `sandbox_approval`, `rules`, `skill_approval`, `request_permissions`, **`mcp_elicitations`**. Codex supports MCP elicitation as an approval channel — WebCodex does not (§2.4). |
| C-27 | `codex-rs/protocol/src/protocol.rs:4159` | `enum ReviewDecision` | `Approved`, `ApprovedExecpolicyAmendment{proposed_execpolicy_amendment}`, `ApprovedForSession`, `ApprovedMcpPolicyAmendment`, `NetworkPolicyAmendment{network_policy_amendment}`, `Denied{rejection}`, `TimedOut`, `Abort`. |
| C-28 | `codex-rs/protocol/src/protocol.rs:4198` | `impl Default for ReviewDecision` | `Denied { rejection: "denied" }` ⇒ **fail closed by default.** |
| C-29 | `codex-rs/protocol/src/protocol.rs:4211` | `to_opaque_string` | Opaque, PII-free serialization for some surfaces. |
| C-30 | `codex-rs/execpolicy/src/decision.rs:9` | `enum Decision` | `Allow`, `Prompt`, `Forbidden` (parse from `"allow"`/`"prompt"`/`"forbidden"`). **The ALLOW/ASK/DENY triad.** |
| C-31 | `codex-rs/execpolicy/src` | files | `policy.rs`, `rule.rs`, `parser.rs`, `amend.rs`, `sandbox_migration.rs`, `executable_name.rs`, `execpolicycheck` binary. Rule amendment is an explicit approval outcome (C-27). |

### 3.4 Auto review (Guardian)

| # | FILE | SYMBOL | OBSERVATION |
|---|---|---|---|
| C-32 | `codex-rs/core/src/guardian/` | dir | `approval_request.rs`, `coverage.rs`, `decision.rs`, `feedback.rs`, `input_budget.rs`, `permissions.rs`, `prompt.rs`, `request_budget.rs`, `review.rs`, `review_request.rs`, `review_session.rs`, `review_session_setup.rs`, `reviewer_config.rs`, `runtime.rs`, … 9527 LOC incl. tests. |
| C-33 | `codex-rs/core/src/guardian/decision.rs:44` | `decide_approval` doc | "`None` requests the existing user flow. **No contributor is never an implicit allow.**" |
| C-34 | `codex-rs/core/src/guardian/decision.rs:17` | `spawn_approval_decision` | Runs the reviewer on a dedicated thread (`"codex-approval-review"`), off the main turn. |
| C-35 | `codex-rs/core/src/guardian/decision.rs:55-77` | inputs | Uses `history_reset`, `config`, `requirements`, `context.model_info`, `approval_policy`, `approvals_reviewer`, `full_access`, `escalated_exec`, `retried`. |
| C-36 | `codex-rs/core/src/guardian/reviewer_config.rs:32` | `build_guardian_review_session_config` | Resolves a **separate reviewer model** and injects host-controlled policy instructions (`GuardianPolicyInstructions`, `guardian_output_contract_prompt`) ⇒ actor/reviewer separation. |
| C-37 | `codex-rs/core/src/guardian/reviewer_config.rs:71` | `resolve_review_model` | Same reviewer catalog used for approvals and checkpoint migration. |
| C-38 | `codex-rs/guardian-context/src/lib.rs:1-12` | crate doc | "Shared context sections for synchronous **Guardian review** and asynchronous **scoring**… Sections preserve source-specific evidence and share prompt framing, while profiles retain the consumer-specific transcript policy." |
| C-39 | `codex-rs/guardian-context/src/action.rs` | `PlannedAction`, `action_for_review` | The reviewer receives a structured planned action plus transcript sections. |
| C-40 | `codex-rs/ext/guardian-reviewer/` | crate `codex-guardian-reviewer` | The reviewer is an **extension**, not core: `review.rs`, `assessment.rs`, `execution.rs`, `outcome.rs`, `routing.rs`, `deadline.rs`, `pool.rs`, `settings.rs`, `retry_tests.rs`, `circuit_breaker_tests.rs`. Deps include `codex-extension-api`, `codex-prompts`, `codex-protocol`, `codex-mcp`, `codex-otel`, `codex-analytics`, `codex-feedback`. |
| C-41 | `codex-rs/ext/guardian-v2/` | dir | Feature-flagged (`features/src/feature_configs.rs:87-96 GuardianV2TranscriptSource`, `GuadianV2ConfigToml`) ⇒ incremental rollout of the reviewer. |
| C-42 | `codex-rs/ext/extension-api/src/contributors/` | files | `approval_review.rs`, `tool_lifecycle.rs`, `tool_policy.rs`, `turn_admission.rs`, `session_isolation.rs`, `world_state.rs`, `skill_invocation.rs`, `mcp.rs` ⇒ the extension point vocabulary. |

---

## 4. OpenCode — permission engine

All rows: `SOURCE = https://github.com/anomalyco/opencode`, `COMMIT = 3c893f0a166cfc433819b4eff65d2e6c7696a1c9`.

| # | FILE | SYMBOL | OBSERVATION |
|---|---|---|---|
| O-01 | `packages/schema/src/permission.ts` | `Effect` | `Schema.Literals(["allow", "deny", "ask"])`. |
| O-02 | `packages/schema/src/permission.ts` | `Rule` / `Ruleset` | `Rule = { action: string, resource: string, effect: Effect }`; `Ruleset = Rule[]`. |
| O-03 | `packages/schema/src/permission.ts` | `Reply` | `Schema.Literals(["once", "always", "reject"])`. |
| O-04 | `packages/schema/src/permission.ts` | `Request` | `{ id, sessionID, action, resources[], save[], metadata?, source? }`; `ID` is branded, prefixed `per_`. |
| O-05 | `packages/schema/src/permission.ts` | `Event` | `permission.v2.asked`, `permission.v2.replied`. |
| O-06 | `packages/core/src/permission.ts:15` | `missingAgentPermissions` | `[{ action: "*", resource: "*", effect: "deny" }]` ⇒ an agent with no configured permissions is **denied** by default. |
| O-07 | `packages/core/src/permission.ts:76` | `evaluate(action, resource, ...rulesets)` | `.flat().findLast(match) ?? { action, resource: "*", effect: "ask" }` ⇒ **last match wins**; default is `ask`. |
| O-08 | `packages/core/src/permission.ts:147` | `denied(input, rules)` | Fast path: any resource evaluating to `deny` ⇒ denied. |
| O-09 | `packages/core/src/permission.ts:155` | `evaluateInput` | Configured rules first; then `[...rules, ...savedRules]`; per-resource effects; precedence `deny > ask > allow`. |
| O-10 | `packages/core/src/permission.ts:190` | `ask` | Creates a pending request only when the effect is `ask`. |
| O-11 | `packages/core/src/permission.ts:197` | `assert` | `deny` ⇒ `BlockedError{rules}`; `allow` ⇒ return; `ask` ⇒ create + await deferred. |
| O-12 | `packages/core/src/permission.ts:220-247` | `reply` (`reject`) | Fails the deferred with `DeclinedError`/`CorrectedError` and **cascades `reject` to all other pending requests in the same session**. |
| O-13 | `packages/core/src/permission.ts:250-256` | `reply` (`always`) | Persists via `saved.add({ projectID, action, resources: request.save })` – only the explicitly listed `save[]` resources. |
| O-14 | `packages/core/src/permission.ts:261-283` | `reply` (`always`) | Retroactively resolves other pending requests that are now `allow`. |
| O-15 | `packages/core/src/permission.ts:119-129` | finalizer | On scope teardown, all pending deferreds are failed with `DeclinedError` ⇒ fail closed. |
| O-16 | `packages/core/src/util/wildcard.ts` | `match` | Escapes regex metachars, `*`→`.*`, `?`→`.`, anchors `^…$`, `" .*"`→`"( .*)?"`; case-insensitive on Windows. **Pure string matching — no path canonicalization.** |
| O-17 | `packages/core/src/permission/saved.ts` | `PermissionSaved` | `{ projectID, action, resources }` persisted to a DB table; `list` filters by `project_id`; `add` uses `onConflictDoNothing`. |
| O-18 | `packages/core/src/permission/sql.ts` | `PermissionTable` | SQL row shape (`id`, `project_id`, `action`, `resource`). |
| O-19 | `packages/opencode/src/session/tools.ts:87` | `context.ask` | Tool context wires `permission.ask({…, ruleset: Permission.merge(agent.permission, session.permission)})`. |
| O-20 | `packages/opencode/src/session/tools.ts:~105` | `plugin.trigger` | `"tool.execute.before"` (gets `{args}`, may mutate) and `"tool.execute.after"` (gets output). |
| O-21 | `packages/opencode/src/session/llm.ts:149` | `Permission.merge` | Merges agent + session permission rulesets. |
| O-22 | `packages/opencode/src/permission.ts` (v1) | `Permission.disabled` | Legacy v1 surface still used by `session/system.ts:108`. |
| O-23 | whole repo | sandbox | **No OS sandbox.** Confirms "OpenCode = policy engine only". |
| O-24 | `LICENSE` | MIT | "Copyright (c) 2025 opencode". |

---

## 5. Hermes — approval state machine, transport, floors

All rows: `SOURCE = https://github.com/NousResearch/hermes-agent`, `COMMIT = 79dbb1450ec404a2240529f5554526da4cfec498` (`v0.21.5+3172.g79dbb14`, local worktree).

| # | FILE | SYMBOL | OBSERVATION |
|---|---|---|---|
| H-01 | `hermes_cli/approval_mode.py:19` | `VALID_APPROVAL_MODES` | `("manual", "smart", "off")` ⇒ the three modes the brief asked about exist. |
| H-02 | `hermes_cli/approval_mode.py:1-6` | module doc | "Approval mode is **profile-scoped configuration, not conversation state**… It must not rebuild a live agent or mutate its system prompt/tool schema, preserving the prompt-cache prefix." ⇒ **the setting is not conversation state** (the comment's own claim, and the only part the evidence supports). **The narrower conclusion — that an agent cannot change it — is NOT established by this row and was withdrawn in review round 23, CHECK 8.** The comment describes *where the configuration lives* (a profile, not the transcript), which is a statement about the read path; it says nothing about whether a process holding the profile could rewrite it, and Hermes' own control plane is exactly such a holder. The round-22 reviewer was right that agent-wide inability does not follow. **The residue "does not leak into the prompt" is withdrawn as well (review round 24, CHECK 8).** A module comment that says approval mode is profile-scoped and that the implementation *must not* mutate the system prompt or tool schema establishes **a stated intent on the part of that module**; it does not establish the **absence of every prompt projection**, which is a whole-codebase property. Asserting absence from a single module's doc comment is the same move as `H-02`'s first overclaim, one sentence later: a bounded source read as a universal. **What the row supports is exactly what the comment says and no more:** (i) approval mode is **profile-scoped configuration, not conversation state** — a claim about where the value lives, evidenced by the module's own description of itself; and (ii) the module's author **states an intent** not to rebuild a live agent or mutate the prompt cache prefix. (ii) is an **intent, recorded, not a verified property** — no observation in `§2` indexes a test or trace establishing that no prompt projection occurs. ****The design-relevant half that survives, and the only one the *plan* reuses, is (i)** — **and the pointer is corrected: rounds 24 and 25 both named `REFERENCE_ARCHITECTURE.md §3`, and §3 is the trust-boundary map, not the approval-mode discussion. The correct targets are `REFERENCE_ARCHITECTURE.md §5.1` (the post-effect classification this row's cache-preservation property is an argument within) and the plan's own P5 mode-lifetime discussion; `IMPLEMENTATION_PLAN.md` P0/P1 and `SECURITY_TEST_VECTORS.md` CP-04 discuss the surrounding gaps. The claim that survives:** **This row supports (i) and nothing further, and the plan's prompt-cache conclusion rests on (i) ALONE, deliberately, without borrowing strength from the unassessed part.** The plan's position is: approval mode is a value the plan never places in conversation state, so *within this plan* it does not become a per-turn invalidation input — a statement about the plan's own design, derived from where the value lives. It is **not** a claim that (a) Hermes has no prompt projection, (b) any agent is incapable of mutating a cache prefix, or (c) a profile-scoped value is invisible to a cache. (a) is unassessed by this evidence; (b) is precisely the claim withdrawn in round 23 and is not re-asserted here; and (c) is **not supported by the observed comment** — the comment describes *where the configuration lives* (a profile, not the transcript), which is a statement about the read path. Whether a profile-scoped value can still appear in a prompt is a separate property that no observation in `§2` establishes either way. Stating (c) as supported would repeat the bounded-read-as-universal error one clause after withdrawing it. . **[The observation cell's original trailing clause — "and does not leak into the prompt" — is struck here in round 26, not merely withdrawn in the explanation below. Round 25 appended the withdrawal and left the sentence standing in the cell, which is the repair-beside-a-claim pattern in its most literal form: the cell is what a reader cites, and a reader citing the cell would have cited the withdrawn claim.]** |
| H-03 | `hermes_cli/approval_mode.py:53-61` | `set_config_value("approvals.mode", …)` | Managed policy causes `SystemExit` ⇒ an operator-managed value **cannot** be overridden. |
| H-04 | `hermes_cli/approval_mode.py:66-71` | effective-check | If the requested value does not become effective, the mode stays and the change is reported as failed. |
| H-05 | `hermes_cli/approval_transport.py:27` | `ApprovalChoice` | `Literal["once", "session", "always", "deny"]` ⇒ all four grant choices exist. |
| H-06 | `hermes_cli/approval_transport.py:1-6` | module doc | "Transports only present an immutable, redacted request and return a correlated human decision. They **do not participate in command detection or authorization policy**. The host validates scope, request binding, and timeout fail-closed." ⇒ explicit Policy ≠ Transport separation. |
| H-07 | `hermes_cli/approval_transport.py:55-74` | `ApprovalRequest.create` | Builds `choices` from `allow_session`/`allow_permanent`; `request_id = uuid4().hex`; `digest = sha256(canonical json incl. session_key)`; `timeout_seconds` default `300`. |
| H-08 | `hermes_cli/approval_transport.py:31-38` | `ApprovalDecision` | `{ request_id, request_digest, choice }` ⇒ a decision is bound to one exact request. |
| H-09 | `hermes_cli/approval_transport.py:99-101` | `_deny` | Helper that normalises any failure into `ApprovalTransportResult("deny", failure)`. |
| H-10 | `hermes_cli/approval_transport.py:114-116` | worker slots | `BoundedSemaphore(8)`; capacity exhaustion ⇒ `_deny("busy")`. |
| H-11 | `hermes_cli/approval_transport.py:121-144` | `_run` | Runs sync or async callback on a **daemon thread**, `asyncio.run` on that worker, "never on a gateway or TUI event loop". `BaseException` ⇒ `error`. |
| H-12 | `hermes_cli/approval_transport.py:145-161` | wait loop | Polls `is_interrupted`; enforces `deadline`; on expiry ⇒ `_deny("timeout")`; on interrupt ⇒ `_deny("interrupted")`. |
| H-13 | `hermes_cli/approval_transport.py:167-185` | `_validate_decision` | Failure codes: `timeout` (completed after deadline — "late results are discarded and cannot authorize another request"), `error`, `invalid` (wrong type), `stale` (`request_id` or `digest` mismatch), `invalid` (choice ∉ `allowed_choices`). **All six failure paths return DENY.** |
| H-14 | `tools/approval_floors.py:1-8` | module doc | "Pre-gate floors… decisions that never reach a prompt. Unconditional blocks (hardline, `sudo -S` password piping, the user's own `approvals.deny` globs) and the permanent command allowlist match. All of them run **BEFORE** yolo / `approvals.mode: off` / cron approve-mode; the allowlist runs after." |
| H-15 | `tools/approval_floors.py:23-42` | `_match_user_deny_rule` | User `approvals.deny` fnmatch globs "block unconditionally — like the hardline floor, a match fires BEFORE the yolo / mode=off bypass ("never let the agent run this, even under yolo")… run over the same normalized/deobfuscated variants the dangerous-pattern detector uses so quoting tricks (`r\m`, `git st""atus`) can't sidestep a rule." |
| H-16 | `tools/approval_floors.py:45-51` | `_user_deny_block_result` | Message: "It cannot be executed via the agent — not even with `--yolo`, `/yolo`, or `approvals.mode=off`." ⇒ invariant I9 reference implementation. |
| H-17 | `tools/approval.py:249` | `approve_session(session_key, pattern_key)` | Session grant keyed by `(session_key, pattern_key)`. |
| H-18 | `tools/approval.py:350` | `is_approved(session_key, pattern_key)` | Grant lookup. |
| H-19 | `tools/approval.py:366,372` | `approve_permanent(pattern_key)` / `load_permanent` | Permanent (persisted) grants keyed by pattern. |
| H-20 | `tools/approval.py:448` | `save_permanent_allowlist(patterns)` | Persistence of `always` grants. |
| H-21 | `tools/approval.py:285` | `clear_session(session_key)` | Session-end revocation. |
| H-22 | `tools/approval.py:122,138,175,209,219,225` | `register_gateway_notify`, `resolve_gateway_approval`, `withdraw_gateway_approval`, `ack_gateway_approval`, `has_blocking_approval`, `pending_gateway_approval_count` | The approval transport surface: notify, resolve, withdraw, ack, and pending accounting. |
| H-23 | `tools/approval.py:585,618,637` | `_Unattended`, `_unattended_contexts`, `_unattended_deny` | Unattended (cron/scheduled) contexts have their own deny path. |
| H-24 | `tools/approval.py:761,793,1052` | `_smart_gate`, `_human_decision`, `_floor_block` | `smart` mode gate; human decision path; floor blocking. |
| H-25 | `tools/approval.py:1078,1168,1242` | `check_dangerous_command`, `check_all_command_guards`, `check_execute_code_guard` | Guard entry points. |
| H-26 | `tools/approval.py:68-93` | `_get_denial_breaker_threshold`, `_record_denial`, `_reset_denials`, `_denial_breaker_addendum` | Repeated denials trip a breaker (anti-loop). |
| H-27 | `tools/path_security.py:8` | `validate_within_dir` | `path.resolve().relative_to(root.resolve())` with `ValueError/OSError` ⇒ "Path escapes allowed directory" ⇒ symlinks and `..` followed. |
| H-28 | `tools/path_security.py:19` | `has_traversal_component` | Cheap literal `..` pre-check before full resolution. |
| H-29 | `tools/path_security.py:26-37` | `_UNSAFE_PATH_CHARS`, `has_unsafe_path_chars` | Rejects control chars, NEL/LS/PS because they "corrupt line-delimited protocols… and forge log lines". |
| H-30 | `LICENSE` | MIT | "Copyright (c) 2025 Nous Research". |

---

## 6. Pi — extension ecosystem

All rows: `SOURCE = https://github.com/earendil-works/pi` (resolves from `badlogic/pi-mono`), `COMMIT = 11894012dd461232eb075bc890538b6866860a10`.

| # | FILE | SYMBOL | OBSERVATION |
|---|---|---|---|
| P-01 | `packages/agent/src/types.ts:66` | `BeforeToolCallResult` | `{ block?: boolean; reason?: string; terminate?: boolean }`. Doc: "Returning `{ block: true }` prevents the tool from executing… If omitted, a default blocked message is used." **The hook can only block, not allow.** |
| P-02 | `packages/agent/src/types.ts:103` | `BeforeToolCallContext` | `{ assistantMessage, toolCall, args, context }` — the hook sees the validated arguments. |
| P-03 | `packages/agent/src/types.ts:89` | `AfterToolCallResult` | Field-level overrides for `content`, `details`, `isError`, `usage`, `terminate`; no deep merge. |
| P-04 | `packages/agent/src/agent.ts:123,201,241,480` | `beforeToolCall?` | Optional per-run hook threaded into the agent loop. |
| P-05 | `packages/agent/src/agent-loop.ts:722` | hook invocation | Loop calls `config.beforeToolCall(...)` before execution. |
| P-06 | `packages/coding-agent/src/core/agent-session.ts:533` | `_installAgentToolHooks` | `this.agent.beforeToolCall = async ({toolCall, args}) => runner.emitToolCall({type:"tool_call", toolName, toolCallId, input})`; the catch clause throws `Extension failed, blocking execution: …` ⇒ **a failing extension blocks.** |
| P-07 | `packages/coding-agent/src/core/agent-session.ts:551` | `afterToolCall` | `runner.emitToolResult({...})`; result content may be replaced by the extension. |
| P-08 | `packages/coding-agent/examples/extensions/dirty-repo-guard.ts` | `checkDirtyRepo` | Uses `pi.exec("git", ["status","--porcelain"])`; "if (!ctx.hasUI) { // In non-interactive mode, block by default return { cancel: true } }" ⇒ **non-interactive defaults to block.** |
| P-09 | `packages/coding-agent/examples/extensions/timed-confirm.ts` | `ctx.ui.confirm(…, { timeout: 5000 })` | Confirm dialog with a timeout; auto-cancel ⇒ falsy ⇒ cancelled. Reference for the local approval UI. |
| P-10 | `packages/coding-agent/examples/extensions/` | file list | Security-relevant examples: `dirty-repo-guard.ts`, `project-trust.ts`, `timed-confirm.ts`, `ssh.ts`, `gondolin/`, `debug-provider.ts`, `commands.ts`. |
| P-11 | `packages/coding-agent/examples/extensions/dirty-repo-guard.ts:8` | import | `from "@earendil-works/pi-coding-agent"` ⇒ confirms the renamed package scope. |
| P-12 | `packages/` | dirs | `agent`, `ai`, `chord`, `client`, `coding-agent`, `durable`, `evals`, `protocol`, `server`, `session-backends`, `telemetry`, `tui`. |
| P-13 | `LICENSE` | MIT | "Copyright (c) 2025 Mario Zechner". |
| P-14 | extension ecosystem (informational) | `gh search repos "pi coding agent"` | Notable extensions: `agegr/pi-web`, `nicobailon/pi-mcp-adapter`, `svkozak/pi-acp`, `narumiruna/pi-extensions`, `badlogic/pi-skills`, `nicobailon/pi-interactive-shell`. **No first-party sandbox/approval extension** was found that would replace our P1/P3 work; `pi-interactive-shell` is PTY/interactive-CLI oriented. |

---

## 7. Coverage decision — `STOP_OSS_EXPANSION`

| Capability needed | Covered by | Gap? |
|---|---|---|
| OS sandbox (macOS) | Codex §3.1 | **route identified, mechanism NOT established.** No WebCodex profile has been compiled or exercised (`§8`); "none" here would read as a working sandbox |
| OS sandbox (Linux) | Codex §3.1 (`bwrap`, Landlock+seccomp) | **route identified, mechanism NOT established** — and out of scope on this plan (macOS-only, P1b `SCOPE`) |
| Filesystem policy model | Codex C-16..C-19 | none |
| Network policy axis | Codex C-08/C-09 | **mechanism unspecified** — see §7.1 |
| Process-tree inheritance | WebCodex §2.1 / §2.1b (present) + Codex C-14 | **partial, and not a gap-free closure.** `SECURITY_INVARIANTS.md` I4 and `REFERENCE_ARCHITECTURE.md §4` limit process-group/Job-Object inheritance to **best-effort termination**, and `OSS_RESEARCH_EVIDENCE.md §2.1a` records ≥10 **unmanaged direct `Command::spawn()` sites** that no shared mechanism currently covers. "Process-tree inheritance" is therefore a *partial* reuse, and its coverage is **narrower than "the managed path"** — round 14 rejected that phrasing, because §2.1a records that the direct `Command::spawn()` sites also set process groups, so tree ownership already extends past `ManagedChild::spawn` even though no shared mechanism covers those sites. The accurate statement, bounded by what §2.1a actually records (corrected in review round 15, CHECK 10): **tree ownership is present on the managed path, and on `some` of the direct path** — the direct sites that §2.1a records as setting a process group are W-23e (`detached_job.rs:2409`), W-23f (`persistent-shell/src/lib.rs:1965`, via an unsafe `pre_exec` with `setsid`), and W-23g (`tool_runtime/helpers.rs:146`); the remaining non-test direct sites (W-23a, W-23b, W-23c, W-23d, W-23j…W-23n) are recorded **without** process-group setup, and W-23h/W-23i are unclassified. So the honest coverage statement is: **no basis for a claim that tree ownership is *largely* present on the direct path**, and no basis for one in the other direction either — the direct path is **mixed and, for the sites that matter most (W-23a/W-23b, both model-reachable shell paths), recorded as lacking it**. What is missing across both paths is a single enforcement point, and inheritance of *confinement* is a separate question this row does not answer (a process group is not a sandbox; see `REFERENCE_ARCHITECTURE.md §4`). The P0 work item is therefore to **establish** the per-site facts for W-23a…W-23i rather than to generalise from the three sites that happen to be documented. Recorded as `none` in round ≤12, corrected to `partial` in round 13, corrected again in round 14, narrowed to a per-site bounded claim in round 15 |
| **Spawn-surface coverage** | WebCodex §2.1a | **gap: no single chokepoint; ≥10 direct sites** |
| Permission evaluator | WebCodex §2.3 + OpenCode §4 | none (model port) |
| External-directory guard | Hermes §5 (`path_security.py`) | none |
| Approval state machine | Hermes §5 (H-05…H-13) | none |
| Approval transport | Hermes §5 (H-06…H-13) | none |
| **Local control channel isolation** | *nothing* | **gap: must be designed and tested** |
| Session grants | OpenCode O-13/O-17 + Hermes H-17…H-21 | none |
| Auto reviewer | Codex §3.4 (`ext/guardian-reviewer`) | none |
| Tool interception / hooks | Pi §6 + OpenCode O-20 | none |
| Audit log | WebCodex §2.3 (W-42…W-46) | none |

**No P2 project is required to close any invariant.**

### 7.1 Deliberately *not assessed* (adversarial review round 1, CHECK 1)

These are **open questions**, not closed ones. They were not investigated because
the decision did not depend on them, but they must not be recorded as
"no gap":

| Candidate | Why it matters | Status |
|---|---|---|
| `codex-process-hardening` | Listed as a dependency of `linux-sandbox`; may contain cheap, independently useful hardening (rlimits, `no_new_privs`, prctl) for the P1 sandbox plan. | **NOT ASSESSED** — read in P1 |
| `codex-network-proxy` | Appears only as a coupling obstacle, but §6.1(b) now requires a real per-request network mechanism; managed-MITM machinery may be directly relevant. | **NOT ASSESSED as reusable** — read in P1 |
| `codex-sandboxing` via **pinned git dependency** | A different proposition from crates.io; not evaluated. | **NOT ASSESSED** — see `COMPONENT_REUSE_MATRIX.md §2` |
| Bounded `VENDOR_SUBSET` (e.g. only the SBPL builder) | Distinct from vendoring the whole crate. | **NOT ASSESSED** |

```
STOP_OSS_EXPANSION = YES        (no P2 *project* is required)
OPEN_ASSESSMENTS   = 4          (the four rows above; all within P0/P1 scope)
```

Projects explicitly **not** opened, and why:

- GrokCode, zcode — name collisions unresolved; brief forbids guessing;
  no capability gap requires them.
- goose, aider, Cline, Roo Code, Claude Code OSS clones, OpenHands — would
  duplicate coverage already held by Codex + OpenCode.
- Daytona, E2B — remote/cloud sandbox providers; the target is *local* coding on
  the user's machine, so they do not fill a gap. (`src/tool_runtime/code_mode` and
  a `code_mode_e2b` test exist in WebCodex, but cloud execution is out of scope.)
- Bubblewrap / Landlock wrappers — reached indirectly as the Linux fallback
  (Codex C-12/C-13); a separate survey adds nothing.
- macOS Seatbelt wrappers — `/usr/bin/sandbox-exec` is the facility; Codex's
  usage is the reference.

---

## 8. Known limits of this evidence set

1. **All upstream repositories were read at a single commit** (see the alias
   table). Upstream may have moved; `COMPONENT_REUSE_MATRIX.md §2` carries an
   explicit re-evaluation trigger.
2. **No WebCodex behaviour was re-probed on this branch.** "Today" columns come
   from the prior empirical evaluation at the same commit
   (`WEBCODEX_EVALUATION.md`, which itself went through a 21-round adversarial
   review ending `ACCEPTED`). Where this document says "unmeasured", it means
   exactly that.
3. **`codex-sandboxing`'s runtime behaviour was not executed.** The reuse
   conclusion rests on static evidence (crates.io absence, `Cargo.toml`
   coupling, LOC) plus the fact that the mechanism is an OS facility. No
   compile spike was performed — the reasoning did not require one, because the
   recommendation is *not* to depend on the crate.
4. **No sandbox profile was compiled or exercised.** The proposed Seatbelt rules
   remain design evidence, not demonstrated confinement. This is the largest
   gap in the submission and is disclosed rather than papered over.
5. **Pi's extension contract was read from `packages/agent` and
   `packages/coding-agent` sources and example extensions**, not from a
   published API document.
6. **ChatGPT Web's MCP client capabilities were not tested.** The finding is
   server-side and stronger: WebCodex advertises no elicitation capability
   (W-53) and forbids depending on it (W-52), so the client's support is moot.
   Recorded as `CHATGPT_WEB_APPROVAL_TRANSPORT = UNSUPPORTED` (as-is server
   side), not as a claim about ChatGPT.
7. **Spawn-surface reachability is a judgement, not a proven call graph.** The
   classification in §2.1a of which direct spawn sites are "model-reachable" was
   made from file role and call site. Two sites remain explicitly unclassified
   (`workspace_checkpoint.rs`, `project_context.rs`), and
   `src/tool_runtime/helpers.rs:146` needs a call-graph check. Establishing this
   by analysis is a P0 acceptance criterion (`IMPLEMENTATION_PLAN.md` P0,
   `SECURITY_TEST_VECTORS.md` CP-01).
8. **Accepted limitation: a single-uid sandbox cannot defend a same-owned
   hardlink, and the shell surface cannot be checked per-access.** Because the
   sandboxed process runs as the same host user, an outside file owned by that
   user can be reached through a hardlink created inside the writable workspace.
   `SECURITY_TEST_VECTORS.md` PE-06 now separates the mechanisms by where they
   are enforceable: the resolver-time `st_nlink > 1` check covers the
   **structured** surface only (SBPL has no `nlink` predicate, and shell/agent
   operations do not use the resolver); the **workspace-entry preflight** (refuse
   the workspace if any regular file in it has `st_nlink > 1` — a name-free,
   deny-based admission gate, corrected in round 7 because the earlier "whose
   other names lie outside the workspace" predicate is not computable from a tree
   walk) covers all surfaces for pre-existing links; `require_uid_separation`
   covers mode-protected inodes **only while the inode is mode-protected**.
   Four residuals remain and are **not** claimed as closed: (a) PE-06a' — an agent-created link to a same-owned inode, without uid
   separation; (b) a **TOCTOU window** — a hardlink created *after* the
   workspace-entry walk is undetected, and cannot be caught per-access on the
   shell surface — restated in round 11 as **`NOT_ENFORCED`** rather than a residual
   *inside* a `DENY` claim, because an admission gate is not a standing per-access
   control and calling the vector "closed on all surfaces" contradicted the
   concession in the next sentence; (c) **PE-06a on macOS** — macOS has no equivalent of Linux's
   `fs.protected_hardlinks`, and P1b targets macOS first, so an agent-created link
   to a not-owned outside file has **no** mechanism at all there. The admission
   preflight cannot help: it inspects links present at admission, whereas PE-06a is
   created afterwards. (d) **PE-06a′ with a world-readable target** — round 10 corrected an over-claim
   here. "uid separation ⇒ `DENY`" holds only while the inode is **mode-protected**;
   a different uid can read a world-readable inode, which is the same reason
   PE-06c resists uid separation. The agent-created world-readable case is
   therefore **not** closed by uid separation. **All four** (a)–(d) are
   `KNOWN_LIMITATION`, none claimed as closed.
9. **Network enforcement is design-level.** No per-request network descriptor is
   implemented; DNS is declared DENY-by-default and hostname-scoped grants are
   declared unavailable without a locally-owned enforcing proxy, which does not
   yet exist (`REFERENCE_ARCHITECTURE.md` §6.1(b)). `codex-network-proxy` is the
   leading candidate to read for this and is currently **not assessed**.
9a. **Network confinement is per-spawn, not per-action (added in round 7).** A
   sandbox profile binds a *process*. `persistent_shell`, `script`, `job`, and
   nested `agent` surfaces spawn once and then perform many actions, so a grant
   approved at spawn time covers all of that child's later egress. No per-action
   boundary exists at the process level. The approval wording must therefore be
   process-scoped, and long-lived surfaces must stay out of network `ASK` until a
   per-action mediation point is designed. Recorded as an accepted limitation, not
   a P1c defect.
9b. **A control-plane-owned proxy does not enforce per-spawn grants by ownership
    alone (added in round 9).** The design required a shared local enforcing
    proxy for hostname-scoped grants but never bound a child to that child's
    approved destination list. Ownership establishes who runs the proxy, not what
    a given spawn may reach; a proxy reachable by every sandboxed child and
    holding no per-spawn state would grant the first connector whatever it
    permits globally. A per-spawn unforgeable capability plus a per-spawn
    allow-list in the proxy is a **precondition** of hostname grants, not a
    refinement. Until that exists, hostname grants are unavailable — which they
    already were, so this closes a proof gap rather than a capability.
9c. **An external-read approval had no enforcement path (added in round 9).**
    `COMPONENT_REUSE_MATRIX.md §4` defaulted `fs.read.external` to `ASK` while no
    phase compiled an approved external path into a per-spawn profile. Resolved by
    adding phase **P1d** (external-path profile extension) and changing the default
    to `DENY` until it ships. General rule adopted: a capability defaults to `ASK`
    only in the phase that makes the grant enforceable.
9d. **An unconfined danger-mode backend and the floors are mutually exclusive
    (added in round 9).** Floors are enforced by a pre-effect decision at the
    tool-dispatch boundary; an unconfined shell performs `cat ~/.ssh/id_rsa` as
    ordinary process I/O and never passes that boundary per action. Round 8's
    `UNSANDBOXED_BACKEND` was therefore withdrawn and replaced by
    `RELAXED_BACKEND`: a profile that widens filesystem and network permission
    **while retaining every credential deny-rule**. An unconfined backend is not
    offered at all.
9e. **Dispatch policy is a per-call boundary, not a per-action one (added in round
    10 — the recurring theme of this review; revised in round 11).** An `ASK`
    default enforced by the policy engine governs a named tool invocation. It does
    not govern what an already-running sandboxed child does next: `kill`, launching
    an agent binary, invoking `git`, or opening a socket are ordinary process
    activity inside the sandbox. Four concrete consequences are now stated rather
    than glossed:
    (a) long-lived surfaces are excluded from network and external-read `ASK`;
    (b) `proc.signal.external` and `agent.spawn.<id>` are `ASK` on the **mediated**
    path and `NOT_ENFORCED` for the same effect inside a running shell — in round
    10 this pair was written as `DENY` in a sentence that itself conceded no
    mechanism existed, which is a safety assertion, not a result; see item 9f for
    the vocabulary rule that now governs every such cell;
    (c) destructive operations and unknown binaries are `NOT_ENFORCED` when
    performed inside a shell or build process in `AUTO`, not `ASK`/`DENY`
    (`IMPLEMENTATION_PLAN.md` P5 criterion 4);
    (d) the external-`credential.helper` policy check covers the **mediated** `git`
    path only. Closing the general gap requires per-action mediation inside the
    child, which is strictly more invasive than any profile and is out of scope
    here — it is the same reason P1c excludes long-lived surfaces rather than
    trying to scope their grants.
9f. **`NOT_ENFORCED` is now a first-class outcome value (added in round 11).**
    `COMPONENT_REUSE_MATRIX.md §4` carries a four-valued vocabulary —
    `ALLOW` / `ASK` / `DENY` / `NOT_ENFORCED` — where `NOT_ENFORCED` means *no
    mechanism observes this effect on this surface*. It was added because the same
    class of defect was committed in **both** directions: an `ASK` where only the
    mediated path was covered (rounds 8–9), and, after that was fixed, a `DENY`
    where nothing was enforced at all (round 10 — "DENY inside a running shell" for
    `kill` and agent exec, written in the same paragraph that conceded no mechanism
    exists). Both are security claims dressed as outcomes. The general rule is now
    stated once: **name the enforcement mechanism for every `ASK`; use
    `NOT_ENFORCED` — never `DENY` — where the mechanism does not reach.**
10. **`codex-process-hardening` was not read.** It may supply cheap, reusable
    hardening for the P1 sandbox plan.
11. **The secret-location pattern list is incomplete.** "Credential material is
    unreachable" is supportable only for the enumerated paths and filename
    patterns (`SECURITY_TEST_VECTORS.md` §4, NET-11). Arbitrary credentials in an
    arbitrary project are **not** covered, and no document may claim otherwise.
