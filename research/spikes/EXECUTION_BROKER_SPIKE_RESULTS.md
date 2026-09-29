# EXECUTION_BROKER_SPIKE_RESULTS.md

Small architecture spike: can per-action sandboxing replace a runner-level
UNION sandbox, and which Codex-sandbox reuse route is viable?

```
SPIKE_STATUS = PARTIAL
```

`PARTIAL`, not `PASS`: the design questions are answered and the module
compiles and is tested, but the five enforcement experiments could not be
measured on this host because the kernel refuses Seatbelt allow-list profiles.
`UNKNOWN` is reported as `UNKNOWN`. See §7.

```
BASE_SHA   = eda5a659df0710b84de5becc9b3bebbb3fc51575
BRANCH     = spike/webcodex-execution-broker-sandbox
HOST       = macOS 26.2 (25C56), aarch64
TOOLCHAIN  = cargo 1.95.0 / rustc 1.95.0
```

---

## 1. Question A — per-action broker, or runner-level UNION sandbox?

**A = per-action broker. Feasible, and the code supports the seam.**

The alternative — compute one profile for the whole runner and confine the
runner process itself — was rejected for a reason visible in the code, not a
stylistic one: the runner **is** the control plane. It holds the model
connection, the MCP dispatch loop, and every secret the task needs. Confining it
means the control plane cannot read what it manages. That is the same structural
argument that decides question B's location, so it is worth stating once here
and not twice.

The per-action form avoids the failure mode that makes UNION attractive and
then dangerous. Under UNION, the effective authority of every action is the
intersection computed once; a surface that fails to narrow inherits the widest
plan, and the bug is a silent over-grant. Under per-action, the plan travels
with the action, so two concurrent actions can hold different authority and a
missing narrowing fails closed.

What the spike demonstrated:

- `SandboxPlan` is a **value**, not a policy object. It renders to a profile on
  demand and refuses rather than degrading.
- Two plans naming different roots render profiles that do not mention each
  other's data (`two_plans_are_independent_at_profile_level`). This is the
  per-action property at the level where it can be checked without the kernel.
- The control plane is never placed inside a profile
  (`control_plane_remains_outside_the_sandbox_and_can_issue_a_different_plan`).

**The finding that matters more than the confirmation:** the seam is *partial*.
`ManagedChild::spawn_with_options` is a chokepoint for 20 production spawn
sites, and **18 more bypass it entirely** — on Unix, `ssh.rs:1142`,
`remote_shell.rs:109`, `detached_job.rs:1924/1941/2409`,
`workspace_checkpoint.rs:358/398`, `project_context.rs:598`,
`persistent-shell/src/lib.rs:1965`, plus the self-update and CLI
control-plane paths. They bypass it because they need `Child::id()` and process
groups they manage themselves.

So a broker placed only inside `ManagedChild` would confine `run_shell` and
leave `git apply` unconfined. Full method and per-site table:
`SPAWN_INTEGRATION_NOTES.md §2`.

---

## 2. Question B — which Codex sandbox reuse route?

**B = SUBPROCESS ADAPTER (B3).**

| Route | Compiles | Deps | LOC | Result |
|---|---|---|---|---|
| B1 pinned git dep | **NO** | 15+ | 0 | **FAIL** — `workspace = true` unresolvable externally (`tokio-tungstenite` `proxy`) |
| B2 bounded vendor | not attempted | 5 | ~2,894 | **REJECTED** — not bounded; consumes Codex's policy model |
| B3 subprocess adapter | **YES** | **0** | **228** | **SELECTED** |

```
CODEX_SANDBOX_REUSE = SUBPROCESS_ADAPTER
```

Reasoning, evidence, and the rejected routes in full:
`CODEX_SANDBOX_REUSE_SPIKE.md`. The one-line version: B1 is blocked by a
workspace-resolution problem no external crate can fix, B2's reusable entry
point takes Codex's `FileSystemSandbeltPolicy` and so drags the policy model
with it, and B3 talks to `/usr/bin/sandbox-exec`, which is a documented macOS
interface rather than a Codex invention — so it copies nothing and owes nothing.

---

## 3. ExecutionBroker design

```
crates/webcodex-process/src/execution_broker/mod.rs   227 LOC
```

```
runner (control plane, never sandboxed)
  -> SecurityBroker would decide a plan           [NOT in this spike]
  -> SandboxPlan { writable_roots, readable_roots, network }
  -> ExecutionBroker::spawn(command, cwd, &plan)
       plan.to_sbpl()  ->  refuse? Err(PlanRefused)  [before any process exists]
       /usr/bin/sandbox-exec -p <profile> <program> <args>
       ManagedChild::spawn_with_options(...)
  -> ManagedChild (unchanged: wait, try_wait, terminate_tree)
```

