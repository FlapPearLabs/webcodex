# Seatbelt Profile Spike — Results

Round 3. Answers: is the production escape hatch gone, does a Codex-derived
profile compile, and does it confine.

## Verdict fields

```text
PRODUCTION_ESCAPE_HATCH = ABSENT
CODEX_BASE_POLICY_REUSE = DIRECT
PROFILE_COMPILER = PASS

T0 = PENDING_USER_RUN
T1 = PENDING_USER_RUN
T2 = PENDING_USER_RUN
T3 = PENDING_USER_RUN

WORKBUDDY_A = ENV_BLOCKED
WORKBUDDY_B = ENV_BLOCKED
WORKBUDDY_C = ENV_BLOCKED
WORKBUDDY_C2 = ENV_BLOCKED
WORKBUDDY_D = ENV_BLOCKED
WORKBUDDY_E = ENV_BLOCKED

HOST_NATIVE_A = PENDING_USER_RUN
HOST_NATIVE_B = PENDING_USER_RUN
HOST_NATIVE_C = PENDING_USER_RUN
HOST_NATIVE_C2 = PENDING_USER_RUN
HOST_NATIVE_D = PENDING_USER_RUN
HOST_NATIVE_E = PENDING_USER_RUN

NATIVE_ALL_PASS = PENDING_USER_RUN
READY_FOR_NORMALIZATION = PENDING_NATIVE_TEST
```

`NATIVE_ALL_PASS` is `PENDING_USER_RUN` because no independent-Terminal run has
happened yet. It is not `true` and not `false`.

## Corrections in this round

Three defects in the round-3 spike were found and fixed. Each would have
produced a wrong result, and two of them would have failed *toward* a false
pass or a false fail.

### Host precondition no longer requires blanket `deny file-read*`

`host_allows_restrictive_profiles()` required **two** probes to succeed, one of
them `(version 1)(allow default)(deny file-read*)`. That probe fails with rc=134
even on a host that applies restrictive profiles correctly, because a blanket
read deny cuts off the system reads `/usr/bin/true` needs in order to start at
all. The precondition therefore reported ENV_BLOCKED on capable hosts.

Now one probe: `(version 1)(allow default)(deny network*)`, which narrows
something while still letting the target run. That is the actual precondition —
`sandbox-exec` exists, and a profile can be applied. Only a failure there means
`sandbox_apply` itself failed, which is the sole legitimate ENV_BLOCKED.

This is the same fact P2 established in round 2, applied to the code rather
than only to the notes.

### B / C / C2 denial oracles corrected

These asserted that a denied read would make the child exit non-zero. That is
backwards: `cat` returns non-zero when it is *denied*, and python/node raise.
So correct enforcement produced a non-zero exit and the tests **FAILED**.

Each test program now catches its own denial and exits 0:

```text
B   if cat outside -> print B_LEAK, exit 1
    else           -> print B_DENIED, exit 0
C   python: try open(outside) -> print C_LEAK, exit 1
              except OSError -> print C_DENIED, exit 0
C2  node:   try readFileSync  -> print C2_LEAK, exit 1
              catch            -> print C2_DENIED, exit 0
```

Pass requires all of: process demonstrably started (`*_STARTED_OK` / `*_SHELL_OK`
present), `*_DENIED` present, `*_LEAK` absent, and rc == 0. A non-zero exit now
means something *other* than enforcement went wrong, which is a signal worth
having rather than an expected outcome.

### Arbitrary toolchain roots are no longer expressible

`spawn_with_toolchain(spec, &[PathBuf])` let any caller widen read access past
the plan. `spawn_with_toolchain(spec, &["/"])` would have compiled to a profile
granting `(subpath "/")` — the whole filesystem, readable — and every check
that should have caught it passed, because `/` is absolute and is not inside
`$HOME`.

The parameter is now `&[TrustedToolchainRoot]`: an opaque type with a private
field and no public constructor. The only way to mint one is
`TrustedToolchainRoot::resolve(executable)`, which canonicalizes the path,
requires a regular file, and requires it to sit inside a **recognised**
toolchain layout (`/opt/homebrew`, `/usr/local`, `/opt/local`, `/sw`, nix, or a
system prefix).

`/` is therefore not a value that fails a range check — it is a value the
resolver will not hand out, because it is not a recognised prefix of any
executable.

Evidence, strongest channel first:

