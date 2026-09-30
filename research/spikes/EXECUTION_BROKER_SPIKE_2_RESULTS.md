# EXECUTION_BROKER_SPIKE_2_RESULTS.md

Round 2 of the execution-broker spike. Three blockers, no normalisation.

```
SPIKE_2_STATUS = PARTIAL

BASE_SHA = 243f639cf015fc5c766bb55f07f5238d8ceab1c0
BRANCH   = spike/webcodex-execution-broker-fidelity
HOST     = macOS 26.2 (Darwin 25.2.0), aarch64
TOOLCHAIN= cargo/rustc 1.95.0 (primary), 1.98.1 (installed for B1b)
```

`PARTIAL`: blocker A (command fidelity) is resolved and proven. Blocker B
(B1 retry) is resolved — with a *different and worse* blocker found. Blocker C
(host-native Seatbelt) is **not** resolved, because the independent-host half of
it cannot be performed from inside this environment, and that is reported as
unresolved rather than assumed.

---

## 1. Blocker A — command execution fidelity

### What was wrong

Round 1's broker took `&mut Command` and copied three fields out of it:

```rust
let program = command.get_program();
launcher.arg("-p").arg(&sbpl).arg(program);
launcher.args(command.get_args());
launcher.current_dir(cwd);
```

`stdin`, `stdout`, `stderr` and the whole environment were dropped. A caller
who wrote `cmd.stdout(Stdio::piped())` got a child whose stdout was **not**
piped, and nothing reported an error. The round-1 test suite did not catch it
because its assertions never depended on a pipe reaching the child.

### The fix: `SpawnSpec`, broker owns construction

`ExecutionBroker::spawn(&mut Command, cwd, plan)` is **removed**. It is replaced
by `ExecutionBroker::spawn(&SpawnSpec)`.

`SpawnSpec` states the execution completely, as a value: `program`, `args`,
`cwd`, `env: EnvPolicy`, `env_vars`, `env_remove`, `stdin`/`stdout`/`stderr` as
`StreamPolicy`, and `plan`. The broker builds the final `Command`.

Patching the old signature by reading more fields off `Command` was rejected on
principle, not taste: **`Command` has no API to enumerate its own state**, so
every field added to a copy-list is a field that can be forgotten. Round 1 is
the proof — it forgot four.

Two supporting changes fell out of the ownership inversion:

- `EnvPolicy { Minimal, Inherit, Empty }`, defaulting to `Minimal`. Under
  `Minimal` the environment is cleared, then only `PATH` and `HOME` (pointing
  at the action's own cwd) are set. A sandboxed child must not inherit the
  runner's credentials because the caller forgot to mention them.
- `ExecutionBroker::build_command(&spec)` is public, so what *would* be executed
  is inspectable without spawning. That is the seam a reviewer or a test needs.

### Results

```
COMMAND_FIDELITY_RESULT = PASS

F1_STDOUT               = PASS   pipe requested -> bytes returned to caller
F2_STDERR               = PASS   stderr separate, does not leak into stdout
F3_STDIN                = PASS   bytes written to child stdin come back on stdout
F4_CWD                  = PASS   child's pwd == spec's cwd (incl. /var canonicalisation)
F5_ENV                  = PASS   explicitly requested variable reaches child
F6_SECRET_ENV_ISOLATION = PASS   runner's env var absent from child under default
F6b_INHERIT_OPT_IN      = PASS   EnvPolicy::Inherit does carry it (proves F6 is not vacuous)
F7_EXIT_STATUS          = PASS   exit 42 arrives as 42 through the launcher
F8_PROCESS_LIFECYCLE    = PASS   pid exposed, try_wait None while running,
                                 terminate_tree actually reaps the tree
```

All eight run for real. None is `ENV_BLOCKED`, and none can be: they execute
under a bare `(allow default)` profile, because what they assert is the
broker's command construction and process handling rather than profile
enforcement. The child really is launched through `/usr/bin/sandbox-exec` with
the launcher as its parent, so the tree is real — only the restrictions are
absent.

`f6b` exists because `f6` alone could pass for the wrong reason. If `Inherit`
also scrubbed everything, `f6` would prove nothing.

F6 uses `std::env::set_var`, which is `unsafe` in edition 2024 and a data race
in multithreaded tests. The tests here are single-threaded per process and the
variable name is unique to each test, but this is a known sharp edge and a
production version of the suite should construct the inherited environment
explicitly instead of mutating the process's.

### Making the unconfined test path unreachable from production

`SandboxPlan` became an enum:

```rust
pub enum SandboxPlan {
    Confined { writable_roots, readable_roots, network },
    UnconfinedForFidelityTesting,
}
```

`ExecutionBroker::spawn` **refuses** the `Unconfined` variant. Only
`spawn_unconfined_for_fidelity_testing` honours it. So no production caller can
obtain an unrestricted child by constructing a plan, while the fidelity tests
still exercise the real launcher. The variant name is intentionally unpleasant
so that reaching for it in production code looks like what it is.

```
cargo test -p webcodex-process --test execution_broker
  20 passed; 0 failed
  = 8 fidelity (F1-F8) + F6b + 5 construction/refusal
  + 2 structural + 5 round-1 enforcement (ENV_BLOCKED)
```

---

## 2. Blocker B — B1 retried properly

Round 1 concluded B1 "structurally blocked". The instruction was that this was
too strong, because Codex's root workspace carries a `[patch.crates-io]` table
that an external consumer does not inherit. Both variants were built and run.

### B1a — zero-config (round-1 configuration, re-confirmed)

```
B1A_ZERO_CONFIG = FAIL (reproduced)

error: failed to select a version for `tokio-tungstenite`.
    ... required by package `codex-otel v0.0.0 (codex?rev=69f7140...)`
    ... which satisfies git dependency `codex-otel` of package `codex-sandboxing`
versions that meet the requirements `^0.28.0` are: 0.28.0
package `codex-otel` depends on `tokio-tungstenite` with feature `proxy` but
`tokio-tungstenite` does not have that feature.
```

The `proxy` feature exists only in OpenAI's fork. Confirmed by reading the
pinned revision's root manifest:

```toml
# codex-rs/Cargo.toml @ 69f7140
[patch.crates-io]
crossterm        = { git = "https://github.com/openai-oss-forks/crossterm",        rev = "efa1778..." }
tokio-tungstenite = { git = "https://github.com/openai-oss-forks/tokio-tungstenite", rev = "0e5b2d7..." }
tungstenite      = { git = "https://github.com/openai-oss-forks/tungstenite-rs",     rev = "4fffad3..." }
```

### B1b — with the Codex root patches replicated

Those three entries were copied verbatim into the consumer's root
`Cargo.toml`. **The original blocker was cleared** — `tokio-tungstenite`
resolved from the fork, and 671 packages locked. A new blocker appeared:

```
B1B_WITH_CODEX_PATCHES = FAIL (new blocker, downstream of the one it fixed)

error: rustc 1.95.0 is not supported by the following packages:
  rama-error@0.3.0 requires rustc 1.96.0
  rama-macros@0.3.0 requires rustc 1.96.0
  rama-utils@0.3.0 requires rustc 1.96.0
```

`rama` is not a Codex-authored crate; it comes from crates.io via
`codex-network-proxy`, which pins it exactly (`=0.3.0-alpha.4`). This is a
toolchain-floor problem, not a resolution problem, so it was worth one more
step: rustc 1.98.1 was installed and B1b retried.

```
error[E0432]: unresolved import `rama_error::OpaqueError`
  --> rama-core-0.3.0-alpha.4/src/stream/json/stream/read.rs:5:47
error[E0432]: unresolved import `rama_error::OpaqueError`
  --> rama-core-0.3.0-alpha.4/src/username/parse.rs:2:30
error: could not compile `rama-core` (lib) due to 3 previous errors
```

**`rama-core 0.3.0-alpha.4` does not compile against its own published
`rama-error 0.3.0-alpha.4`.** It references a type that does not exist in the
sibling crate it is pinned to. This is an upstream packaging defect in a
pre-release, reachable only because Codex pins the version with `=`.

| Measurement | Value |
|---|---|
| `COMPILE` | FAIL at `rama-core 0.3.0-alpha.4` |
| `DIRECT_DEP_COUNT` | 1 (`codex-sandboxing`) |
| `TRANSITIVE_SIZE` | 671 packages locked — for a macOS seatbelt profile builder |
| `BUILD_TIME` | 46.9s to reach the rama failure, from cold |
| `API_COUPLING` | unchanged from round 1: the entry point takes `FileSystemSandbeltPolicy` / `MacosSeatbeltProfile` / `AbsolutePathBuf` |
| `REQUIRED_PATCHES` | 3 (`crossterm`, `tokio-tungstenite`, `tungstenite`) — all OpenAI forks |
| `WEB_CODEX_CARGO_DELTA` | would add tokio + 671 transitive crates to `webcodex-process` |

The round-1 conclusion is **confirmed but for a different reason, and it is
worse than round 1 said.** Round 1 said "blocked by `workspace = true`". The
truth is: unblocking that requires replicating three upstream fork patches,
satisfying a raised MSRV, and then compiling a broken pre-release that no
external consumer can build. The first two are costs; the third is a wall.

### Re-judging B1 vs B3

The instruction was not to defend B3 on the strength of round 1. Re-judged on
this round's evidence:

**B1 remains rejected, and the case is stronger.** Even if `rama-core` were
repaired, B1 would still drag `codex-otel`, `codex-network-proxy` and 671
packages into a crate that only needs to render a profile string, and it would
still carry the round-1 API coupling. The decision does not rest on B1's
blocker being unfixable — it rests on 671 transitive crates and an API that
consumes Codex's policy model, both of which survive any patch.

---

## 3. Backend status correction

Round 1 described `/usr/bin/sandbox-exec` and SBPL as a "documented macOS
interface". **That was wrong and is corrected here.** It is not a supported
third-party API:

```
DEPRECATED                             = YES
UNSUPPORTED_FOR_THIRD_PARTY_CUSTOM_POLICY = YES
USED_BY_CODEX_AS_PRAGMATIC_BACKEND     = YES
REPLACEMENT_AVAILABLE                  = NO
```

Apple has deprecated the Seatbelt profile language, documents no support for
third-party policy use of it, and provides no replacement for restricting a
child process. Codex uses it anyway, as a pragmatic backend — which is a
statement about Codex's constraint set, not an endorsement of the interface.

This is recorded as data in `BackendStatus` in the module, with a `summary()`
method, so a future backend swap is a visible diff and no downstream reader can
mistake "works on our host" for "supported".

Per the instruction, deprecation does not by itself disqualify the backend. It
is entered as **maintenance risk**: an interface Apple has deprecated, with no
replacement, on which a security boundary would depend.

```
B3_STATUS = STILL_VIABLE / STILL_SELECTED (provisional, pending blocker C)
```

Not written as `FINAL`.

---

## 4. Blocker C — host-native Seatbelt control

### Measured in the WorkBuddy environment

| Probe | Profile | rc | Meaning |
|---|---|---|---|
| P0 | `(version 1)(allow default)` | **0** | profile applied |
| P1 | `(version 1)(allow default)(deny network*)` | **71** | `sandbox_apply: Operation not permitted` |
| P2 | `(version 1)(allow default)(deny file-read*)` | **71** | same |
| P3 | `(version 1)(allow default)(deny file-write*)` | **71** | same |
| P4 | `(version 1)(allow file-read*)` | **71** | same |
| P5 | `(version 2)(allow default)(deny file-read*)` | **71** | same |

Every narrowing rule is refused; only the permissive profile applies. Note that
P1 is a **network** restriction, not a file one — so the refusal is not
something about filesystem allow-lists specifically.

Two explanations fit equally well:

- **(a)** this host kernel refuses restrictive Seatbelt profiles; or
- **(b)** this process already runs inside a sandbox, and a sandboxed process
  cannot impose further restrictions on a child.

They have opposite consequences. (a) means per-action sandboxing cannot be
verified on any Mac and the backend must change. (b) means it is only
unverifiable from inside this tool.

### The independent-host half could not be performed

```
HOST_NATIVE_CONTEXT = NOT_OBTAINED
```

Attempted and failed:

```
$ osascript -e 'tell application "Terminal" to do script "echo LAUNCHED"'
execution error: "Terminal"遇到一个错误：发生权限违例。 (-10004)
```

TCC denies this agent automation of Terminal. `launchctl managername` reports
`Aqua`, so a GUI session exists — the agent simply may not drive it.

Running another shell *inside* WorkBuddy would not have been an independent host
environment, so it was not attempted as a substitute. A second, weaker signal
did appear: `ps` is itself blocked in this environment
(`PermissionError: [Errno 1] Operation not permitted`), so the probe's ancestor
chain came back empty. That is consistent with (b) and is not proof of it.

```
USER_NATIVE_PROBE_REQUIRED = YES
```

A script was generated and is syntax-checked and executable:

```
research/spikes/native-seatbelt-probe.sh
```

**The single command for the user to run, from a Terminal window they opened
themselves:**

```bash
bash /Users/songshiyao/Desktop/Projects/webcodex/research/spikes/native-seatbelt-probe.sh
```

It runs P0–P5, prints the session context and the ancestor chain, and explains
how to read the result. It is read-only — three profiles around `/usr/bin/true`,
no system state changed. Its output as run from inside the agent is the table
above; the independent-host run is the missing half.

```
NESTED_SANDBOX_HYPOTHESIS = UNRESOLVED

  Discriminating evidence so far:
    consistent with (b)  - ps is blocked in this environment
    consistent with (a)  - nothing observed yet that excludes it
  Decided by: P1/P2 return rc=0 natively, rc=71 in here  -> SUPPORTED
                 P1/P2 return rc=71 natively too         -> REJECTED
```

---

## 5. Verdict

```
SELECTED_MACOS_BACKEND = B3_SUBPROCESS_ADAPTER (provisional)

READY_FOR_NORMALIZATION = NO

BLOCKERS =
  1. HOST_NATIVE_SEATBELT_UNVERIFIED
     Restrictive-profile enforcement is unmeasured on any host. Per-action
     sandboxing is currently a design with no passing enforcement test, and
     normalisation would route 20 model-reachable spawn sites behind a
     mechanism whose enforcement nobody has observed working. That is the one
     blocker that matters.
  2. RUSTC_FLOOR_FOR_CODEX_SANDBOXING
     B1b needs rustc >= 1.96 for `rama`, and then fails on a broken
     `rama-core 0.3.0-alpha.4`. Recorded as measured; not a blocker for B3,
     which adds no dependencies and builds on 1.95.
  3. NO_SUPPORTED_BACKEND
     The macOS backend is deprecated and unsupported for third-party policy,
     with no replacement. Maintenance risk, accepted knowingly rather than
     overlooked.
```

### Why `READY_FOR_NORMALIZATION = NO`, stated plainly

The API is now sound — blocker A is genuinely fixed and F1–F8 prove it. But
normalisation means routing `run_shell`, `git_apply`, `ssh` and 18 other
model-reachable spawn sites through this broker, and doing so would put a
security boundary in front of all of them while the boundary's enforcement
remains **unmeasured on every host available to this work**.

Routing first and verifying later inverts the risk. If enforcement turns out
not to work on a normal Terminal either, the correct response is to change the
backend, and 20 rewired call sites is the wrong thing to have changed at that
point. Verification first costs one script run.

Next step is therefore blocker C, not normalisation.

### What would flip it to YES

1. `native-seatbelt-probe.sh` run from a normal Terminal, P1/P2 returning
   rc=0. That establishes enforcement is real and measurable on this Mac, and
   the round-1 A–E tests become runnable as-is.
2. A–E then pass for real.
3. Only then route the `ManagedChild` family (15 sites via `SpawnOptions`),
   then the direct-spawn sites.

If P1/P2 return rc=71 natively, the conclusion is that macOS Seatbelt cannot
be verified on this machine at all, and the backend decision reopens — with
Linux/bwrap the obvious alternative, since its primitives are scriptable and
testable in CI.

---

## 6. State of this branch

```
PRODUCTION_FILES_CHANGED = 1
  crates/webcodex-process/src/execution_broker/mod.rs   (rewritten: SpawnSpec API)
  crates/webcodex-process/tests/execution_broker.rs    (rewritten: F1-F8)
  crates/webcodex-process/src/lib.rs                   (unchanged this round)

Production call sites routed = 0  (no normalisation, per instruction)
Forbidden surfaces touched  = none of shell.rs / job_manager.rs /
                              workspace_checkpoint.rs / git_apply / ssh /
                              remote_shell / persistent_shell / coding_agent /
                              plugin / LSP
No SecurityBroker, no approval, no HMAC, no socket, no policy engine, no proxy.
```

`cargo fmt --check` clean, `cargo check -p webcodex-process` clean, 20/20 tests
pass, `webcodex-process`'s pre-existing 3 passed / 15 ignored unchanged.