Properties that are structural, and therefore true regardless of kernel
support:

1. **Refusal precedes execution.** A plan that cannot be rendered returns
   `Err(PlanRefused)` during profile construction. No process is created.
   (`refused_plan_never_reaches_spawn`)
2. **Empty plan is refused**, not silently widened. An empty allow-list that
   defaulted to `(allow default)` would be a total sandbox bypass wearing the
   appearance of confinement.
3. **`NetworkPolicy::Allow` is refused**, because no proxy exists in this
   spike. An unimplemented allow path must be a hard error, never a silent
   downgrade to `Deny`.
4. **Path quoting is escaped**, and the test counts top-level rules (exactly 7)
   so an injected `(allow default)` inside a quoted path would be detected
   rather than merely improbable.
5. **The broker is a zero-sized value.** It holds no state and is not consumed
   by a spawn, so the same control-plane process can issue plan A, then plan B.
6. **Environment is cleared**, then `PATH` and `HOME` are re-set explicitly.
   Inheriting the runner's environment wholesale would hand the sandboxed
   action every credential the runner holds.

Property 2 is the one worth arguing for. A sandbox that fails open on a
malformed plan is worse than no sandbox, because it produces a passing test
suite and a false sense of coverage.

---

## 4. Tests A–E

```
$ cargo test -p webcodex-process --test execution_broker -- --nocapture
running 12 tests
...
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured
```

Seven tests are host-independent and assert for real. Five are the enforcement
experiments and **all five returned `ENV_BLOCKED`** on this host.

| Test | Question | Result | What was actually established |
|---|---|---|---|
| A | workspace read/write | **PARTIAL** | not measurable here |
| B | external read denied | **PARTIAL** | not measurable here |
| C | process tree inherits profile | **PARTIAL** | not measurable here |
| D | same action, different profiles → different outcome | **PARTIAL** | profile *text* differs and is independent; *enforcement* difference unmeasured |
| E | network denied | **PARTIAL** | not measurable here |

`ENV_BLOCKED` is emitted by a runtime probe
(`host_allows_allowlist_profiles()`) that runs a real `sandbox-exec` with a
narrowing profile and checks the exit status. It is not a hardcoded skip: on a
host that accepts allow-lists these five tests execute their assertions. The
reason for the early return is that a test which cannot fail is not evidence —
a silent pass here would have produced a green suite and a false claim.

The environment limitation, measured directly:

```
$ sandbox-exec -p '(version 1)(allow default)' /usr/bin/true          -> rc 0
$ sandbox-exec -p '(version 1)(allow default)(deny file-read*)' …     -> rc 71
$ sandbox-exec -p '(version 2)(allow default)(deny file-read*)' …     -> rc 71
$ sandbox-exec -p '(version 1)(allow file-read*)' /usr/bin/true       -> rc 71
   sandbox-exec: sandbox_apply: Operation not permitted
```

`(allow default)` applies. **Every narrowing profile is refused with EPERM.**
Tried: v1 and v2 syntax, deny-only and allow-only forms, literal subpath allows.

What does work on this host, and is therefore established:

- `sandbox-exec` applies a profile in the child **before `exec`**;
- descendants inherit it (`/bin/sh -c '…'` ran and its child read a file);
- the mechanism is live. Only *allow-list enforcement* is unavailable.

Test C's specific claim — inheritance — is the one part of C that this host
cannot confirm, because inheritance of a permissive profile proves nothing
about inheritance of a restrictive one.

---

## 5. Control plane outside the sandbox

```
CONTROL_PLANE_OUTSIDE_SANDBOX = YES (structurally verified)
```

`ExecutionBroker` is a unit struct with no fields. Nothing in the calling
process is placed inside a profile; only the `sandbox-exec` child is. The
test issues three different plans from one untouched broker and asserts each
renders independently. This does not depend on kernel support and is the one
architectural property the host limitation does not touch.

---

## 6. Normalization TODO

Every production spawn site, and where it must go. `ManagedChild` sites can be
routed by teaching `SpawnOptions` to carry a plan; the direct sites need their
own construction changed, which is why they are listed separately.