| Channel | Result |
|---|---|
| Downstream caller passes `&[PathBuf::from("/")]` | **E0308 mismatched types** — does not compile |
| Downstream caller writes `TrustedToolchainRoot(PathBuf::from("/"))` | **E0423 private fields** — does not compile |
| `resolve("/")`, `resolve("/usr")`, `resolve("/opt")`, `resolve("/System")`, `resolve("/private")`, `resolve("/Users")` | all rejected |
| `resolve()` on a directory inside a recognised prefix | rejected (not a regular file) |
| `resolve()` on a stray temp file | rejected (outside every recognised prefix) |
| `resolve("/usr/bin/true")` | accepted, yields `/usr/bin` — a bounded prefix |

The two compile errors are the real proof: a downstream crate cannot express
the grant at all, so no amount of runtime carelessness can produce one.

`arbitrary_root_cannot_become_a_toolchain_grant` in the compiler unit tests
also pins the *old* behaviour as a precondition, so the new guarantee is
recorded as a difference rather than an assertion.

## Task A — production escape hatch: ABSENT

`SandboxPlan::UnconfinedForFidelityTesting` and
`ExecutionBroker::spawn_unconfined_for_fidelity_testing` are **deleted from the
type system**, not merely refused at runtime. The permissive profile now lives
in `#[cfg(test)] pub(crate) mod testing`, and `build_command` is
`#[cfg(test)] pub(crate)`.

Evidence, three independent channels:

**1. Source scan** — `production_api_has_no_unrestricted_execution_escape_hatch`
walks `crates/webcodex-process/src/**/*.rs` with comments *and string literals*
stripped, and requires every occurrence of an escape-hatch identifier to sit
after a `#[cfg(test)]` gate. String literals are stripped because a string is
not a symbol: without that, the token list would match its own declaration and
every future diagnostic mentioning these names.

**2. Release build** — `cargo build -p webcodex-process --release` compiles
clean, with no `build_command` dead-code warning, which is only possible if the
method is gated out.

**3. Release artifact** — inspecting `libwebcodex_process.rlib`:

```text
nm -gU (exported symbols)   : 0 matches for unconfined/permissive/bypass
code object (.rcgu.o)       : 0 matches for all four patterns
lib.rmeta (debug metadata)  : 1 match each  <- names, not code
```

The rmeta hits are the point worth stating plainly: the *names* still exist in
compiler metadata because the test module is in the same crate, but **no
executable code and no exported symbol corresponds to them**. There is nothing
to call and nothing to link against.

`CODEX_TEST_PERMISSIVE_PROFILE` is `#[cfg(test)]` and appears in zero release
object bytes.

## Task B — Codex-informed baseline: DIRECT reuse

Both `.sbpl` files are direct copies from `openai/codex` @
`69f7140559180269e2eb8f5be6e0c20eb37b0c85` (Apache-2.0), with provenance headers
retained in-file. Full record in `CODEX_SEATBELT_REUSE.md`.

Local changes: the provenance header; an explicit `(deny network*)` (upstream
states network as an allow-list over deny-default, WebCodex denies); and
compiler-emitted toolchain rules.

The reused *mechanism* is the one that matters: roots reach the kernel as
`sandbox-exec -D` argv parameters, and the profile only ever contains
`(subpath (param "NAME"))`. No path is ever interpolated into SBPL text.

## Task C — profile compiler: PASS

`execution_broker/compiler.rs`, 10 unit tests, all passing.

Fail-closed behaviours, each with a test:

| Condition | Result |
|---|---|
| relative root | `RootNotAbsolute` |
| non-existent / unresolvable root | `RootUnresolvable` |
| plan with no roots | `NoFilesystemAccess` |
| `NetworkPolicy::Allow` | `Unsupported` |
| toolchain path not a recognised prefix / not a file | `ToolchainRootRejected` |
| toolchain prefix inside `$HOME` | `ToolchainRootRejected` |

Roots are canonicalized. This matters on macOS specifically: `/tmp` is a symlink
to `/private/tmp`, so `(subpath "/tmp/x")` and `(subpath "/private/tmp/x")` are
different rules. A non-canonical root would produce a rule matching nothing —
a silent over-restriction.

**Measured fail-closed property:** a profile referencing an undefined `(param)`
is rejected by `sandbox-exec` with rc=65 *before* `sandbox_apply`. So a bug
where emitted rules and passed parameters disagree cannot produce a permissive
profile; it produces a refusal.

## Native evidence inherited from round 2

The user's own Terminal.app run established, and this round does not re-argue:

```text
P0 (allow default)                              rc=0
P1 (allow default)(deny network*)               rc=0
P2 (allow default)(deny file-read*)             rc=134
P3 (allow default)(deny file-write*)            rc=0
P4 (allow file-read*)                           rc=71  execvp failed
P5 (version 2)(allow default)(deny file-read*)  rc=134
```

