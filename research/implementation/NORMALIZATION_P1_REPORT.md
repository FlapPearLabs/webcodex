# P1 Execution Normalization — Implementation Report

- **Baseline branch:** `spike/webcodex-seatbelt-profile`
- **Baseline HEAD:** `a295cb2612c71c9ced01c74010f629d2f679c54d`
- **Implementation branch:** `impl/webcodex-execution-normalization-p1`
- **Scope:** P1 local execution core only
- **READY_FOR_NORMALIZATION:** YES

---

## 0. What this report does and does not claim

This report claims one thing: the highest-value local, model-triggered execution paths now reach
`ExecutionBroker` through a workspace-derived `SandboxPlan`, and every refusal is fail-closed.

It does **not** claim:

- that all execution in WebCodex is normalized;
- that the system is secure;
- that prompt injection is solved;
- that Codex parity is complete;
- that native sandbox enforcement was verified on this host (it was not — see §7).

P1 explicitly excludes: SSH / `remote_shell`, browser/CDP, plugin/MCP providers,
`coding_agent` / Hermes / Codex child processes, LSP, the persistent interactive shell,
self-update/installer, the CLI controller, approval/ASK/session-grant, and the detached durable
payload. The remaining un-routed surfaces are enumerated and named in §5 so this list cannot
quietly become wrong.

---

## 1. The required chain

Every routed path now goes through:

```
SandboxPlan → SpawnSpec → ExecutionBroker → Codex-derived Seatbelt profile → task process tree
```

The Runner-side chokepoint is `spawn_local_action`
(`crates/webcodex-runner/src/webcodex_runner/local_execution.rs`). It is the **only** place in the
Runner that constructs an `ExecutionBroker` for model-triggered local work.

P1's static policy, expressed once in `derive_workspace_plan`:

| Authority | Value |
|---|---|
| Writable roots | the canonical workspace root only |
| Readable roots | none beyond the minimum system allowances |
| External filesystem | denied |
| Network | `NetworkPolicy::Deny` |
| Toolchain | host-derived `TrustedToolchainRoot` prefixes only |
| Approval / ASK / session grant | **not implemented in P1** |

---

## 2. Old vs. new spawn path

### 2.1 `run_shell` main path

**Before.** `shell.rs` assembled a `std::process::Command` and called `ManagedChild::spawn`
directly. The process inherited whatever the Runner had; confinement was whatever the caller
remembered to apply.

**After.** The command is expressed as a serializable `CommandBlueprint`, converted into a
`LocalExecutionRequest`, and handed to `spawn_local_action`, which derives the plan from trusted
project context and launches via the broker:

```rust
// shell.rs — the single routed launch site
let mut child = match spawn_local_action(policy, project_registry_dir, request) { ... };
```

### 2.2 Local jobs (`job_manager`)

**Before.** Two direct `ManagedChild::spawn` sites for queued/running local jobs.

**After.** Both go through the chokepoint:

- `job_manager.rs:3001` — job start
- `job_manager.rs:3269` — queue advance / next command

### 2.3 `workspace_checkpoint`: `git_output` and `git_apply`

**Before.** `workspace_checkpoint.rs` spawned git itself.

**After.** Both delegate to the git broker, so git gets the same workspace-derived plan as
everything else:

```rust
// git_output
let result = crate::git_broker::run_git(root, args, input, None).map_err(|refusal| { ... })?;

// git_apply — patch still travels over stdin, semantics unchanged
let mut argv = vec!["apply"];
argv.extend_from_slice(args);
argv.push("-");
let result = crate::git_broker::run_git(root, &argv, Some(patch.as_bytes()), None) ...
```

`git_apply` was a **high-priority** P1 target. Patch semantics are preserved: the real `ExitStatus`
from the broker is propagated, and the patch is delivered on stdin exactly as before.

### 2.4 `project_context`: `bounded_git_output`

**Before.** Its own git invocation with its own byte-budget/timeout logic.

**After.** `crate::git_broker::run_git_bounded(root, &args, max_bytes, deadline)`. The existing
`BoundedGitOutput` semantics (`complete` / `timed_out` / `warning_code`) are preserved on top of the
brokered run.

### 2.5 Validation / execute

`validation/execute.rs::run_bounded` is **not** routed in P1. It launches interpreter-based tools
(pyright and similar adapters), whose runtime prefixes are not inside any recognized
`TrustedToolchainRoot` prefix. Routing them requires a new policy decision about which interpreters
are trusted — and P1 excludes approval/ASK/grant, which is the mechanism that decision would need.

This is recorded in-source as `RUNTIME_COMPATIBILITY_TODO` rather than left as a silent gap, and a
test (`i_validation_execute_is_declared_unrouted_with_a_runtime_todo`) **fails** if the marker is
removed or if the file starts claiming to route.

---

## 3. Workspace-derived plan: the model cannot choose the root

The root is not a model parameter. `SandboxPlan` is derived from trusted server-side project
context only:

- `WorkspaceAuthority` (`crates/webcodex-process/src/execution_broker/workspace_authority.rs`)
  canonicalizes trusted roots and resolves a candidate to the **narrowest covering** trusted root.