| # | Spawn surface | Current spawn path | Model-triggered? | Future route through ExecutionBroker |
|---|---|---|---|---|
| 1 | `shell.rs:1187` profile prepare | `ManagedChild::spawn` | yes (pre-action) | via `SpawnOptions` |
| 2 | `shell.rs:3030` `run_shell` | `ManagedChild::spawn` | **yes — primary** | via `SpawnOptions` |
| 3 | `job_manager.rs:2981/3246/3439` | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 4 | `detached_job.rs:2677` payload | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 5 | `detached_job.rs:2973` supervisor | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 6 | `ssh.rs:1146` (Windows) | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 7 | `remote_shell.rs:116` (Windows) | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 8 | `mcp_gateway.rs:622` provider | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 9 | `plugin.rs:1239` provider | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 10 | `coding_agent.rs:1465` | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 11 | `external_tools.rs:1129` | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 12 | `projects/catalog.rs:371` | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 13 | `validation/execute.rs:53` | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 14 | `main.rs:2299` | `ManagedChild::spawn` | no (control plane) | leave outside by design |
| 15 | `webcodex-lsp/supervisor.rs:214` | `ManagedChild::spawn` | yes (semantic nav) | via `SpawnOptions` |
| 16 | `webcodex-lsp/supervisor.rs:1980` | `command.spawn` **direct** | yes | rewrite construction → broker |
| 17 | `persistent-shell/windows.rs:280` | `ManagedChild::spawn` | yes | via `SpawnOptions` |
| 18 | `persistent-shell/lib.rs:1965` | `command.spawn` **direct** | yes | rewrite construction → broker |
| 19 | `ssh.rs:1142` (Unix) | `command.spawn` **direct** | yes | rewrite construction → broker |
| 20 | `remote_shell.rs:109` (Unix) | `command.spawn` **direct** | yes | rewrite construction → broker |
| 21 | `detached_job.rs:1924/1941` | `command.spawn` **direct** | yes | rewrite construction → broker |
| 22 | `detached_job.rs:2409` payload | `command.spawn` **direct** | yes | rewrite construction → broker |
| 23 | `workspace_checkpoint.rs:358` `git_output` | `command.spawn` **direct** | yes | rewrite construction → broker |
| 24 | `workspace_checkpoint.rs:398` `git_apply` | `command.spawn` **direct** | yes (**writes via patch**) | rewrite construction → broker |
| 25 | `project_context.rs:598` `bounded_git_output` | `command.spawn` **direct** | yes | rewrite construction → broker |
| 26 | `webcodex-environment/process.rs:42` | `self.spawn()` **direct** | no (self-update) | out of scope, document |
| 27 | `environment/installer_unix.rs:442` | `command.spawn` **direct** | no (privilege drop) | out of scope, document |
| 28 | `environment/upgrade.rs:2563` | `command.spawn` **direct** | no (verification) | out of scope, document |
| 29 | `environment/unified_update/installer.rs:491` | `command.spawn` **direct** | no (installer) | out of scope, document |
| 30 | `webcodex-cli/connect/process.rs:568` | `command.spawn` **direct** | no (control plane) | leave outside by design |
| 31 | `webcodex-cli/connect/process.rs:655` | `command.spawn` **direct** | no (control plane) | leave outside by design |
| 32 | `webcodex-cli/controller.rs:767` | `command.spawn` **direct** | no (control plane) | leave outside by design |
| 33 | `browser/cdp.rs:385` | `ManagedChild::spawn` | no (drives host UI) | leave outside by design |
| 34 | `webcodex-process/unix.rs:57` | the chokepoint itself | n/a | becomes broker-aware |
| 35 | `webcodex-process/windows.rs:168` | the chokepoint itself | n/a | becomes broker-aware |

```
NORMALIZATION_TODO_COUNT = 20 model-reachable sites to route
                            (15 via SpawnOptions, 5 requiring construction rewrite)
                            +  4 control-plane / host-UI sites deliberately left outside
                            +  4 self-update sites out of scope, documented
```