`HOST_NATIVE_SEATBELT = WORKING`. `NESTED_SANDBOX_HYPOTHESIS = SUPPORTED` —
P1 and P3 prove narrowing applies natively, while the same profiles fail
`sandbox_apply` inside WorkBuddy.

P2/P5's rc=134 is not a sandbox failure: a blanket `deny file-read*` cuts off
the system reads the program needs to start. P4 shows a closed-default profile
without `process-exec` cannot even exec. This round's profile shape exists
because of those two results — deny-default plus explicit minimum allowances,
not deny-default alone.

## Fidelity: F1–F8 still pass

`cargo test -p webcodex-process --lib` → **27 passed, 0 failed**.

F1 stdout pipe · F2 stderr separate · F3 stdin pipe · F4 cwd · F5 explicit env ·
F6 runner secret not inherited · F6b `Inherit` is opt-in · F7 exit status ·
F8 `ManagedChild` lifecycle.

Two round-2 tests were rewritten rather than ported, because their old
assertions encoded the old design:

- `two_plans_are_independent_at_profile_level` asserted the two profiles'
  *text* differed. With roots as argv the text is now **identical by
  construction**, and authority differs only in the definitions. The test now
  asserts exactly that, and says why.
- `quotes_in_paths_are_escaped_not_injected` was replaced by
  `roots_travel_as_argv_never_as_profile_text`, which creates a directory named
  `we"ird) (allow default) (` and requires that no part of it reaches the
  profile. Stronger than the old escaping test: there is no escaping step to
  get wrong.

F1–F8 now live in `#[cfg(test)]` unit tests. An integration test links the
library without `cfg(test)` and therefore *cannot* see the permissive helper —
which is the escape hatch's absence demonstrated a second way.

## WorkBuddy-side A–E: ENV_BLOCKED

`cargo test -p webcodex-process --test execution_broker` → 6 passed, and every
one printed `ENV_BLOCKED` before returning. **These are not passes.** The
`test result: ok` line is an artifact of an early `return`, not evidence of
confinement. Each test states this in its own output.

The tests were restructured so that when they *do* run they cannot produce a
false pass:

- **B** requires the child to print a marker proving it started, then asserts
  the denied data is absent. A `SIGABRT` or rc=71 fails the test instead of
  counting as enforcement.
- **C / C2** require the outer shell's marker before accepting the descendant's
  denial.
- **E** binds the listener in the test process (outside the sandbox) and asserts
  the connection failed with an OS error, not a silent hang.
- **D** runs the same `cat` under two plans and requires deny/allow.

Each test builds its own `TEMP_ROOT` with `workspace/` and `outside/`. Nothing
depends on a fixed `/tmp/webcodex-sandbox-spike/outside.txt` left by an earlier
run.

## The remaining step, and why it is not optional

```bash
bash /Users/songshiyao/Desktop/Projects/webcodex/research/spikes/native-seatbelt-ae.sh
```

Run from Terminal.app. The script refuses to continue if the host cannot apply
a restrictive profile (exit 3), builds `seatbelt-ae-probe`, runs T0–T3 and A–E
through the **production** broker and compiler, and prints
`NATIVE_T0=… NATIVE_ALL_PASS=…`.

There is no permissive path in that binary. If it reports a pass, the profile
that produced it is the profile WebCodex would ship.

T0–T3 exist because of P2/P5: they check that the base policy does not cut off
the program it is supposed to run. T2 and T3 report `SKIP_REASON` if `python3`
or `node` is absent, and toolchain roots are resolved from `PATH` and passed
through the compiler's home-directory check.

## Not done, deliberately

- `PRODUCTION_CALL_SITES_ROUTED = 0`. No real spawn surface is touched:
  `shell.rs`, `job_manager.rs`, `workspace_checkpoint.rs`, `project_context.rs`,
  `ssh.rs`, `remote_shell`, `persistent-shell`, `coding_agent`, `plugin`, LSP,
  and browser are all untouched.
- No `SecurityBroker`, no ALLOW/ASK/DENY engine, no approval, no Unix socket,
  no HMAC, no session grant, no danger mode, no network proxy, no Linux
  backend.
- No spawn normalization. This round makes the sandbox *correct*; it does not
  make it *used*.

`READY_FOR_NORMALIZATION = PENDING_NATIVE_TEST` — and it should stay there until
a real Terminal run says the profile confines.
