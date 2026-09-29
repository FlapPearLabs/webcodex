# CODEX_SANDBOX_REUSE_SPIKE.md

Stage B: measure all three Codex-sandbox reuse routes against a pinned Codex
revision, then pick one. No route is chosen by argument; each was compiled or
tried.

```
Codex revision: openai/codex @ 69f7140559180269e2eb8f5be6e0c20eb37b0c85
                (the revision already pinned by OSS_RESEARCH_EVIDENCE.md)
Host:          macOS 26.2 (25C56), aarch64
Toolchain:     cargo 1.95.0 / rustc 1.95.0
```

---

## B1 — PINNED GIT DEPENDENCY

### What was tried

```toml
[dependencies]
codex-sandboxing = { git = "https://github.com/openai/codex",
                     rev = "69f7140559180269e2eb8f5be6e0c20eb37b0c85",
                     package = "codex-sandboxing" }
```

The first attempt guessed the crate name `codex-sandbox` and failed with
`no matching package named codex-sandbox found`. The real crate is
`codex-sandboxing`, at `codex-rs/sandboxing/`. The second attempt used the
correct name and got further, resolving the whole Codex git tree plus a second
git dependency (`github.com/microsoft/mxc`) before failing.

### Result

```
COMPILE             = FAIL
DEPENDENCY_COUNT    = 15 direct crates + tokio (transitively: mxc, otel,
                      network-proxy, protocol, uds, 3x utils-*, windows-sandbox,
                      dunce, regex-lite, tracing, url, which, anyhow,
                      serde_json, libc)
BLOCKER             = version resolution inside Codex's own workspace
APPROX_INTEGRATION_LOC = n/a (never compiled)
```

Verbatim blocker:

```
error: failed to select a version for `tokio-tungstenite`.
    ... required by package `codex-otel v0.0.0 (codex?rev=69f7140...)`
    ... which satisfies git dependency `codex-otel` of package `codex-sandboxing`
versions that meet the requirements `^0.28.0` are: 0.28.0
package `codex-otel` depends on `tokio-tungstenite` with feature `proxy` but
`tokio-tungstenite` does not have that feature.
```

### Why this is not fixable from outside

Every dependency in `codex-rs/sandboxing/Cargo.toml` is declared
`{ workspace = true }`. That syntax only resolves inside Codex's workspace,
where the root `Cargo.toml` supplies a `[patch]` table. An external crate
consuming `codex-sandboxing` gets the *published* resolution graph instead, in
which the `proxy` feature does not exist.

**So the blocker is structural, not a version pin that could be adjusted.**
Making this work would mean either vendoring Codex's workspace root or
forking its patch table — at which point B1 has become B2 with extra steps and
a much larger surface.

`codex-sandboxing` also carries Windows-only crates
(`codex-windows-sandbox`, `codex-otel` on the windows target) that a macOS-only
spike has no use for, and `tokio` — an async runtime for a synchronous
`spawn` wrapper.

---

## B2 — BOUNDED VENDOR / PORT

### What was measured

Codex `sandboxing` crate, at the pinned revision:

```
Total src (including tests): 10,716 LOC across 25 files
Non-test src:                  seatbelt.rs  1125
                               manager.rs    816
                               policy_transforms.rs 670
                               windows.rs    401
                               violation.rs  300
                               bwrap.rs      195
                               spawn.rs      142
                               landlock.rs   115
                               terminal_queries.rs 104
                               lib.rs        100
                               denial.rs      72
                               seatbelt_scratch.rs 69
                               + 3 small files
```

The reusable core for a macOS minimum is
`create_seatbelt_command_args_with_profile` (`seatbelt.rs:882`), plus
`MACOS_PATH_TO_SEATBELT_EXECUTABLE` (`seatbelt.rs:62`).

### Result

```
COMPILE                = NOT_ATTEMPTED (see below)
COPIED_OR_PORTED_LOC   = ~2,900 LOC across the 6 non-test files that make up
                         the macOS seatbelt path (seatbelt 1125 + manager 816
                         + policy_transforms 670 + spawn 142 + denial 72 +
                         seatbelt_scratch 69 = 2,894 measured); the single
                         reusable entry point is ~180 LOC but pulls in the rest
DEPENDENCIES           = 5 codex/external crates in seatbelt.rs alone:
                         codex-protocol, codex-network-proxy,
                         codex-utils-absolute-path, url, tracing
MAINTENANCE_SURFACE    = high
LICENSE_OBLIGATION     = Apache-2.0 (same as WebCodex) + NOTICE propagation;
                         Codex NOTICE credits Ratatui (MIT, Florian Dehau
                         and the Ratatui Developers)
```

**Why not attempted as a compile:** the port target is
`create_seatbelt_command_args_with_profile`, and reading it shows the cost is
not the 1,300 lines — it is the coupling. The function takes
`FileSystemSandbeltPolicy`, `MacosSeatbeltProfile`, and `AbsolutePathBuf`, and
those types carry Codex-specific semantics:

- `get_unreadable_roots_with_cwd` / `get_writable_roots_with_cwd_preserving_mutable_paths`
  — the policy model, which is the whole of P2 in the research plan