`git_apply` (#24) deserves attention: it is model-reachable, it is on the
direct-spawn list, and it **writes to the workspace from stdin**. In the
current code nothing confines it.

---

## 7. Known limitations

1. **Enforcement is unmeasured on this host.** A–E are `ENV_BLOCKED`. The
   design is validated; the enforcement is not. Nothing here should be read as
   "per-action sandboxing works on macOS".
2. **`ExecutionBroker` is wired to nothing.** It is defined, exported, and
   tested, but no production code calls it. `run_shell` is *not* sandboxed by
   this branch.
3. **20 model-reachable spawn sites are unrouted**, per §6.
4. **`NetworkPolicy::Allow` is unimplemented**, so any action needing egress is
   refused. This is correct for a spike and wrong for production.
5. **macOS only.** Non-macOS returns `Err(UnsupportedPlatform)`. No Linux
   (bwrap/landlock/seccomp) or Windows (AppContainer) equivalent exists.
6. **No approval path, deliberately.** Per the brief, human approval, Unix
   socket, HMAC, auto-reviewer, `APPROVE_FOR_ME`, session grants, danger mode,
   and a policy engine are all out of scope. `SecurityBroker` does not exist;
   the plan in this spike is a hand-constructed value.
7. **The profile is a string.** `to_sbpl` renders text. Path escaping is tested
   for quotes; other metacharacters are not exhaustively fuzz-tested.
8. **A first measurement pass in this spike was wrong** and had to be
   corrected: a literal `Command::spawn` search reported "zero direct spawns in
   production", missing all 18 because they are written as method calls on
   local bindings. `SPAWN_INTEGRATION_NOTES.md §1` states the correction rather
   than quietly dropping it. It is recorded because the wrong number was
   nearly used to justify a smaller normalisation plan.
9. **Test B/D/E depend on a fixture outside the repo.** The "outside" file
   lives at `/tmp/webcodex-sandbox-spike/outside.txt`, so those tests need that
   path to exist before they can assert anything. On a host where allow-list
   profiles *are* accepted, tests B, D, and E would fail for want of the
   fixture rather than for want of enforcement. The fixture is created by the
   spike, not by the test. A production version of this suite must create it
   in a temp dir.

---

## 8. Next implementation slice

Recommended next slice, in order:

1. **Route the `ManagedChild` family (15 sites) through `SpawnOptions`.**
   Smallest change with the largest coverage: one chokepoint, and every caller
   inherits it. Make the plan **mandatory** on model-reachable paths so a
   missing plan fails closed rather than defaulting to unrestricted.
2. **Rewrite the five direct model-reachable sites** (#16, #18–#25) to build
   their `Command` through the broker. `git_apply` first — it is the only one
   that writes from stdin and is model-reachable.
3. **Implement `NetworkPolicy::Allow` behind a proxy** rather than by widening
   the profile. Direct egress cannot be granted selectively in Seatbelt; a
   local proxy is the only mechanism that can mediate it.
4. **Re-run A–E on a host that accepts allow-list profiles.** Until then
   enforcement claims are `UNKNOWN`. If no such host is available, the Linux
   (bwrap) path becomes more attractive than macOS, because its primitives are
   scriptable and testable in CI.
5. **Then, and only then**, add the `SecurityBroker` that decides plans. It
   should be added against a routing that already fails closed, not before —
   a policy engine in front of an unrouted spawn surface authorises nothing.

Deliberately not next: OpenCode/Hermes/Pi ports, Linux support, danger mode,
session grants.

---

## 9. Verdict

```
SPIKE_STATUS = PARTIAL

A. per-action broker vs runner-level UNION  -> PER_ACTION_BROKER, feasible
B. Codex sandbox reuse route               -> SUBPROCESS_ADAPTER (B3)

EXECUTION_BROKER_COMPILES             = YES
REAL_WEBCODEX_PATH_INTEGRATED         = NO   (module exists; no production caller)
WORKSPACE_RW                           = PARTIAL (ENV_BLOCKED)
EXTERNAL_DENY                          = PARTIAL (ENV_BLOCKED)
PROCESS_TREE_INHERITANCE               = PARTIAL (ENV_BLOCKED)
PER_ACTION_PROFILE_DIFFERENCE          = PARTIAL (profile text independent;
                                             enforcement difference unmeasured)
NETWORK_DENY                           = PARTIAL (ENV_BLOCKED)
CONTROL_PLANE_OUTSIDE_SANDBOX          = YES  (structural, host-independent)

TESTS                = 12 passed; 7 host-independent real assertions;
                       5 enforcement tests ENV_BLOCKED on this host
PRODUCTION_FILES_CHANGED = 1  (crates/webcodex-process/src/lib.rs — module export)
RESEARCH_FILES_CHANGED  = 3  (this file, SPAWN_INTEGRATION_NOTES.md,
                              CODEX_SANDBOX_REUSE_SPIKE.md)
NO_PRODUCTION_BEHAVIOUR_CHANGE = YES
```

Two questions answered, one of them with a caveat that changes the plan. The
spike's most valuable output is not the confirmation that per-action sandboxing
is feasible — that was the expected answer — but the measurement that
`ManagedChild` is a *partial* chokepoint and 18 production spawn sites bypass
it, one of which (`git_apply`) is model-reachable and writes to the workspace.
A normalisation plan built on the uncorrected count would have left it
unrouted.
