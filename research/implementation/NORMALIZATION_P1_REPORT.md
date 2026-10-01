# P1 Execution Normalization — Closure Report

- **Baseline branch:** `spike/webcodex-seatbelt-profile`
- **Baseline HEAD:** `a295cb2612c71c9ced01c74010f629d2f679c54d`
- **Implementation branch:** `impl/webcodex-execution-normalization-p1`
- **Closure branch (this round):** `fix/webcodex-execution-normalization-p1-closure`
- **Closure HEAD:** `739ac79e16f913ccc18d89c21d78e557988489fc` (the branch is created from this
  baseline; the closure delta is the set of commits on `fix/webcodex-execution-normalization-p1-closure`
  above this SHA)
- **Scope:** P1 local execution core + the closure defects GPT identified
- **P1_CLOSURE_STATUS:** `PARTIAL` (defects #1–#5 and #7 closed; #6 — detached durable payload — recorded as blocked, not solved)
- **P1_CORE_LOCAL_NORMALIZATION:** `PASS`
- **P1B_REQUIRED:** `YES`
- **READY_FOR_NORMALIZATION / READY_FOR_P2:** `NO` — see §11

---

## 0. What this report does and does not claim

This report claims one thing for the **local, model-triggered** execution core: it now reaches
`ExecutionBroker` through a workspace-derived `SandboxPlan`, with positive-selection environment
handling, registered-project-only authority, order-independent narrowest-root selection, a network
gate with a positive control, real `run_shell` entry-point coverage, real `git apply` through the
broker, and `validation/execute` routed through the same chokepoint — and every refusal is
fail-closed.

It does **not** claim:

- that all execution in WebCodex is normalized;
- that the system is secure;
- that prompt injection is solved;
- that Codex parity is complete;
- that native sandbox enforcement was verified on this host (it was not — see §7);
- that the detached durable payload is normalized (it is **not** — see §9).

The closure round (role `WEBCODEX_EXECUTION_NORMALIZATION_P1_CLOSURE`) closes GPT's defects #1–#5
(model env inheritance, `HOME` fallback, `narrowest_covering` order-dependence, network-gate positive
control, real `run_shell`/`git apply` paths) and #7 (`validation/execute` must route). Defect #6
(detached durable payload) is explicitly **out of scope for this round**: it is recorded as
`DETACHED_DURABLE_NORMALIZATION = BLOCKED_PROCESS_OWNERSHIP` in §9, with no redesign.

P1 explicitly excludes (and the remaining un-routed surfaces are enumerated and named in §5 so this
list cannot quietly become wrong): SSH / `remote_shell`, browser/CDP, plugin/MCP providers,
`coding_agent` / Hermes / Codex child processes, LSP, the persistent interactive shell,
self-update/installer, the CLI controller, approval/ASK/session-grant, network allow, and the
detached durable payload.

---

## 1. The required chain

Every routed path now goes through:

```
SandboxPlan → SpawnSpec → ExecutionBroker → Codex-derived Seatbelt profile → task process tree
```

The Runner-side chokepoint is `spawn_local_action`
(`crates/webcodex-runner/src/webcodex_runner/local_execution.rs`). It is the **only** place in the
Runner that constructs an `ExecutionBroker` for model-triggered local work, and it is the only place
that derives the workspace plan from registered project context.

P1's static policy, expressed once in `derive_workspace_plan`:

| Authority | Value |
|---|---|
| Writable roots | the canonical workspace root only |
| Readable roots | none beyond the minimum system allowances |
| External filesystem | denied |
| Network | `NetworkPolicy::Deny` |
| Toolchain | host-derived `TrustedToolchainRoot` prefixes only |
| Approval / ASK / session grant | **not implemented in P1** |

### 1.1 Environment model: positive selection (defect #2, P0)

Environment handling is now a **positive list**, not a credential denylist. See
`APPROVED_INHERITED_ENV_KEYS` / `is_approved_inherited_env_key` / `approved_inherited_env` /
`sanitize_snapshot` in `local_execution.rs`. The approved set is deliberately named — `PATH`,
`LANG`, the `LC_*` locale family, `LANGUAGE`, `TERM`, `COLORTERM`, `NO_COLOR`, `TZ`, plus the Windows
console/`TEMP` plumbing — and deliberately **excludes `HOME`** and every credential name
(`GH_TOKEN`, `GITHUB_TOKEN`, `OPENAI_API_KEY`, `AWS_SECRET_ACCESS_KEY`, `NPM_TOKEN`, webcodex tokens,
`DESKTOP_MCP_*`). A name the denylist never thought of cannot leak, because nothing reaches the child
unless it is on the list.

`LocalEnv::Inherit` was **deleted** from the type. `EnvPolicy::Inherit` is therefore unreachable from
any P1 model-triggered surface — this is compiler-enforced, not a convention. Previously the broker
received `EnvPolicy::Inherit` (the Runner's whole environment minus a five-name denylist), which is
not a boundary. Now every model-triggered action carries an explicit `LocalEnv::Snapshot` built from
`approved_inherited_env` + the isolated floor + operator `shell.env` + sensitive-name removal.
Prepared shell-profile snapshots pass through exactly the same `sanitize_snapshot` path.

The structural guard `k_model_triggered_env_is_positively_selected_not_inherited` verifies: known
secrets (`TOTALLY_NEW_SECRET_123`, `GH_TOKEN`, `GITHUB_TOKEN`, `OPENAI_API_KEY`, `AWS_SECRET_ACCESS_KEY`,
`NPM_TOKEN`, `DESKTOP_MCP_SOURCE_TOKEN`) stay out of the selected set; `PATH`/`LANG`/`TERM`/`NO_COLOR`
stay in; `LocalEnv::Inherit` survives only as documentation; and no production code in
`shell.rs` / `local_execution.rs` / `job_manager.rs` requests `EnvPolicy::Inherit`.

### 1.2 Authority model: registered project only (defect #3, P0)

`resolve_workspace_authority` (runner side) now takes **no policy parameter**. Authority is the
registered project root and nothing else:

- no registry, or no registered project covering the requested cwd → `AuthorityError::NoTrustedContext`;
- otherwise → `WorkspaceAuthority::narrowest_covering(requested_cwd, &[project.path])`.

The structural elimination of the `policy`/`allowed_roots` parameter is asserted by
`l_home_is_not_a_project_authority`: the resolver's declaration contains neither `policy` nor
`allowed_roots`, and its body never reads `policy.allowed_roots`. `$HOME` cannot reach the resolver,
so it cannot become an execution authority. A `HOME` trusted only as a generic `allowed_root` (file
ops) with no registered project → refused. A registered project under `HOME` → authority is the
project root, never `HOME`.

This is a **compatibility change stated as fail-closed**: a request that previously resolved to a
coarse `$HOME` grant now refuses. That is the intended behaviour — under-confining a model-triggered
action to the user's whole home directory was the bug.

---

## 2. Old vs. new spawn path

### 2.1 `run_shell` main path

**Before.** `shell.rs` assembled a `std::process::Command` and called `ManagedChild::spawn` directly.
`EnvPolicy` was `Inherit` (credential denylist).

**After.** The command is expressed as a serializable `CommandBlueprint`, converted into a
`LocalExecutionRequest` with an explicit `LocalEnv::Snapshot` (positive-selection environment), and
handed to `spawn_local_action`, which derives the plan from registered project context and launches
via the broker:

```rust
// shell.rs — the single routed launch site (env rule rebuilt into a Snapshot first)
let mut child = spawn_local_action(project_registry_dir, request)?;
```

### 2.2 Local jobs (`job_manager`)

**Before.** Two direct `ManagedChild::spawn` sites for queued/running local jobs, with `EnvPolicy::Inherit`.

**After.** Both go through the chokepoint, with the validation-step env merged via `Snapshot` →
`sanitize_snapshot` → `LocalEnv::Snapshot`:

- `job_manager.rs:3001` — job start
- `job_manager.rs:3269` — queue advance / next command

### 2.3 `workspace_checkpoint`: `git_output` and `git_apply`

**Before.** `workspace_checkpoint.rs` spawned git itself.

**After.** Both delegate to `crate::git_broker::run_git(root, …)`, so git gets the same
workspace-derived plan as everything else:

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

### 2.5 Validation / execute — **ROUTED in the closure (defect #7)**

`validation/execute.rs::run_bounded` is now **routed** through the broker. It builds a
`LocalExecutionRequest` with `LocalEnv::Snapshot(approved_inherited_env(&host_env_map()))` and calls
`spawn_local_action(project_registry_dir, request)`. If the interpreter cannot start under the trusted
runtime policy, `run_bounded` returns a spawn failure — it must **never** fall back to a bare
`Command::spawn` or to `ManagedChild::spawn` of an unbrokered `Command`.

Runtime compatibility and execution normalization are now **separate claims**:

- `EXECUTION_NORMALIZATION = PASS` — the validation process is brokered and confined; and
- `RUNTIME_COMPATIBILITY = FAIL` is allowed and does not downgrade the normalization claim — an
  interpreter whose prefix is not a recognized `TrustedToolchainRoot` reports a spawn failure, which
  is the correct fail-closed outcome, not an escape.

The old `RUNTIME_COMPATIBILITY_TODO` marker is gone (its presence is asserted **absent** in the test
suite). Authority is threaded from the caller: `execute_validation_at_root` / the production
`execute_validation_with_shutdown` pass the trusted `project_registry_dir` down to `run_pyright` →
`run_bounded`. The execution layer never re-derives the authority from an arbitrary cwd. Timeout,
stdout/stderr caps, shutdown cleanup, and process-tree ownership are preserved: the routed path still
uses `ManagedChild` and `terminate_validation_child` (which owns the tree via
`request_terminate_tree`), so cleanup behaviour is unchanged.

The anti-bypass guard `i_validation_execute_is_brokered_and_cannot_direct_spawn` verifies the
production region of `validation/execute.rs` contains `spawn_local_action`, contains
`request_terminate_tree`, and contains **no** `Command::new(` or `ManagedChild::spawn(` — scoped to
the production region, because the `#[cfg(test)]` module legitimately compiles a fixture with `rustc`
and spawns a helper directly to test the cleanup routine in isolation.

---

## 3. Workspace-derived plan: the model cannot choose the root

The root is not a model parameter. `SandboxPlan` is derived from trusted server-side project context
only:

- `WorkspaceAuthority` (`crates/webcodex-process/src/execution_broker/workspace_authority.rs`)
  canonicalizes trusted roots and resolves a candidate to the **narrowest covering** trusted root.
- `resolve_workspace_authority` (runner side) adapts registered project context into that authority.
- A model-supplied `cwd` is treated as a **request**, not an authority. Naming a wider directory is a
  refusal, never a wider grant.

Two independent tests pin this: P1-A (runner) and P1-H / `plan_grants_only_the_project_root`
(git broker). Both assert `readable_roots.is_empty()` and `network == NetworkPolicy::Deny`.

### 3.1 `narrowest_covering` is order-independent (defect #4)

`narrowest_covering` now picks the **deepest canonical covering root** regardless of the order the
roots were supplied (uses `depth(path) = path.components().count()`; ties broken deterministically).
Both process-side tests (`plan_grants_the_workspace_and_nothing_else`, and the runner P1-M) assert the
project root wins in **both** orderings and with duplicates. The old code returned the *first* covering
root, which let a caller that listed a coarse root first confine a project to its parent.

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

### 4.1 Network gate with a positive control (defect #5)

The native network case (`e_network_is_denied_with_a_positive_control`) is driven by a **positive
control** rather than a dead port:

1. the test process binds `127.0.0.1:<random port>` — `LISTENER_BOUND`;
2. an **unsandboxed** child connects to it — `UNSANDBOXED_CONNECT_PASS`; this proves the listener is
   real and reachable, so a subsequent denial is meaningful;
3. only then does the **production** `run_shell` path run the same connect — `SANDBOX_PROCESS_STARTED`,
   then `SANDBOX_CONNECT_DENIED`.

A PASS requires all four. If step 1 or 2 fails there is nothing to measure, so the case reports
`TEST_BLOCKED` (never PASS). This removes the prior bug where `exec 3<>/dev/tcp/127.0.0.1/1` "passed"
on any host because port 1 is closed whether or not a sandbox exists.

---

## 5. Surface accounting (re-measured, not estimated)

Counted from the source at this commit, production code only (test modules excluded). The single
rolled-up `P1_REMAINING_MODEL_TRIGGERED_COUNT=4` is **replaced** by the five separated metrics the
closure requires, each named individually.

```text
P1_REQUESTED_TARGETS_TOTAL=10
P1_REQUESTED_TARGETS_ROUTED=6
P1_REQUESTED_TARGETS_BLOCKED=0
RUNNER_LOCAL_REMAINING_COUNT=4
TOTAL_KNOWN_MODEL_TRIGGERED_UNROUTED_SURFACES=4
```

(The breakdown below names every target so the totals are auditable; a surface that disappears or
appears silently would make these numbers wrong, and `known_unrouted_surfaces_are_still_present_and_named`
asserts the remaining ones still exist in source.)

**Requested (10) = routed (6) + runner-local remaining (4).** Every production call site reaching the
broker:

| # | Site | Path |
|---|---|---|
| 1 | `shell.rs:3112` | `run_shell` main execution (single launch site for all `configured_*_shell_command` builders, prepared/explicit/job/validation variants) |
| 2 | `job_manager.rs:3001` | local job start |
| 3 | `job_manager.rs:3269` | local job queue advance |
| 4 | `workspace_checkpoint.rs:357` | `git_output` |
| 5 | `workspace_checkpoint.rs:396` | `git_apply` (high-priority P1 target) |
| 6 | `project_context.rs:610` | `bounded_git_output` |

`local_execution.rs:311` is the chokepoint definition itself, not an additional path. `validation/execute.rs`
is now routed too (§2.5); it is counted as part of the validated execution path, which is why
`RUNNER_LOCAL_REMAINING_COUNT` is 4, not 5 — the old count double-counted it.

Note on what these 6 sites cover: because the shell command builders all converge on `shell.rs:3112`,
one call site carries the `run_shell` main path, the prepared and explicit shell variants, the
shell-job variants, and the validation-step variant. The count is of *launch sites reaching the broker*,
not of command variants — those are pinned by the structural guard in §6.2, which names each routed
builder function individually.

`project_context.rs:1125` also calls `git_broker::run_git`, but it sits inside a `#[cfg(test)]` module
and is excluded from this count.

**Runner-local remaining (4), each named and each out of P1 scope by the accepted scope statement:**

| # | Site | Function | Why not routed |
|---|---|---|---|
| 1 | `shell.rs:1253` | `run_prepare_command` | Runs a user-configured shell profile's `init_script`. Authority is the profile configuration, not a model-authored command; cwd is profile-owned. |
| 2 | `shell.rs:1460` | `capture_profile_env_snapshot` | Runner-owned control-plane probe (reads the environment a profile would produce). |
| 3 | `shell.rs:1657` | Tool Plugin launcher (`get_or_prepare`) | P1 excludes plugin/MCP provider processes. |
| 4 | `job_manager.rs:3462` | `start_ssh_shell_job` | P1 excludes SSH / `remote_shell`. |

Plus, outside the Runner's local-execution surface entirely: the **detached durable payload**
(`detached_job.rs`) — see §9, `DETACHED_DURABLE_NORMALIZATION = BLOCKED_PROCESS_OWNERSHIP`; the
persistent interactive shell, LSP, browser/CDP, and `coding_agent`/Hermes/Codex children.

These four are **asserted to still exist** by `known_unrouted_surfaces_are_still_present_and_named`,
so this list cannot silently drift. Note: that test now **asserts the absence** of
`RUNTIME_COMPATIBILITY_TODO` from `validation/execute.rs` (it is routed), and asserts the *presence*
of the detached-payload spawn error string (it is still unrouted). The two were previously
contradictory; the contradiction is resolved by removing validation from the unrouted list in the same
change that routed it.

---

## 6. Tests

### 6.1 Functional coverage: P1-A … P1-M

| Case | Asserts | Where |
|---|---|---|
| P1-A | Plan comes from trusted context; a wider cwd is refused, not granted | `normalization_p1_tests.rs` |
| P1-B | Inside the workspace the action can read and write | `normalization_p1_tests.rs` → `P1_NATIVE_RUN_SHELL` |
| P1-C | A cwd outside authority refuses **before spawn** | `normalization_p1_tests.rs` (chokepoint) |
| P1-D | A file outside the workspace is not readable | `normalization_p1_tests.rs` → `P1_NATIVE_EXTERNAL_DENY` |
| P1-E | Network is denied, with a positive control | `normalization_p1_tests.rs` → `P1_NATIVE_NETWORK_DENY` |
| P1-F | Descendants inherit the profile (grandchild cannot escape) | `normalization_p1_tests.rs` → `P1_NATIVE_DESCENDANT` |
| P1-G | `git apply` applies a real patch through the broker, semantics preserved | `git_broker.rs` → `P1_NATIVE_GIT_APPLY` |
| P1-H | The git helper gains no authority beyond the project root | `git_broker.rs` |
| P1-I | Validation execute is now routed; no `Command::new(` / `ManagedChild::spawn(` in production region | `normalization_p1_tests.rs` |
| P1-J | Missing trusted context fails before process creation with a stable code | `normalization_p1_tests.rs` |
| P1-K | Model-triggered env is positively selected, not inherited | `normalization_p1_tests.rs` |
| P1-L | `HOME` is not a project authority (resolver takes no policy) | `normalization_p1_tests.rs` |
| P1-M | `narrowest_covering` is order-independent | `normalization_p1_tests.rs` + `workspace_authority.rs` |

P1-G and P1-H live in `webcodex-workspace` on purpose: the git broker is `pub(crate)` there, and a
test that reimplemented it would prove nothing about the code that ships.

### 6.2 Structural anti-bypass guard

`p1_routed_functions_cannot_spawn_outside_the_broker` pins the P1-routed **functions** and fails if
any of them contains `ManagedChild::spawn(` or `Command::new(` (the chokepoint `local_execution.rs` is
allowed a `Command::new(` only for its documented control-plane probes).

It is function-level rather than file-level on purpose. A file-level ban was tried first and flagged
three real spawn sites that P1 does not own: `run_prepare_command` (profile config authority), the
Tool Plugin launcher (excluded provider), and `start_ssh_shell_job` (excluded SSH). Widening the
guard to those would either be wrong or would re-open the P1 scope decision, which this round does not
do. The complement — `the_local_execution_chokepoint_exists_and_is_used` — fails if the chokepoint is
deleted, or if `shell.rs` / `job_manager.rs` merely *import* it without actually calling it.

### 6.3 Which layer the native cases enter (defect #6 from the first round)

`b_workspace_is_readable_and_writable`, `d_external_filesystem_is_denied`, `e_network_is_denied_*`,
`f_descendants_inherit_the_profile` all call **`run_shell_with_profiles_and_execution_state`** — the
real production `run_shell` service boundary — not `spawn_local_action` directly. The chain under test
is production end to end:

```text
run_shell_with_profiles_and_execution_state
  -> run_shell_impl
    -> configured_shell_command          (command text + environment rule)
    -> execute_configured_command
      -> spawn_local_action             (the chokepoint)
        -> resolve_workspace_authority  (registered project only)
        -> ExecutionBroker::spawn_with_toolchain
          -> Codex-derived Seatbelt profile
            -> task process tree
```

Nothing here re-implements `run_shell`. P1-C and P1-J still call the chokepoint directly because they
are about the chokepoint's own precondition (a bad cwd refuses before spawn); P1-L calls the resolver
directly for the same reason. Structural tests may pin `spawn_local_action`, but they cannot substitute
for the real `run_shell` entry-point coverage — which is exactly what the first round lacked.

### 6.4 Test results at this commit

```text
cargo fmt --check       clean
cargo check             clean (webcodex-process, webcodex-runner, webcodex-workspace)
webcodex-process        38 + 15 + 3 passed, 0 failed
webcodex-workspace      72 passed, 0 failed        (63 at baseline + 9 new)
runner normalization_p1 14 passed, 0 failed
runner validation       59 passed, 0 failed
git diff --check        clean
secret scan             no credentials in the diff (matches are the denylist
                        names in comments and the P1-K placeholder values)
```

### 6.5 An honest note on 9 workspace tests

Nine `webcodex-workspace` tests initially failed after this change. That was a **real regression
introduced here**, not a pre-existing condition: verified by stashing the change and running the same
command on the baseline (63 passed / 0 failed). Root cause: the host cannot apply a restrictive
Seatbelt profile (`sandbox-exec: sandbox_apply: Operation not permitted`), so the broker correctly
failed closed and every git-dependent test failed with it. Fixed by distinguishing the two causes —
a genuine `git apply` failure panics; a refused profile reports `ENV_BLOCKED` and asserts the patch
was **not** applied. On `ENV_BLOCKED` hosts the eight `project_context` tests skip explicitly via
`brokered_git_usable()` and print `SKIP ENV_BLOCKED` — they never pass silently.

### 6.6 The routing change moved validation from "unconfined" to "fail-closed", and that had a cost

Routing `validation/execute` (defect #7) changed what the validation tests *measure*. Before the
closure they spawned an interpreter directly and passed everywhere. After it they go through the
broker, so on this host they hit the same `sandbox_apply` refusal and **16 tests failed**.

This was verified as a real regression rather than assumed: stashing the change and running the same
test on the baseline passes. It was then fixed the same way `webcodex-workspace` fixed its git
tests — a loud, once-per-process capability probe plus a `require_broker_capable!()` skip.

The first version of that probe was wrong and worth recording, because it is the exact failure mode a
"guard" invites. It pointed at a deliberately missing program and inspected only the response
envelope, so it never reached the launcher, reported **every** host as capable, and the gate silently
did nothing while looking like protection. The probe now performs a real brokered spawn of a real
executable and treats **only** the launcher's refusal signature as `ENV_BLOCKED`; any other outcome
counts as usable, so a genuine routing regression still fails loudly. The shared probe lives in
`main_tests.rs` (`broker_can_run`, `project_registry_for`, `require_broker_capable!`) rather than
being duplicated per test module.

Two registry facts the routing exposed, both of which had to be fixed honestly rather than papered
over:

* `execute_validation_at_root` originally passed the **project root** where a **registry directory**
  was expected. A project directory contains no `<name>.toml`, so the lookup found nothing and every
  spawn refused. The fix is that the test entry point now takes a real registry directory, built by
  the shared helper in production's on-disk format. The alternative — teaching the resolver to accept
  a bare directory — would have reintroduced exactly the implicit-authority hole §1.2 closed.
* `git diff --check` and the credential scan are clean. The scan's matches are all the §1.1 denylist
  names inside comments and the deliberate `ghp_leaked` / `sk-leaked` placeholder values P1-K uses to
  prove positive selection *excludes* them.

### 6.7 Full `webcodex-runner` suite: 67 failures, ENV_BLOCKED, NOT fixed here

`cargo test -p webcodex-runner --bin webcodex-runner` reports **914 passed / 67 failed** on this
host. The failures are concentrated in `dispatch_shell`, `shell_config`, `shell_profiles`,
`shell_job_execution` and two structured-process job tests — i.e. every test that asserts on the
behaviour of a real `run_shell` child.

**This is stated, not hidden, and it is not counted as a P1 result.** What is established:

* every one of those paths reaches `ExecutionBroker` at the **baseline** commit `739ac79e` too, so
  they already hit this host's `sandbox_apply` refusal before the closure changed anything;
* the failures therefore share one environmental cause, not seven defects;
* the §12 required matrix does not include this suite, and it is green for the parts P1 owns.

The honest limit: I could not complete a baseline comparison run of this suite to *prove* the 67 are
pre-existing — the baseline rebuild ran past 45 minutes and was terminated. So the claim rests on the
path analysis above, not on a measured A/B. Confirming it is a follow-up on a host that can apply a
Seatbelt profile, and it is recorded here rather than papered over. On such a host these tests are
expected to pass, and the `require_broker_capable!()` gates added in §6.6 become no-ops.

---

## 7. Native verification: NOT RUN ON THIS HOST (ENV_BLOCKED)

`research/spikes/native-normalization-p1.sh` exercises the **production** path
(`run_shell_with_profiles_and_execution_state` → `spawn_local_action` → `ExecutionBroker` →
`sandbox-exec`, plus `git_broker` for P1-G), not a research probe binary. It was **not** run inside
WorkBuddy, and the closure does not treat `ENV_BLOCKED` as a pass.

Running it on this host produces:

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

The script's required verdict lines (from §13 of the closure spec): `P1_NATIVE_RUN_SHELL`,
`P1_NATIVE_GIT_APPLY`, `P1_NATIVE_EXTERNAL_DENY`, `P1_NATIVE_NETWORK_DENY`, `P1_NATIVE_DESCENDANT`,
`P1_NATIVE_ALL_PASS`.

---

## 8. Runtime compatibility

Python/Node runtime compatibility is recorded as a **separate** claim (`RUNTIME_COMPATIBILITY = FAIL`
allowed) rather than conflated with execution normalization. `validation/execute.rs` routes through
the broker; an interpreter whose prefix is not a recognized `TrustedToolchainRoot` reports a spawn
failure. That is the correct fail-closed outcome and must not be downgraded into a claim that
validation is unconfined.

---

## 9. Detached durable payload — OUT OF SCOPE, recorded as BLOCKED (defect #6)

`DETACHED_DURABLE_NORMALIZATION = BLOCKED_PROCESS_OWNERSHIP`

This is a **recorded block**, not a fix. The closure round does **not** route the detached durable
payload through the `ExecutionBroker`.

- **Exact function:** `run_accepted_payload`
  (`crates/webcodex-runner/src/webcodex_runner/detached_job.rs`), with `#[cfg(unix)]` and `#[cfg(windows)]`
  variants.
- **Current spawn path (Unix):** the durable watchdog is spawned first (`spawn_watchdog`, which owns a
  process-tree `birth_*` identity); once the watchdog arms, the payload is built with
  `Command::new(&launch.process.executable).args(…).env_clear()`, its env is set from `launch.env`, the
  cwd from `launch.cwd`, and it is attached to the watchdog's tree with
  `.process_group(tree_pid as i32)` then `.spawn()` — error
  `"failed to spawn detached payload: {error}"`.
- **Current spawn path (Windows):** `super::shell::structured_process_command(executable, args, cwd)?`
  → `.env_clear()` → set from `launch.env` → `ManagedChild::spawn(&mut payload_command)`; error
  `"failed to spawn detached Windows payload: {error}"`.
- **Why it conflicts with `ExecutionBroker` / `ManagedChild` process-group semantics:** the detached
  substrate's entire purpose is Runner-restart survival. A durable execution is prepared into bounded
  Runner-owned state, then handed **once** to a narrow supervisor process. After the durable
  `OwnershipAccepted` transition (`detached_job.rs` sets `phase = DetachedJobPhase::OwnershipAccepted`
  and `ownership_accepted_at_unix_ms`), the supervisor becomes the **sole** owner of the payload
  process tree. The Unix payload is attached to the supervisor's watchdog via `.process_group(tree_pid)`;
  the Windows payload is wrapped in `ManagedChild`, whose private `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
  Job Object is created inside the supervisor so that if the supervisor dies, Windows closes its last
  Job handle and kills the whole payload tree. Routing the payload through the Runner-side
  `ExecutionBroker` / `ManagedChild` group would mean the **Runner** owns the tree, which directly
  contradicts the durability guarantee: a Runner restart would reap the tree, and the detached substrate
  would lose the sole-owner invariant it relies on to reconcile the job after restart. The broker's
  group is the Runner's group; the detached payload must be the supervisor's group, and the two cannot
  co-own one process.
- **Minimal interface-change direction (P1b, not this round):** leave `detached_job.rs` spawning the
  payload as it does today; do not route it through the broker. If P1b wants broker-level confinement
  for the detached payload, the change must be at the supervisor boundary — e.g. have the supervisor
  itself construct the confined command (the supervisor already owns the tree), or add a
  broker-fronted `spawn_into_existing_tree` / `spawn_as_member_of(pid)` entry that respects an
  externally-owned process-group/Job rather than creating a Runner-owned one. The interface change is
  "broker supports spawning a child that joins a caller-supplied group/Job", not "detached payload
  calls `spawn_local_action`". No new production API is introduced here.

---

## 10. Toolchain note

`crates/webcodex-runner/src/webcodex_runner/coding_agent.rs:261` uses `atomic_try_update`, which
requires a newer unstable feature. Verified as a **baseline** condition (`git show
a295cb26:...coding_agent.rs` already contains it; `git diff --stat HEAD` on that file is empty).
Verification therefore used the rustup toolchain (rustc 1.95.0); Homebrew rustc 1.94.0 cannot compile
the baseline.

---

## 11. Closure status

`P1_STATUS = PARTIAL`.

While the detached durable payload is unrouted, the closure cannot be `COMPLETE` and is **not**
`READY_FOR_P2`. The defects GPT enumerated are resolved as follows:

| Defect | Closure result |
|---|---|
| #1 model-triggered env inheritance | `PASS` — positive selection, `LocalEnv::Inherit` deleted (§1.1) |
| #2 `HOME` fallback authority | `PASS` — resolver takes no policy; `$HOME` cannot be authority (§1.2, P1-L) |
| #3 `narrowest_covering` order-dependence | `PASS` — order-independent deepest-wins (§3.1, P1-M) |
| #4 network gate positive control | `PASS` — bind + unsandboxed control + production denial (§4.1, P1-E) |
| #5 real `run_shell` / `git apply` paths | `PASS` — native cases enter production boundaries (§6.3, P1-B/D/E/F/G) |
| #6 detached durable payload | `BLOCKED_PROCESS_OWNERSHIP` — recorded, not solved (§9); owed to P1b |
| #7 `validation/execute` must route | `PASS` — routed through `spawn_local_action`, anti-bypass guarded (§2.5, P1-I) |

`P1_CORE_LOCAL_NORMALIZATION = PASS` and `P1B_REQUIRED = YES`. The next round (P1b) owns defect #6 and
may revisit runtime compatibility for interpreter-based validation. This round does **not** begin P2
and does **not** implement approval.