- read-only subpaths, protected ancestor computation, mutable-path preservation
- `codex_network_proxy` and `codex_protocol` types in the signature
- `seatbelt_scratch`, an allow-list of scratch directories

Porting the profile builder means porting the policy model it consumes, or
rewriting the body. Either way the "bounded vendor" becomes "reimplement
Codex's filesystem-policy semantics", which is the novel design the brief asked
to avoid. **Measuring this was the point of the exercise; compiling it would
have produced a large, high-maintenance artifact whose correctness nobody
could check.**

The honest read: Codex's seatbelt code is good *because* it encodes hard-won
policy details (protected ancestors, symlink normalisation, scratch rules).
Those details are exactly what cannot be copied shallowly and still be correct.

---

## B3 — SUBPROCESS ADAPTER

### What was built and run

A standalone prototype, zero dependencies:

```
/tmp/wc-spike/b3-adapter/src/main.rs   ~45 LOC
  -> builds an SBPL allow-list profile
  -> /usr/bin/sandbox-exec -p <profile> <program> <args>
```

Build: `Finished dev profile in 44.89s`, 1 warning (unused import), no
dependencies.

### Result

```
COMPILE           = PASS
NEW_LOC           = 45 (prototype) -> 228 (production module in webcodex-process)
DEPENDENCIES      = 0
OS_ASSUMPTIONS    = macOS, /usr/bin/sandbox-exec present, Seatbelt allow-list
                    profiles accepted by the kernel
```

### The OS assumption turned out to be false on this host

This is the important finding, and it is reported rather than worked around.

```
$ /usr/bin/sandbox-exec -p '(version 1)(allow default)'            /usr/bin/true
   -> rc 0, profile applied

$ /usr/bin/sandbox-exec -p '(version 1)(allow default)(deny file-read*)' /usr/bin/true
   -> rc 71, sandbox-exec: sandbox_apply: Operation not permitted

$ /usr/bin/sandbox-exec -p '(version 2)(allow default)(deny file-read*)' /usr/bin/true
   -> rc 71, same

$ /usr/bin/sandbox-exec -p '(version 1)(allow file-read*)' /usr/bin/true
   -> rc 71, same
```

`(allow default)` succeeds. **Every profile that narrows the default is
refused with `EPERM` at `sandbox_apply`.** Tried: v1 and v2 syntax, deny-only
and allow-only forms, literal subpath allows. All rejected. `/usr/bin/true`
was used as the target so no target-side explanation is available.

What *does* work is execution and descendant inheritance:

```
$ /usr/bin/sandbox-exec -p '(version 1)(allow default)' \
      /bin/sh -c 'echo CHILD_RAN; /bin/cat /tmp/webcodex-sandbox-spike/outside.txt'
   CHILD_RAN
   OUTSIDE_FIXTURE
```

So the mechanism (`sandbox-exec` pre-`exec`, profile inherited by descendants)
is confirmed live on this host; only allow-list enforcement is unavailable.

**This is an environment limitation, not a defect in B3.** It is also the most
important thing this stage learned: the spike's *design* is validated, its
*enforcement* is not measurable here. Any claim that per-action sandboxing
"works" would be unfounded on this host.

---

## Comparison

| Route | Compiles | Added deps | New/Vendored LOC | Codex coupling | Maintenance | License | Result |
|---|---|---|---|---|---|---|---|
| **B1** pinned git dep | **NO** | 15+ direct, tokio | 0 | total: every type is Codex-owned | n/a | Apache-2.0 | **REJECTED** — `workspace = true` deps unresolvable externally (`tokio-tungstenite` `proxy` feature) |
| **B2** bounded vendor | not attempted | 5 codex/external crates | ~2,900 measured | high: policy model + AbsolutePathBuf | high | Apache-2.0 + NOTICE (Ratatui/MIT) | **REJECTED** — "bounded" is illusory; the builder consumes the policy model |
| **B3** subprocess adapter | **YES** | **0** | **228** | none (SBPL is a public OS interface) | **low** | n/a (no code copied) | **SELECTED** |

---

## CODEX_SANDBOX_REUSE = SUBPROCESS_ADAPTER

One route is selected, not "any of them".

**Why B3 despite the enforcement limitation on this host:**

1. It is the only route that compiles. B1 is blocked structurally; B2 was not
   compiled because measurement showed the port is not bounded.
2. It copies **zero** lines from Codex, so it creates no license obligation and
   no maintenance surface tied to an upstream we do not control.
3. `/usr/bin/sandbox-exec` and SBPL are **documented macOS interfaces**, not a
   Codex invention. The reuse that matters — the platform facility — is
   already available to us directly.
4. 228 lines with zero dependencies is small enough to be reviewed as a whole,
   which is what let this spike find and fix two of its own bugs.

**What selecting B3 does not claim:** it does not claim the enforcement works
on this host. That is `PARTIAL` and is recorded as such. B3 is selected as the
*route*, on *design* grounds, with enforcement pending a host where allow-list
profiles can be applied.

**What would change this answer:** if a future Codex exposes a stable,
workspace-resolvable sandbox crate, B1 becomes preferable to a hand-written
profile builder, because the hard-won policy details in B2 would then be
available without porting them.