- `resolve_workspace_authority` (runner side) adapts `RunnerPolicy` into that authority.
- A model-supplied `cwd` is treated as a **request**, not an authority. Naming a wider directory is a
  refusal, never a wider grant.

Two independent tests pin this: P1-A (runner) and P1-H / `plan_grants_only_the_project_root`
(git broker). Both assert `readable_roots.is_empty()` and `network == NetworkPolicy::Deny`.

---

## 4. Fail-closed

Every one of these refuses **before a process exists**, with a stable refusal code, and never falls
back to a bare `Command::spawn` or an unrestricted `ManagedChild::spawn`:

| Condition | Refusal code |
|---|---|
| Missing trusted project context | `sandbox_authority_unavailable` |
| Invalid / non-canonicalizable workspace root | `git_workspace_root_invalid` |
| cwd outside every trusted root | `sandbox_authority_unavailable` |
| Sandbox compiler / launcher failure | `git_spawn_refused` (git) / broker refusal (runner) |
| Untrusted git executable | `git_workspace_root_invalid` |

Git executables are resolved only from fixed trusted prefixes (`/usr/bin/git`, `/bin/git`,
`/usr/local/bin/git`, `/opt/homebrew/bin/git`, `/opt/local/bin/git`); `/` and `$HOME` are rejected.
Git runs under `EnvPolicy::Minimal` with `HOME=<root>`, `GIT_CONFIG_NOSYSTEM=1` and
`GIT_TERMINAL_PROMPT=0`.

---

## 5. Surface accounting (re-measured, not estimated)

Counted from the source at this commit, production code only (test modules excluded).

```text
P1_ROUTED_COUNT=6
P1_REMAINING_MODEL_TRIGGERED_COUNT=4
```

**Routed (6).** Every production call site that reaches the broker:

| # | Site | Path |
|---|---|---|
| 1 | `shell.rs:3112` | `run_shell` main execution (the single launch site for all `configured_*_shell_command` builders, prepared/explicit/job/validation variants) |
| 2 | `job_manager.rs:3001` | local job start |
| 3 | `job_manager.rs:3269` | local job queue advance |
| 4 | `workspace_checkpoint.rs:357` | `git_output` |
| 5 | `workspace_checkpoint.rs:396` | `git_apply` (high-priority P1 target) |
| 6 | `project_context.rs:610` | `bounded_git_output` |

`local_execution.rs:311` is the chokepoint definition itself, not an additional path.

Note on what these 6 sites cover: because the shell command builders all converge on
`shell.rs:3112`, one call site carries the `run_shell` main path, the prepared and explicit shell
variants, the shell-job variants, and the validation-step variant. The count is of *launch sites
reaching the broker*, not of command variants — those are pinned by the structural guard in §6.2,
which names each routed builder function individually.

`project_context.rs:1125` also calls `git_broker::run_git`, but it sits inside a `#[cfg(test)]`
module and is excluded from this count.

**Remaining (4), each named and each out of P1 scope by the accepted scope statement:**

| # | Site | Function | Why not routed |
|---|---|---|---|
| 1 | `shell.rs:1253` | `run_prepare_command` | Runs a user-configured shell profile's `init_script`. Authority is the profile configuration, not a model-authored command; cwd is profile-owned. |
| 2 | `shell.rs:1460` | `capture_profile_env_snapshot` | Runner-owned control-plane probe (reads the environment a profile would produce). |
| 3 | `shell.rs:1657` | Tool Plugin launcher (`get_or_prepare`) | P1 excludes plugin/MCP provider processes. |
| 4 | `job_manager.rs:3462` | `start_ssh_shell_job` | P1 excludes SSH / `remote_shell`. |

Plus, outside the Runner's local-execution surface entirely: the detached durable payload
(`detached_job.rs`), the persistent interactive shell, LSP, browser/CDP, and
`coding_agent`/Hermes/Codex children.

These four are **asserted to still exist** by
`known_unrouted_surfaces_are_still_present_and_named`, so this list cannot silently drift.

---

## 6. Tests

### 6.1 Functional coverage: P1-A … P1-J

| Case | Asserts | Where |
|---|---|---|
| P1-A | Plan comes from trusted context; a wider cwd is refused, not granted | `normalization_p1_tests.rs` |
| P1-B | Inside the workspace the action can read and write | `normalization_p1_tests.rs` → `P1_NATIVE_RUN_SHELL` |
| P1-C | cwd outside authority refuses **before spawn** | `normalization_p1_tests.rs` |
| P1-D | A file outside the workspace is not readable | `normalization_p1_tests.rs` → `P1_NATIVE_EXTERNAL_DENY` |
| P1-E | Network is denied | `normalization_p1_tests.rs` → `P1_NATIVE_NETWORK_DENY` |
| P1-F | Descendants inherit the profile (grandchild cannot escape) | `normalization_p1_tests.rs` → `P1_NATIVE_DESCENDANT` |
| P1-G | `git apply` applies a real patch through the broker, semantics preserved | `git_broker.rs` → `P1_NATIVE_GIT_APPLY` |
| P1-H | The git helper gains no authority beyond the project root | `git_broker.rs` |
| P1-I | The validation exception stays declared with `RUNTIME_COMPATIBILITY_TODO` | `normalization_p1_tests.rs` |
| P1-J | Missing trusted context fails before process creation with a stable code | `normalization_p1_tests.rs` |

