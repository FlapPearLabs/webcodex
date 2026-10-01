# P1 Execution Normalization — Closure Report

- **Baseline branch:** `spike/webcodex-seatbelt-profile`
- **Baseline HEAD:** `a295cb2612c71c9ced01c74010f629d2f679c54d`
- **Implementation branch:** `impl/webcodex-execution-normalization-p1`
- **Closure branch (this round):** `fix/webcodex-execution-normalization-p1-closure`
- **Closure HEAD:** `739ac79e16f913ccc18d89c21d78e557988489fc` (the branch is created from this
  baseline; the closure delta is the set of commits on `fix/webcodex-execution-normalization-p1-closure`
  above this SHA)
- **Scope:** P1 local execution core + the closure defects GPT identified + the harness-trustworthiness fixes
- **P1_CLOSURE_STATUS:** `PARTIAL` (defects #1–#5 and #7 closed; #6 — detached durable payload — recorded as blocked, not solved)
- **P1_STATUS:** `PARTIAL`
- **P1B_REQUIRED:** `YES`
- **P1_CORE_LOCAL_NORMALIZATION:** `PASS`
- **READY_FOR_NORMALIZATION / READY_FOR_P2:** `NO` — see §11

### Harness-trustworthiness round (role `WEBCODEX_P1_CLOSURE_HARNESS_FIXER`)

This round changes **no** production behaviour. It fixes five defects in the evidence and reporting
that the previous round's own conclusions rested on — i.e. it makes the gate capable of failing.

| # | Defect | Fix |
|---|---|---|
| 1 | Network gate could **false-pass**: the `TcpListener` was moved into an accept thread that exited before the sandboxed measurement, so the sandboxed connect met a *closed* port — which an unconfined process would also have met | listener stays parent-owned; liveness re-probed before **and** after the sandboxed attempt; new deterministic test pins both directions (§4.1) |
| 2 | Native script collapsed two suites into one `PIPESTATUS[0]`, so a failing runner suite plus a passing workspace suite read as `rc=0` | two independent invocations, `P1_RUNNER_TEST_RC` / `P1_WORKSPACE_TEST_RC`, both required for `P1_NATIVE_ALL_PASS`; rule verified by a host-independent `--self-check` (§6.4.1) |
| 3 | `P1_NATIVE_GIT_APPLY` was emitted by `git_broker`'s own fidelity test, which never enters the checkpoint layer — so "the checkpoint path is covered" was an unestablished claim | new in-module `#[cfg(test)]` case drives the **private** `workspace_checkpoint::git_apply` against a real repo and patch; broker test demoted to informational `P1_NATIVE_GIT_BROKER_FIDELITY` (§6.1.1) |
| 4 | `run_pyright`'s second parameter was named `authority_root` while both `run_bounded` and every call site treat it as the **project registry directory** — a name that invites a future maintainer to pass a project root, which silently makes every validation spawn refuse | renamed `project_registry_dir`, with a comment stating that it is a registry directory and not a project root (§2.5) |
| 5 | Target accounting mixed *target categories* with *launch sites*, reported `P1_REQUESTED_TARGETS_BLOCKED=0` while a blocked category existed, and claimed a repo-wide unrouted count (`=4`) that the same report contradicted | `P1_REQUESTED_TARGETS_TOTAL=6 / ROUTED=5 / BLOCKED=1`, `P1_ROUTED_PRODUCTION_LAUNCH_SITES=7` measured separately, and `TOTAL_KNOWN_MODEL_TRIGGERED_UNROUTED_SURFACES=NOT_FULLY_ENUMERATED` with all nine named surfaces listed (§5, §5.1) |

Defect 3 also retires a specific false statement: an earlier version of this report asserted
`REAL_CHECKPOINT_GIT_APPLY_PATH=YES` on the strength of the broker test. That assertion was wrong —
the broker test never calls the checkpoint wrapper — and it is withdrawn. The checkpoint path is now
covered by a test that actually enters it (§6.1.1).

Status is unchanged and deliberately not upgraded: `P1_STATUS=PARTIAL`, `P1B_REQUIRED=YES`.

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
3. the listener is re-probed with a second unsandboxed connect —
   `LISTENER_STILL_LIVE_BEFORE_SANDBOX`;
4. only then does the **production** `run_shell` path run the same connect — `SANDBOX_PROCESS_STARTED`,
   then `SANDBOX_CONNECT_DENIED`;
5. after the sandboxed attempt the listener is probed once more —
   `LISTENER_SURVIVED_SANDBOX_ATTEMPT`.

A PASS requires all five. If step 1, 2 or 3 fails there is nothing to measure, so the case reports
the failure and never a vacuous `SANDBOX_CONNECT_DENIED=true`.

#### Why steps 3 and 5 exist: a real false-pass, found and closed

An earlier version moved the `TcpListener` **into a thread** that accepted the unsandboxed
positive-control connection and then exited. That dropped the listener **before** the sandboxed
measurement. The sandboxed child therefore met a *closed* port — and so would an entirely
unconfined process, because the port was closed for everybody. The case could not distinguish
"the profile denied network" from "nothing is listening any more", and it **false-passed**.

The fix is that the listener is owned by the parent frame for the whole test and never moved, with
steps 3 and 5 as the regression guard. The guard is itself pinned by a separate deterministic test,
`liveness_probe_distinguishes_a_live_listener_from_a_dropped_one`, which asserts both directions on
a real socket with no sandbox involved: a listener this process still owns **is** reachable, and the
same port after `drop` **is not**. Without the second half the guard would be a tautology — a probe
that could not fail would prove nothing.

This removes the prior bug where `exec 3<>/dev/tcp/127.0.0.1/1` "passed" on any host because port 1
is closed whether or not a sandbox exists.

---

## 5. Surface accounting (re-measured, not estimated)

Counted from the source at this commit, production code only (test modules excluded). The single
rolled-up `P1_REMAINING_MODEL_TRIGGERED_COUNT=4` is **replaced** by the five separated metrics the
closure requires, each named individually.

```text
P1_REQUESTED_TARGETS_TOTAL=6
P1_REQUESTED_TARGETS_ROUTED=5
P1_REQUESTED_TARGETS_BLOCKED=1
P1_ROUTED_PRODUCTION_LAUNCH_SITES=7
TOTAL_KNOWN_MODEL_TRIGGERED_UNROUTED_SURFACES=NOT_FULLY_ENUMERATED
```

**The two units are different and must not be mixed.** `P1_REQUESTED_TARGETS_*` counts the six
*target categories* the P1 request named. `P1_ROUTED_PRODUCTION_LAUNCH_SITES` counts the concrete
production call sites that reach the broker at this commit. One category can carry several launch
sites (`run_shell` alone has two), and one launch site can be reached from more than one category.
Reporting a launch-site count as a target count — or the reverse — is what made the previous version
of this section wrong.

**Requested targets (6) = routed (5) + blocked (1).** Each category named, with the launch sites
measured underneath it:

| # | Requested target category | Status | Production launch sites reaching the broker |
|---|---|---|---|
| 1 | `run_shell` | ROUTED | 1 (`shell.rs:3184`) |
| 2 | local jobs | ROUTED | 2 (`job_manager.rs:2999` start, `job_manager.rs:3266` queue advance) |
| 3 | detached durable payload | **BLOCKED** | 0 — see §9 |
| 4 | `workspace_checkpoint` git | ROUTED | 2 (`workspace_checkpoint.rs:357` `git_output`, `:396` `git_apply`) |
| 5 | `project_context` git | ROUTED | 1 (`project_context.rs:610` `bounded_git_output`) |
| 6 | validation execute | ROUTED | 1 (`validation/execute.rs:101`) |

`P1_ROUTED_PRODUCTION_LAUNCH_SITES=7` is the sum of the routed rows: 1 + 2 + 2 + 1 + 1. It was
measured by enumerating production call sites of `spawn_local_action` and `git_broker::run_git*`,
excluding `#[cfg(test)]` modules and excluding the chokepoint's own definition
(`local_execution.rs:309`). It is a launch-site count and must not be read as a target count.

The single blocked category is the **detached durable payload**. It is not routed, not approximated,
and not counted as routed: `DETACHED_DURABLE_NORMALIZATION = BLOCKED_PROCESS_OWNERSHIP` (§9). The
previous `P1_REQUESTED_TARGETS_BLOCKED=0` was wrong on its face — a blocked category existed and was
being reported as zero.

Note on what these sites cover: because the shell command builders all converge on
`shell.rs:3184`, one call site carries the `run_shell` main path, the prepared and explicit shell
variants, the shell-job variants, and the validation-step variant. The count is of *launch sites
reaching the broker*, not of command variants — those are pinned by the structural guard in §6.2,
which names each routed builder function individually.

`project_context.rs:1125` also calls `git_broker::run_git`, but it sits inside a `#[cfg(test)]` module
and is excluded from this count.

### 5.1 Known model-triggered surfaces still NOT routed

`TOTAL_KNOWN_MODEL_TRIGGERED_UNROUTED_SURFACES=NOT_FULLY_ENUMERATED` replaces the previous
`=4`. That number was a false count: it was derived from the Runner-local list only, while the same
report named several further unrouted surfaces (detached payload, persistent interactive shell, LSP,
browser/CDP, coding-agent children). A repo-wide count that the report itself contradicts is worse
than no count. Every surface named anywhere in this report is listed below; the enumeration is
**not claimed to be complete**, and precision is preferred over a fabricated total.

Model-triggered, unrouted, named:

1. **Detached durable payload** — `detached_job.rs`. Requested target category #3; blocked
   (`BLOCKED_PROCESS_OWNERSHIP`, §9).
2. **Persistent interactive shell** — `shell.rs:1253` `run_prepare_command` (profile `init_script`;
   authority is the profile configuration, not a model-authored command).
3. **Profile environment snapshot** — `shell.rs:1460` `capture_profile_env_snapshot` (Runner-owned
   control-plane probe).
4. **Tool Plugin / MCP provider launcher** — `shell.rs:1657` `get_or_prepare`. P1 excludes plugin/MCP
   provider processes by scope.
5. **SSH / remote shell jobs** — `job_manager.rs:3462` `start_ssh_shell_job`. P1 excludes SSH.
6. **LSP** — language-server child processes; excluded by scope.
7. **Browser / CDP** — browser and Chrome DevTools Protocol drivers; excluded by scope.
8. **Coding-agent children** — `coding_agent.rs` ACP/Hermes/Codex child processes; excluded by scope.
9. **Project catalog / managed-worktree git** — `projects/catalog.rs:361` `run_git_bounded_with_program`
   spawns `Command::new(program)` **directly**, without the broker. Callers include
   `projects/lifecycle.rs:681` (`git init`), `projects/managed_worktree.rs:91,103,870`. This is
   Runner-owned project management rather than a model-authored command, which is why it is not one
   of the six requested targets — but it is a real git surface that is **not** broker-routed, and
   the earlier report did not name it at all.

Items 2–5 are asserted still present by `known_unrouted_surfaces_are_still_present_and_named`, so
that list cannot silently drift. That test also asserts the *absence* of `RUNTIME_COMPATIBILITY_TODO`
from `validation/execute.rs` (it is routed) and the *presence* of the detached-payload spawn error
string (it is still unrouted).

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
| P1-G | The **checkpoint wrapper** `workspace_checkpoint::git_apply` applies a real patch through `git_broker` → `ExecutionBroker`, semantics preserved | `workspace_checkpoint.rs` (in-module `#[cfg(test)]`) → `P1_NATIVE_GIT_APPLY` |
| P1-G′ | Broker-level `git apply` fidelity, **without** the checkpoint layer | `git_broker.rs` → `P1_NATIVE_GIT_BROKER_FIDELITY` (informational, not the P1-G gate) |
| P1-H | The git helper gains no authority beyond the project root | `git_broker.rs` |
| P1-I | Validation execute is now routed; no `Command::new(` / `ManagedChild::spawn(` in production region | `normalization_p1_tests.rs` |
| P1-J | Missing trusted context fails before process creation with a stable code | `normalization_p1_tests.rs` |
| P1-K | Model-triggered env is positively selected, not inherited | `normalization_p1_tests.rs` |
| P1-L | `HOME` is not a project authority (resolver takes no policy) | `normalization_p1_tests.rs` |
| P1-M | `narrowest_covering` is order-independent | `normalization_p1_tests.rs` + `workspace_authority.rs` |

P1-G and P1-H live in `webcodex-workspace` on purpose: the git broker is `pub(crate)` there, and a
test that reimplemented it would prove nothing about the code that ships.

### 6.1.1 Why P1-G is measured at the checkpoint wrapper, not at the broker

The broker's own test drives `git_broker::run_git` directly. That proves two links of the chain —
`git_broker` → `ExecutionBroker` — and fidelity of git patch semantics underneath the broker. It
proves **nothing** about the layer the model actually reaches, because it never enters
`workspace_checkpoint` at all:

```text
model-authored patch
  → workspace_checkpoint::git_apply     ← argv assembly, stdin payload, success/failure reading
    → git_broker::run_git                ← plan derivation, trusted-root resolution
      → ExecutionBroker                  ← profile compilation, launcher
        → sandbox-exec → git
```

A claim that the checkpoint path is covered by the broker's test is **not** established by that test.
`git_apply` is also `private`, so an external integration test could not call it even in principle.

The gate is therefore measured by `workspace_checkpoint::tests::checkpoint_git_apply_applies_a_real_patch_through_the_broker`,
an in-module `#[cfg(test)]` case that calls the private `git_apply(root, &[], patch)` directly against
a real throwaway git repository and a real textual patch, then asserts the patched file is on disk with
the expected bytes — `Ok(())` alone is not accepted as success. `git_apply` stays private: promoting it
to `pub` so a test could reach it would add an outward-facing surface for no production reason.

The three outcomes are never collapsed:

| Observation | Verdict |
|---|---|
| patch applied, file bytes match | `PASS` |
| git started and rejected the patch | `FAIL`, and the test fails |
| launcher or kernel refused; git never ran under the profile | `ENV_BLOCKED` |

`ENV_BLOCKED` is not a pass and is never converted into one. On a host that refuses `sandbox_apply`,
this case establishes nothing — which is exactly what it reports. The lower-level broker fidelity test
is retained and still run, but it now emits its own marker,
`P1_NATIVE_GIT_BROKER_FIDELITY`, so a green broker test can never be mistaken for evidence about the
checkpoint layer.

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
runner normalization_p1 15 passed, 0 failed       (14 at closure + 1 liveness regression guard)
runner validation       59 passed, 0 failed
bash -n native-normalization-p1.sh   clean
git diff --check        clean
secret scan             no credentials in the diff
```

On this host the native-facing cases report `ENV_BLOCKED` — the host refuses `sandbox_apply`, so no
profile was ever applied and nothing was confirmed. That is the correct, non-pass result; see §7.

**Toolchain note (measured, not assumed).** The `webcodex-runner` test build fails to compile under
the Homebrew `rustc 1.94.0` that comes first on this machine's `PATH`:

```text
error[E0658]: use of unstable library feature `atomic_try_update`
   --> crates/webcodex-runner/src/webcodex_runner/coding_agent.rs:261
```

That call site is inside a `#[cfg(test)]` block in a file this work does not touch, and the failure
is a toolchain-version artefact, not a defect introduced here: `AtomicU32::try_update` is accepted by
the rustup toolchains on this machine (`1.95.0`, `1.98.1`) and rejected by Homebrew's `1.94.0`. All
results above were produced with `PATH="$HOME/.cargo/bin:$PATH"`. This is recorded rather than
silently worked around, because "the suite does not build" and "the suite is red" are different
facts and only one of them is a code problem.

### 6.4.1 The native smoke script aggregates two suites, not one pipeline

`research/spikes/native-normalization-p1.sh` runs two suites and captures **two independent exit
codes**:

```text
P1_RUNNER_TEST_RC=<rc>
P1_WORKSPACE_TEST_RC=<rc>
```

and `P1_NATIVE_ALL_PASS=true` requires **all** of:

1. `RUNNER_RC == 0`, **and**
2. `WORKSPACE_RC == 0`, **and**
3. every required `P1_NATIVE_*` marker reported `PASS`, **and**
4. `ENV_BLOCKED == 0`.

#### The bug this fixes

The previous version ran both suites inside one subshell and read a single code:

```bash
( cargo test RUNNER ; cargo test WORKSPACE ) | tee "$RUN_LOG"
TEST_RC="${PIPESTATUS[0]}"
```

`PIPESTATUS[0]` is the exit status of the **subshell**, which is the status of its *last* command. A
runner suite that failed every case, followed by a workspace suite that passed, yields `TEST_RC=0`.
The gate would then report the *absence of a later failure* as a pass — the most dangerous possible
reading, because the runner suite is the one that carries the production `run_shell` confinement
cases. The two suites are now separate commands with separate statuses, so neither can mask the other.

#### The aggregation rule is itself tested

`all_pass` is not trusted because it looks correct. `bash research/spikes/native-normalization-p1.sh --self-check`
exercises it with no cargo, no sandbox and no host dependency, and fails if any of these is accepted:

* `runner rc=1` with `workspace rc=0` — the exact defect above;
* `workspace rc=1`;
* a non-`PASS` marker;
* `ENV_BLOCKED=1`;
* both suites failing.

It also asserts the inverse — that a genuinely clean `(0, 0, 0, 0)` **is** accepted — so the rule
cannot be trivially "always false". The full run aborts with exit 2 if the self-check does not hold,
rather than reporting a verdict derived from an unverified rule. Measured on this host:

```text
SELF-CHECK PASSED: ALL_PASS requires RUNNER_RC=0 AND WORKSPACE_RC=0 AND no failures AND no ENV_BLOCKED
```

The script also runs the broker-level fidelity test alongside the checkpoint-wrapper test and reports
it as `P1_NATIVE_GIT_BROKER_FIDELITY`, explicitly marked informational, so it is never counted
toward `P1_NATIVE_ALL_PASS`.

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
