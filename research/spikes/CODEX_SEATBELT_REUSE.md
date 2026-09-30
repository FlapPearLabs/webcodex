# Codex Seatbelt Reuse — Provenance

Scope: what WebCodex took from `openai/codex`, what it changed, and why the
boundary is where it is.

## Upstream

| Field | Value |
|---|---|
| `UPSTREAM` | `openai/codex` |
| `UPSTREAM_SHA` | `69f7140559180269e2eb8f5be6e0c20eb37b0c85` |
| `SOURCE_FILES` | `codex-rs/sandboxing/src/seatbelt_base_policy.sbpl`<br>`codex-rs/sandboxing/src/seatbelt_read_only_platform_defaults.sbpl` |
| `LICENSE` | Apache-2.0 (`openai/codex` `LICENSE`) |
| `REVIEWED_AS` | read-only reference; the checkout is not vendored, not depended on |

## Reuse

| File | Local path | Kind |
|---|---|---|
| `seatbelt_base_policy.sbpl` | `crates/webcodex-process/src/execution_broker/sbpl/codex_base_policy.sbpl` | **DIRECT COPY** + 2 local changes |
| `seatbelt_read_only_platform_defaults.sbpl` | `crates/webcodex-process/src/execution_broker/sbpl/codex_read_only_platform_defaults.sbpl` | **DIRECT COPY**, header only |

`CODEX_BASE_POLICY_REUSE = DIRECT`

### Concept reused, not just text

The mechanism worth naming, because it is the security-relevant part:

**Filesystem roots travel to the kernel as `sandbox-exec` argv parameters, not
as profile text.** The profile contains only `(subpath (param "NAME"))`; the
path itself is passed as `-D NAME=<path>`. Upstream does this
(`READABLE_ROOT_<i>` / `WRITABLE_ROOT_<i>`).

Consequence: there is no SBPL parser between a root and the kernel, so a root
cannot inject policy syntax, cannot break out of a string literal, and cannot
silently widen the profile. A compiler bug fails in one of two ways — the
`-D` is missing, or the `(param)` is undefined — and both are checked before
any process exists.

Measured on the spike host:

```text
profile referencing an undefined (param)  -> rc=65  (compile error, fail-closed)
profile referencing a defined (param)      -> rc=71  (sandbox_apply refused here)
```

`rc=65` is the important one: a mismatch between emitted rules and passed
parameters does **not** degrade into a permissive profile.

Also reused as design input, not copied: the writable-root anchor deny
(`deny file-write-unlink` on the root's own vnode) that stops a task renaming
its authority away and recreating it.

## Local changes

### `codex_base_policy.sbpl`

1. **Provenance header.** SPDX identifier, upstream project, commit, source
   path, license, and an enumeration of every local change.
2. **Explicit `(deny network*)`.** Upstream states network as an *allow-list*
   appended to a deny-default profile, so "deny" needs no rule. WebCodex keeps
   deny-default and denies explicitly. `NetworkPolicy::Allow` is refused by the
   compiler, so this is unconditional.
3. **Toolchain read access — emitted by the compiler, not written here.** See
   below.

### `codex_read_only_platform_defaults.sbpl`

Provenance header only. No rule added, removed, or reordered.

### `execution_broker/compiler.rs` (new, all WebCodex)

The thin parameter layer. It does three things and nothing else:

- validates and canonicalizes every root (absolute, resolvable, outside `$HOME`
  for toolchain roots);
- emits one `(allow file-read* …)` / `(allow file-write* …)` rule per root,
  with a matching `-D` definition;
- fails closed on an inexpressible plan (empty roots, relative root, unresolvable
  root, `NetworkPolicy::Allow`).

### Why the toolchain rules are compiler-generated

A deny-default profile must still be able to *start* a Homebrew-installed
interpreter. Upstream's platform defaults stop at fixed system prefixes
(`/usr/bin`, `/usr/lib`, `/System`, …) and do not cover `/opt/homebrew`, which
is where this machine's `node` lives.

The alternative — `allow file-read* ~` — would hand every sandboxed action the
user's entire home directory to make one binary runnable. That is not a
trade-off worth making, so the compiler instead emits exactly one
`(allow file-read* file-map-executable (subpath (param "WEB_CODEX_TOOLCHAIN_<i>")))`
per declared toolchain prefix, and the binary is the only thing that names
those prefixes.

The rules are deliberately not written into the static `.sbpl`: an undefined
`(param)` makes `sandbox-exec` reject the whole profile (rc=65), which is
fail-closed but would also break every profile that declares no toolchain.

Toolchain roots are refused if they resolve inside `$HOME`. Without that check,
"let this action run node" would be a way to say "read my home directory".

## Notice

`openai/codex` ships a `NOTICE` file:

```text
OpenAI Codex
Copyright 2025 OpenAI

This project includes code derived from Ratatui, licensed under the MIT license.
Copyright (c) 2016-2022 Florian Dehau
Copyright (c) 2023-2025 The Ratatui Developers
```

`NOTICE_ACTION = REVIEWED, NO ACTION REQUIRED FOR THE COPIED FILES`

The Ratatui attribution covers TUI code, not SBPL policy text. Both copied
files are original SBPL authored in `codex-rs/sandboxing/src/`, and the
`sbpl` files themselves trace their own design lineage to Chromium's sandbox
policies (upstream's own comment, preserved verbatim in the copied header,
links `source.chromium.org/.../sandbox/policy/mac/common.sb`). The Chromium
BSD-3-Clause notice is likewise not triggered by these files.

Apache-2.0 §4 is satisfied by the in-file provenance header retained in both
copies, plus this record. Neither file's substance was removed.

## Not adopted

Deliberately out of scope, so that reuse does not quietly become a dependency:

- `FileSystemSandboxPolicy` / the Codex protocol policy model
- `codex_network_proxy` (WebCodex refuses `NetworkPolicy::Allow` instead)
- `manager.rs`, `seatbelt_daemon.rs`, the UDS approval/protection transport
- `policy_transforms.rs`, `denial.rs`, `violation.rs`
- `bwrap.rs` / `landlock.rs` (Linux backends)
- the `codex-otel` / `codex-protocol` dependency surface — the reason
  `B1_CODEX_DIRECT_DEPENDENCY = REJECTED` in round 2

WebCodex needs Codex's Seatbelt *knowledge*, not Codex's sandbox crate.