P1-G and P1-H live in `webcodex-workspace` on purpose: the git broker is `pub(crate)` there, and a
test that reimplemented it would prove nothing about the code that ships.

### 6.2 Structural anti-bypass guard

`p1_routed_functions_cannot_spawn_outside_the_broker` pins the P1-routed **functions** and fails if
any of them contains `ManagedChild::spawn(` or `Command::new(`.

It is function-level rather than file-level on purpose. A file-level ban was tried first and flagged
three real spawn sites that P1 does not own: `run_prepare_command` (profile config authority), the
Tool Plugin launcher (excluded provider), and `start_ssh_shell_job` (excluded SSH). Widening the
guard to those would either be wrong or would re-open the P1 scope decision, which this round does
not do. The complement —
`the_local_execution_chokepoint_exists_and_is_used` — fails if the chokepoint is deleted, or if
`shell.rs` / `job_manager.rs` merely *import* it without actually calling it.

### 6.3 Test results at this commit

```text
webcodex-process        38 + 15 + 3 passed, 0 failed
webcodex-workspace      72 passed, 0 failed        (63 at baseline + 9 new)
runner normalization_p1 11 passed, 0 failed
cargo fmt --check       clean (webcodex-process, webcodex-runner, webcodex-workspace)
```

### 6.4 An honest note on 9 workspace tests

Nine `webcodex-workspace` tests initially failed after this change. That was a **real regression
introduced here**, not a pre-existing condition: verified by stashing the change and running the
same command on the baseline (63 passed / 0 failed). Root cause: the host cannot apply a restrictive
Seatbelt profile (`sandbox-exec: sandbox_apply: Operation not permitted`), so the broker correctly
failed closed and every git-dependent test failed with it. Fixed by distinguishing the two causes —
a genuine `git apply` failure panics; a refused profile reports `ENV_BLOCKED` and asserts the patch
was **not** applied. On `ENV_BLOCKED` hosts the eight `project_context` tests skip explicitly via
`brokered_git_usable()` and print `SKIP ENV_BLOCKED` — they never pass silently.

### 6.5 Bug fixed outside the P1 surface

`research/spikes/native-seatbelt-ae.sh` had a cleanup-order bug: it deleted the run log and then
still grepped that deleted file for `RUNTIME_PYTHON` / `RUNTIME_NODE`, comparing two empty strings
and silently emitting nothing. The verdicts are now captured before the log is removed.

---

## 7. Native verification: NOT RUN (ENV_BLOCKED)

`research/spikes/native-normalization-p1.sh` exercises the **production** path
(`spawn_local_action` → `ExecutionBroker` → `sandbox-exec`), not the research probe binary.

It was run here and produced:

```text
restrictive profile probe: rc=71 (host refuses narrowing)
P1_NATIVE_ALL_PASS=ENV_BLOCKED
exit 3
```

`rc=71` is `sandbox-exec` refusing to apply a profile inside this nested sandbox session. **This is
not a pass.** No native enforcement claim in this report is backed by a completed run.

The script is designed so this cannot be misread:

- it probes the host before running anything and exits 3 if narrowing is refused;
- each case prints exactly one of `PASS` / `FAIL` / `ENV_BLOCKED`;
- a **missing** verdict line is `NOT_REPORTED` and counts as a failure — silence must never read as
  success;
- exit codes: `0` all PASS, `1` at least one FAIL, `3` host cannot measure.

To obtain real native evidence, run from an ordinary Terminal.app session (launchd session `Aqua`):

```bash
bash research/spikes/native-normalization-p1.sh
```

---

## 8. Runtime compatibility

Python/Node runtime compatibility problems were **not** solved in P1; they are recorded as
`RUNTIME_COMPATIBILITY_TODO` in `validation/execute.rs`, with a test that prevents the marker from
being quietly dropped or quietly upgraded into a claim of routing.

---

## 9. Explicitly out of scope for P1

SSH / `remote_shell`; browser/CDP; plugin/MCP providers; `coding_agent` / Hermes / Codex child
processes; LSP; persistent interactive shell; self-update / installer; CLI controller; approval /
ASK / session grant; network allow; `NetworkPolicy::Allow`.

---

## 10. Toolchain note

`crates/webcodex-runner/src/webcodex_runner/coding_agent.rs:261` uses `atomic_try_update`, which
requires a newer unstable feature. Verified as a **baseline** condition (`git show
a295cb26:...coding_agent.rs` already contains it; `git diff --stat HEAD` on that file is empty).
Verification therefore used the rustup toolchain (rustc 1.95.0); Homebrew rustc 1.94.0 cannot
compile the baseline.
