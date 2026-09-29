# COMPONENT_REUSE_MATRIX.md

Reuse decisions for every capability needed to turn WebCodex into a safe,
low-friction, Codex-style local coding agent.

Baseline: `yyjeqhc/webcodex @ 7301186b98527cb4ebc0191f6033474e9abf7c20`.

Reuse vocabularies are exclusive — exactly one per row:

| Reuse type | Meaning |
|---|---|
| `DIRECT_DEPENDENCY` | Add the upstream crate/package as a versioned dependency. Requires a published artifact + stable API. |
| `VENDOR_SUBSET` | Copy a subtree of upstream source into this repo. Requires license clearances + maintenance ownership. |
| `SUBPROCESS_ADAPTER` | Invoke an upstream **binary** or an **OS facility**; write only argument/profile construction. |
| `REIMPLEMENT_SMALL_CORE` | Re-derive a small (< ~500 LOC equivalent) core from the upstream design, in Rust. |
| `DESIGN_ONLY` | Borrow only interfaces/architecture. No code, no text copied. |
| `REUSE_EXISTING` | Capability already present in WebCodex; extend it in place. |
| `NOT_NEEDED` | Requirement is already satisfied without new work. |

License summary of upstream sources (all verified from `LICENSE` files or the
GitHub license API):

| Project | URL | License | Copyright holder |
|---|---|---|---|
| WebCodex | `github.com/yyjeqhc/webcodex` | Apache-2.0 | WebCodex contributors |
| Codex | `github.com/openai/codex` | Apache-2.0 (+ `NOTICE`: Ratatui, MIT) | OpenAI |
| OpenCode | `github.com/anomalyco/opencode` (`sst/opencode` redirects) | MIT | opencode |
| Hermes | `github.com/NousResearch/hermes-agent` | MIT | Nous Research |
| Pi | `github.com/earendil-works/pi` (`badlogic/pi-mono` redirects) | MIT | Mario Zechner |

---

## 1. Master matrix

| # | Capability | Project | Exact module/file | Lang | License | Maturity evidence | Reuse type | Integration cost | Security risk | Decision |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | macOS sandbox mechanism | Apple / Codex | `sandboxing/src/seatbelt.rs:62` `MACOS_PATH_TO_SEATBELT_EXECUTABLE = "/usr/bin/sandbox-exec"` | Rust | Apache-2.0 | Shipped in codex-cli 0.136.0; 1125 LOC + dedicated tests (`seatbelt_tests.rs`, `seatbelt_fcntl_tests.rs`, `seatbelt_scratch.rs`) | `SUBPROCESS_ADAPTER` + small SBPL compiler | M | Low — OS facility; `sandbox-exec` is Apple-deprecated but functional | **Adopt** |
| 2 | macOS SBPL profile generation | Codex | `sandboxing/src/seatbelt.rs` (`create_seatbelt_command_args_with_profile`) | Rust | Apache-2.0 | Same as #1 | `REIMPLEMENT_SMALL_CORE` | S–M | Medium — a wrong SBPL rule fails open silently; needs negative tests | **Adopt, minimal subset** |
| 3 | Linux sandbox mechanism | bubblewrap | `sandboxing/src/bwrap.rs` (`find_system_bwrap_in_path`) | Rust | Apache-2.0 (Codex glue) | Codex prefers system `bwrap`; WSL1 unsupported (documented) | `SUBPROCESS_ADAPTER` | S | Low | **Adopt when `bwrap` present** |
| 4 | Linux sandbox fallback | Codex | `linux-sandbox/src/landlock.rs`, `src/main.rs` (`codex-linux-sandbox`) | Rust | Apache-2.0 | Separate shipped binary; uses `landlock`, `seccompiler`, `rustix` | `REIMPLEMENT_SMALL_CORE` | M–L | Medium — Landlock kernel-version dependent | **Adopt as fallback; else DENY** |
| 5 | Filesystem policy model | Codex | `protocol/src/models.rs:422` `PermissionProfile::Managed{file_system, network}` | Rust | Apache-2.0 | Part of the protocol crate consumed by every surface | `REIMPLEMENT_SMALL_CORE` | S | Low | **Adopt (3 profiles)** |
| 6 | Network policy axis | Codex | `protocol/src/models.rs:428` `NetworkSandboxPolicy`; `seatbelt.rs:336-346` | Rust | Apache-2.0 | Distinct SBPL rules for localhost vs DNS vs open | `DESIGN_ONLY` | S | Medium — DNS is a covert exfil channel if allowed wholesale | **Adopt design (own axis)** |
| 7 | Process-tree ownership | **WebCodex** | `crates/webcodex-process/src/unix.rs:43` `ManagedChild::spawn` | Rust | Apache-2.0 | Already in-tree; Windows Job Object equivalent in `windows.rs` | `REUSE_EXISTING` | XS | Low | **Reuse for tree ownership.** It is the *majority* sandbox attach site, **not the only one**: ≥10 direct `Command::spawn()` sites bypass it, so the attach surface is the pair {managed path, shared hook} (see `REFERENCE_ARCHITECTURE.md` §4, `OSS_RESEARCH_EVIDENCE.md` §2.1a) |
| 8 | Unified execution entry | **WebCodex** | `crates/webcodex-core/src/runner_operation.rs:632` `RunnerOperation` | Rust | Apache-2.0 | Single enum covering shell/process/script/job/file/git/agent/etc. | `REUSE_EXISTING` | XS | Low | **Reuse** |
| 9 | Policy engine: mode model | **WebCodex** | `src/tool_runtime/permissions/policy.rs:64` `EffectiveAuthorityConfig::from_env` | Rust | Apache-2.0 | Fail-closed on invalid config; projected to `runtime_status` | `REUSE_EXISTING` | S | Low | **Extend from 2 → 4 modes** |
| 10 | Policy engine: ALLOW/ASK/DENY rules | OpenCode | `packages/core/src/permission.ts:76` `evaluate`; `packages/schema/src/permission.ts` | TS | MIT | 310 LOC core + 65 LOC schema + unit tests | `REIMPLEMENT_SMALL_CORE` | S–M | Medium — last-match-wins lets a later `allow` override an earlier `deny` | **Adopt model; add deny-floor** |
| 11 | Pattern/wildcard matcher | OpenCode | `packages/core/src/util/wildcard.ts` | TS | MIT | Used by `evaluate` | `DESIGN_ONLY` | XS | **High if used for paths** — pure string glob, no canonicalization | **Policy layer only, never as path guard** |
| 12 | External-directory guard | Hermes | `tools/path_security.py:8` `validate_within_dir` | Py | MIT | Resolve-then-`relative_to`; plus `..` pre-check and control-char check | `REIMPLEMENT_SMALL_CORE` | S | Low | **Adopt (≥ 2025-09-28 Hermes)** |
| 13 | Secret-path policy | **WebCodex** | `webcodex_core::sensitive_paths::is_secret_path` | Rust | Apache-2.0 | Referenced from read/skill/diff/patch; coverage unverified in baseline, and the symbol is **not present in `OSS_RESEARCH_EVIDENCE.md §2`** — round 20 recorded this after a round-19 finding, so the row's basis is a source-level reading from an earlier pass that a reader of the submitted evidence cannot check. `REUSE_EXISTING` is provisional until the observation is added; the reuse decision is sound, the citation is not | `REUSE_EXISTING` | S | Medium — must be extended to the shell surface | **Extend** |
| 14 | Hard-deny floor | **WebCodex** | `src/tool_runtime/permissions/mod.rs:124` `is_hard_denied_output` | Rust | Apache-2.0 | Explicitly mode-independent; suppresses soft attach | `REUSE_EXISTING` | M | **Medium — currently string-matching over error prose** | **Convert to structured error kinds** |
| 15 | Hard-deny floor (reference) | Hermes | `tools/approval_floors.py:1-51` | Py | MIT | Runs before yolo/mode=off; deobfuscation variants | `DESIGN_ONLY` | XS | Low | **Adopt ordering + deobfuscation idea** |
| 16 | Approval policy model | Hermes | `hermes_cli/approval_mode.py:19` `VALID_APPROVAL_MODES = ("manual","smart","off")` | Py | MIT | Profile-scoped persistent config; managed policy blocks change | `REIMPLEMENT_SMALL_CORE` | S | Low | **Adopt (map to our 4 modes)** |
| 17 | Approval state machine + transport contract | Hermes | `hermes_cli/approval_transport.py` (185 LOC) | Py | MIT | Digest-bound request/decision; bounded workers; 6 failure codes all → deny | `REIMPLEMENT_SMALL_CORE` | M | Low | **Adopt — highest-value borrow** |
| 18 | Approval grant lifetimes | Codex | `protocol/src/protocol.rs:4159` `ReviewDecision` | Rust | Apache-2.0 | `Approved` / `ApprovedForSession` / `ApprovedExecpolicyAmendment` / `ApprovedMcpPolicyAmendment` / `NetworkPolicyAmendment` / `Denied` / `TimedOut` / `Abort`; `Default = Denied` | `DESIGN_ONLY` | S | Low | **Adopt vocabulary** |
| 19 | Session-grant storage | OpenCode | `packages/core/src/permission/saved.ts` + `permission/sql.ts` | TS | MIT | Project-keyed rows `(project_id, action, resource)`, `onConflictDoNothing` | `REIMPLEMENT_SMALL_CORE` | S | Low | **Adopt shape (project-scoped)** |
| 20 | Session-grant storage (reference) | Hermes | `tools/approval.py:249,350,366,448` `approve_session`/`is_approved`/`approve_permanent`/`save_permanent_allowlist` | Py | MIT | `(session_key, pattern_key)` pairs; in-memory session + persisted permanent | `DESIGN_ONLY` | XS | Low | **Adopt keying** |
| 21 | Auto-reviewer architecture | Codex | `core/src/guardian/decision.rs`, `ext/guardian-reviewer/`, `ext/guardian-v2/` | Rust | Apache-2.0 | Full subsystem: separate model, policy prompt, output contract, budget, deadline, circuit breaker; ~9500 LOC incl. tests | `DESIGN_ONLY` | M–L | Medium — reviewer is a second model; must never hold the floors | **Design only; do not vendor** |
| 22 | Reviewer decision contract | Codex | `core/src/guardian/decision.rs:44` (`None` ⇒ user flow; never implicit allow) | Rust | Apache-2.0 | Explicit fail-closed comment | `DESIGN_ONLY` | XS | Low | **Adopt semantics** |
| 23 | Reviewer input shape | Codex | `guardian-context/src/lib.rs` (`PlannedAction`, `action_for_review`, transcript sections) | Rust | Apache-2.0 | Deterministic context composition with token budget | `DESIGN_ONLY` | M | Medium — transcript may carry injection; reviewer must treat it as untrusted | **Adopt shape** |
| 24 | Extension / tool-interception hooks | Pi | `packages/agent/src/types.ts:66` `BeforeToolCallResult{block?,reason?,terminate?}`; `coding-agent/src/core/agent-session.ts:533` | TS | MIT | Errors convert to block; non-interactive blocks by default | `DESIGN_ONLY` | S | Low | **Adopt restrict-only semantics** |
| 25 | Plugin hook wiring (reference) | OpenCode | `packages/opencode/src/session/tools.ts` `plugin.trigger("tool.execute.before"/"after")` | TS | MIT | Hooks can mutate args before execution | `DESIGN_ONLY` | XS | Medium — mutating hooks can *widen* scope | **Adopt hooks, restrict-only** |
| 26 | Approval UI confirm with timeout | Pi | `examples/extensions/timed-confirm.ts` (`ctx.ui.confirm(..., {timeout})`) | TS | MIT | Auto-cancel on timeout; `ctx.hasUI` gates | `DESIGN_ONLY` | S | Low | **Adopt for local approval UI** |
| 27 | Decision audit records | **WebCodex** | `workflow_session_contract.rs:538` `PermissionDecision`; `permissions/mod.rs:156` `permission_summary_from_events` | Rust | Apache-2.0 | Stable wire shape; counters for auto/manual/denied/pending/hard-denied | `REUSE_EXISTING` | S | Low | **Extend with decision provenance** |
| 28 | Command exec policy (Allow/Prompt/Forbidden) | Codex | `execpolicy/src/decision.rs:9` `Decision`; `rule.rs`, `parser.rs`, `amend.rs` | Rust | Apache-2.0 | Separate crate + `execpolicycheck` binary; rules are amendable from an approval | `DESIGN_ONLY` | M | Medium — command-string rules are defeatable (see I14) | **Optional layer, never the boundary** |

---

## 2. Decision detail: can we reuse Codex's sandbox directly?

**Question:** can `Codex`'s sandbox be consumed instead of written?

**Answer: `SUBPROCESS_ADAPTER` + a small profile compiler is the recommended
route.** Of the alternatives, `DIRECT_DEPENDENCY` **via crates.io** and
`VENDOR_SUBSET` **of the whole sandbox crate** are assessed as not practical;
`DIRECT_DEPENDENCY` **via a pinned git revision** and `VENDOR_SUBSET` **of a
bounded piece** are **not assessed** and are not closed. No blanket
"not practical" verdict is claimed for the approach as a whole.

**This recommendation is provisional and the gate is named (review round 13,
CHECK 1).** The reviewer is right that recommending an option while two
alternatives remain unassessed is a preference, not a demonstrated comparison. **Round 30 withdraws the word that survived here.** Rounds 13-29 called the recommendation "defensible", and the sentence that carried the weight ("both would still have to be *built* before they reach the adapter's position") is an **estimate about work not yet scoped, not a comparison established by the submitted evidence** (round 29, CHECK 1, blocking). The honest characterisation of the grounds is now the weaker and more accurate one:

1. **The assessed options were ruled out on evidence, not ranked against the unassessed ones.** crates.io publication is confirmed absent and the one published sibling is stale — those are facts, and neither pinned-git nor bounded-vendoring is a variation on them. **What is *not* established is that the adapter beats those two.** It has not been compared with them, so it is not a demonstrated winner.
2. **The recommendation is gated on assessing the other two, not on being right.**

1. **The assessed options were ruled out on evidence, not outranked.** crates.io
   publication is confirmed absent and the one published sibling is stale — those
   are facts, and neither pinned-git nor bounded-vendoring is a variation on them.
   **The sentence that stood here read "`SUBPROCESS_ADAPTER` wins on the assessed
   set by a margin the unassessed options are unlikely to close, because both would
   still have to be *built*...", and round 31 withdraws it (round 30, CHECK 1): the
   *built* argument is an estimate about work nobody has scoped, so the margin it
   predicts is not evidence. Ruling out two options is a different achievement from
   beating the other two, and only the first is supported here.**
   **P1b** does not start profile compilation until pinned-git and bounded-vendoring
   have each been assessed against the same four criteria applied above
   (publication/API surface, staleness and update path, licence and notice
   obligation, LOC delta) — this is `IMPLEMENTATION_PLAN.md` P1b acceptance
   criterion (11). If either is assessed as equal-or-better on all four, this
   recommendation is **reopened**, not defended. (Round 14 put this gate in P0 and
   round 15 caught that the criterion it produced lives in P1b; the phase named in
   this document and the phase holding the criterion are now the same, and the gate
   is placed there because the assessment exists to inform **which profile P1b
   compiles** — running it earlier would gate work that had not been scoped yet.)

The honest characterisation is therefore **"recommended, provisional pending two open assessments that could overturn it"** — not "selected", and not, as rounds 13-29 had it, "defensible". **Defensible was a comparative claim about two options nobody had assessed, and it is withdrawn in round 30.** **Round 33 removes the remaining comparative word as well: "cheapest *assessed*" was still a cost ranking, and this matrix records no cost figures for any option, so the ranking had no basis in the document that asserts it (review round 32, CHECK 1, blocking). The recommendation now rests on the reuse-type reasoning above and nothing else.**

Evidence:

1. **Not published as a library.**
   `codex-sandboxing` does not exist on crates.io:
   `GET https://crates.io/api/v1/crates/codex-sandboxing` →
   `{"errors":[{"detail":"crate 'codex-sandboxing' does not exist"}]}`.
   Workspace members use `version.workspace = true` with path dependencies —
   an internal workspace, not a public API surface.

2. **The one published sibling is stale.**
   `codex-protocol` exists on crates.io at **0.63.0, published 2025-12-11**
   (1667 lifetime downloads), while the repository under study is at
   `69f71405` with a much later protocol. Depending on 0.63.0 would not match
   current `codex-sandboxing` sources.

3. **Heavy internal coupling.**
   `codex-sandboxing/Cargo.toml` depends on `codex-mxc-sandbox`,
   `codex-network-proxy`, `codex-protocol`, `codex-uds`,
   `codex-utils-absolute-path`, `codex-utils-path-uri`, `codex-utils-pty`,
   `codex-windows-sandbox`, plus `tokio`, `which`, `dunce`, `regex-lite`.
   `codex-protocol` itself pulls `codex-execpolicy`, `codex-extension-items`,
   and more. Vendoring the subtree means vendoring a large transitive closure.

4. **Private/internal API surface.**
   The sandbox is driven through `SandboxManager::transform` with a request
   struct carrying `permissions`, `network`, `sandbox_exe`,
   `environment_id`, `windows_sandbox_level`
   (`sandboxing/src/manager.rs:352`). These are Codex-runtime concepts, not a
   sandbox API.

5. **License is not the blocker.** Apache-2.0 permits vendoring with `NOTICE`
   preservation (`NOTICE`: "OpenAI Codex / Copyright 2025 OpenAI"). The blocker
   is engineering surface and maintenance ownership, not licensing.

6. **The mechanism is an OS facility, not Codex IP.**
   On macOS the whole mechanism is
   `/usr/bin/sandbox-exec <generated profile> <original argv>`
   (`seatbelt.rs:62`, `manager.rs:434-471`). On Linux Codex itself prefers the
   system `bubblewrap` binary (`sandboxing/src/bwrap.rs`). Therefore an adapter
   that constructs the launcher argv + a minimal profile gets the *same*
   enforcement primitive without the coupling.

7. **Size of the reuse candidate.** `seatbelt.rs` 1125 + `manager.rs` 816 +
   `violation.rs` 300 + `spawn.rs` 142 = **2,383 LOC** before transitive
   dependencies — too large for a "small core" and too coupled for a clean
   vendor.

**Conclusion (scoped per option — corrected in review rounds 1 and 2, CHECKS 1
and 10):**

An earlier revision published a blanket
`VENDOR_SUBSET_VERDICT = NOT_PRACTICAL` / `DIRECT_DEPENDENCY_VERDICT = NOT_PRACTICAL`
while simultaneously leaving the pinned-git and bounded-subset options
unassessed. That was an overreach and is withdrawn. Verdicts are now per option:

| Option | Verdict | Basis |
|---|---|---|
| `DIRECT_DEPENDENCY` via **crates.io** | `NOT_PRACTICAL` | **assessed** — `codex-sandboxing` is not published; `codex-protocol` is a stale 0.63.0 |
| `DIRECT_DEPENDENCY` via **pinned git** (`git = …, rev = …`) | `NOT_ASSESSED` | plausible, not evaluated; not required to reach a decision; must not be read as closed |
| `VENDOR_SUBSET` of the **whole sandbox crate** | `NOT_PRACTICAL` | **assessed** — 2,383 LOC + transitive closure (`codex-network-proxy`, `codex-protocol`, `codex-uds`, `codex-mxc-sandbox`, `codex-windows-sandbox`, `codex-utils-*`) |
| `VENDOR_SUBSET` of a **bounded piece** (e.g. only the SBPL builder) | `NOT_ASSESSED` | plausible, not evaluated |
| `SUBPROCESS_ADAPTER` + small profile compiler | **`RECOMMENDED`** | the mechanism is an OS facility; the adapter is where the value is |
| `NOT_PRACTICAL` (whole-approach) | **not claimed** | — |

Headline answer to the brief's "can we reuse Codex's sandbox?":

```
CODEX_SANDBOX_REUSE            = SUBPROCESS_ADAPTER   (recommended)
CODEX_SANDBOX_REUSE_CONFIDENCE = HIGH   for the negative findings on crates.io publication,
                                        coupling depth, and licence non-blockage
                                 MEDIUM for the effort estimate, and for the judgement that
                                        an adapter beats a dependency
UNASSESSED_AND_OPEN            = pinned-git dependency; bounded vendor subset of the SBPL
                                 builder; codex-process-hardening; codex-network-proxy
```

**Options that were *assessed and rejected*, versus options *not assessed*
(adversarial review round 1, CHECKS 1, 9, 10):**

| Option | Status |
|---|---|
| `DIRECT_DEPENDENCY` via crates.io | **Assessed → impractical.** `codex-sandboxing` is absent from crates.io; `codex-protocol` is a stale 0.63.0 (2025-12-11) snapshot. |
| `DIRECT_DEPENDENCY` via a **pinned git dependency** (`git = …, rev = …`) | **NOT ASSESSED.** This is technically possible and is a *different* proposition from crates.io. It was not evaluated because it was not required to reach a decision, but it must **not** be recorded as closed. If pursued, it pulls the whole transitive closure and pins the consumer to Codex's internal API stability. |
| `VENDOR_SUBSET` of `codex-sandboxing` + `seatbelt.rs` | **Assessed → impractical for the whole crate** (2,383 LOC + transitive deps: `codex-network-proxy`, `codex-protocol`, `codex-uds`, `codex-mxc-sandbox`, `codex-windows-sandbox`, `codex-utils-*`). A *bounded* subset (e.g. the SBPL string builder alone) was **NOT ASSESSED** as a separate option. |
| `codex-process-hardening` | **NOT ASSESSED.** Listed as a dependency of `linux-sandbox`; its contents were not read. It may contain hardening (rlimit/prctl/no-new-privs style) that would be cheap and useful independently of the sandbox. Flagged for P1. |
| `codex-network-proxy` | **NOT ASSESSED as a reuse candidate.** It appeared only as a coupling obstacle (and as the managed-MITM mechanism behind `enforce_managed_network`). Given §6.1(b) requires a real per-request network mechanism, this crate is now **explicitly worth reading** in P1 rather than being treated only as a dependency. |
| `codex-windows-sandbox` / `mxc-sandbox` | **Out of scope** (Windows has no FS/network confinement target in this stage). |
| `codex-execpolicy` | Design-only; see row 28. Command-rule engines are defeatable and cannot be the boundary (`SECURITY_INVARIANTS.md` I14). |

If a future Codex release publishes `codex-sandboxing` as a stable crate, or if
the pinned-git option is pursued, this decision should be re-evaluated — the
adapter boundary is designed so that swapping to a dependency later is a
one-module change.

**Re-evaluation trigger (explicit):** also re-open this section if
`codex-process-hardening` or `codex-network-proxy` turns out to supply a
directly reusable, small, well-tested component for the P1 sandbox plan or the
§6.1(b) network descriptor.

---

## 3. License audit (mandatory, per-invariant)

| Project | File/module | License | Copyright | NOTICE required | Modification allowed | Redistribution requirement | WebCodex-compatible |
|---|---|---|---|---|---|---|---|
| Codex | `sandboxing/src/seatbelt.rs` (design reference) | Apache-2.0 | OpenAI | Yes if any code copied | Yes | Retain `LICENSE` + `NOTICE`, state changes | **YES (design-only here)** |
| Codex | `protocol/src/models.rs` `PermissionProfile` (design reference) | Apache-2.0 | OpenAI | Yes if copied | Yes | Same | **YES (design-only here)** |
| Codex | `execpolicy/src/decision.rs` (design reference) | Apache-2.0 | OpenAI | Yes if copied | Yes | Same | **YES (design-only here)** |
| OpenCode | `packages/core/src/permission.ts` | MIT | opencode | Retain MIT notice if any text copied | Yes | Include MIT license text | **YES** |
| OpenCode | `packages/core/src/util/wildcard.ts` | MIT | opencode | Yes if copied | Yes | Same | **YES (design-only here)** |
| Hermes | `hermes_cli/approval_transport.py` | MIT | Nous Research | Yes if ported verbatim | Yes | Include MIT license text | **YES** |
| Hermes | `tools/path_security.py` | MIT | Nous Research | Yes if copied | Yes | Same | **YES** |
| Pi | `packages/agent/src/types.ts` (`BeforeToolCallResult`) | MIT | Mario Zechner | Yes if copied | Yes | Same | **YES (design-only here)** |
| Apple | `/usr/bin/sandbox-exec` (system binary) | OS component | Apple | N/A | N/A | N/A | **YES (invoked, not redistributed)** |
| bubblewrap | system `bwrap` binary | LGPL-2.1+ | bubblewrap contributors | N/A | N/A | N/A | **YES (invoked, not linked/redistributed)** |

**Rules applied (corrected per adversarial review round 1, CHECK 10):**

An earlier revision stated two over-broad rules: "any row that later becomes
`VENDOR_SUBSET` must add the upstream `LICENSE` text and a `NOTICE` delta", and
for `DESIGN_ONLY` rows "`notice if ported verbatim`". Both were imprecise, and
the second **understated** obligations.

Corrected obligations, which are **conditional on the actual source and the
actual distribution**:

| Situation | Obligation |
|---|---|
| Apache-2.0 source copied (Codex) | Include the Apache-2.0 text; preserve the upstream `NOTICE` (Codex's carries "OpenAI Codex / Copyright 2025 OpenAI" and a Ratatui MIT derivation); state significant modifications. Apache-2.0 §4(b)/(d) requires this for **redistribution** of the work or derivatives. |
| Apache-2.0 used only as a design reference, no code copied | Attribution is courteous but not legally required. Keep the citation for provenance; no `NOTICE` delta needed. |
| MIT source copied (OpenCode, Hermes, Pi) | Include the MIT license text **and the copyright notice** in the distribution. MIT has no `NOTICE`-file concept — saying a "NOTICE delta" is mandatory for MIT is wrong. What *is* mandatory is retaining the copyright + permission notice. |
| MIT source **adapted** (not copied literally) | The derivative is still a reproduction of protected expression unless the result is genuinely independent. Rewriting in another language from a design is the safest posture and is what this plan does; where a port is close to line-by-line, treat it as copied and carry the notice. **"Ported verbatim" was the wrong trigger** — "derived from" is the right one. |
| Invoking a system binary (`sandbox-exec`, `bwrap`) | No distribution of the binary occurs; no source obligation attaches to this repository. Note `bwrap` is LGPL-2.1+, which matters only if it were **linked or redistributed**, neither of which applies. |

Practical consequence for this plan: every row below is currently
`DESIGN_ONLY`, `REIMPLEMENT_SMALL_CORE`, `SUBPROCESS_ADAPTER`, or
`REUSE_EXISTING`, so **no third-party license text is required in this
repository today**. Any future `VENDOR_SUBSET` or close port must determine its
obligation from the table above, per source, per distribution form.

- No source is copied in this branch.
- `DESIGN_ONLY` rows copy **no** implementation text — interfaces and behavior
  only. Where a "reimplementation" is close to line-by-line, it must be
  reclassified as a port and carry the upstream notice.
- The `REIMPLEMENT_SMALL_CORE` rows (the Hermes transport contract, the OpenCode
  permission model) are language ports. They are treated as **derived from**
  MIT material: if the resulting Rust is recognisably the same structure and
  naming, the MIT copyright notice for Nous Research / opencode must be carried
  in the file header or a `NOTICE`-equivalent. Decide this at implementation
  time, per module, not by a blanket rule.

---

## 4. Minimal sufficient capability set

Derived by intersecting WebCodex's existing vocabulary, OpenCode's
`action`/`resource` model, Codex's permission axes, and Hermes' grant patterns.
Deliberately small (see axiom A4).

**Outcome vocabulary — four values, not two or three (added in review round 11;
corrected to four in round 12).** Until now every row in this table was written as
`ALLOW` / `ASK` / `DENY`, and that binary forced a false choice in the presence of
the per-action gap: either claim `DENY` for an effect nothing enforces (a security
claim), or claim `ASK` for a mediated path while implying the unmediated one is
covered (a false guarantee). Both were wrong. `ALLOW` is carried as a named row
rather than left as an unstated default, because it is a **positive** claim — the
confinement layer permits this effect on every surface that can produce it — and
needed the same "what must exist for it to be written here" condition as the other
three. A **four-valued** vocabulary is required, and the fourth value is the honest
one:

| Value | Meaning | What must exist for it to be written here |
|---|---|---|
| `ALLOW` | The effect is permitted and **nothing needs to observe it**, because the confinement profile already permits it on every surface that can produce it | A named confinement layer that permits the effect. If any surface can produce the effect outside that layer, this value may not be used |
| `ASK` | The effect is **mediated** where it goes through dispatch, and the unmediated path is separately controlled | A dispatch-time mediation point **plus** a stated outcome for the same effect inside a running child |
| `DENY` | The effect is **enforced impossible** on every surface that can produce it | An enforcement mechanism that reaches *all* those surfaces. If it does not, this value may not be used |
| `NOT_ENFORCED` | **No mechanism observes this effect on this surface.** It is a *limitation label*, not a decision | Nothing — this value exists precisely so that "we have no control here" stops being written as `DENY` |

`NOT_ENFORCED` is not a fifth risk class to be designed away in this plan; it is
the honest record of what an OS-sandbox-shaped boundary can and cannot observe. A
row may only carry `NOT_ENFORCED` if it also names the realistic consequence, so
the reader can decide whether to accept the posture.

The rule that follows, and that this review has now had to learn twice in two
directions: **writing `DENY` where nothing enforces is a security claim;
writing `ASK` where only the mediated path is covered is a false guarantee. Both
are defects. `NOT_ENFORCED` is the correct answer in both cases.**

| Capability | Existing WebCodex anchor | Enforcement layer | Default |
|---|---|---|---|
| `fs.read.workspace` | `PROJECT_READ` | OS sandbox + policy | **`ALLOW` only for paths NOT matching the enumerated deny patterns — the cell opens with the weaker of the two available scopes, because the matrix's own definition of `ALLOW` requires it** (`COMPONENT_REUSE_MATRIX.md §4` defines `ALLOW` as holding on *every* surface that can produce the effect, so a cell may not open with the unscoped label and rely on a later section to bound it). **Round 31 tightens this further: it previously read "non-secret working set", and round 30 (CHECK 8) was right that no content-classification producer exists — the plan specifies a path-pattern deny list and nothing more, so a credential embedded in a file that matches no pattern is not detected by anything.** (`COMPONENT_REUSE_MATRIX.md §4` defines `ALLOW` as holding on *every* surface that can produce the effect, so a cell may not open with the unscoped label and rely on a later section to bound it). **In-project secret patterns are `DENY`: `.env`, `*.pem`, `id_rsa`, `.npmrc`, cloud credential files, agent token stores (`IMPLEMENTATION_PLAN.md` P1b criterion (9)).** A reader taking this column alone must not conclude the workspace is uniformly readable. |
| `fs.write.workspace` | `PROJECT_WRITE` | OS sandbox + policy | ALLOW |
| `fs.read.external` | (none) | OS sandbox + per-spawn profile extension | **Scope corrected in review round 20, following a round-19 finding.** The cell previously read a flat "**DENY until P1d ships**", which is false against the profile this plan ships: `IMPLEMENTATION_PLAN.md P1b`'s `SCOPE` is "read/write inside the project root, **read system paths needed to run a toolchain**", and P1d's own `SCOPE` says "the sandbox **already needs** read access to system paths for the toolchain". So the capability's scope must exclude those paths or the stated default is not true of the system being built. Corrected form: **DENY for *external* paths — i.e. paths outside both the project root and the toolchain-read allow-list that P1b's profile already carries — until P1d ships**; paths on that allow-list are `ALLOW` by profile from P1b, and that is a deliberate, bounded exception rather than an undeclared hole. The allow-list itself must be enumerated in P1b's deliverable and negatively tested, otherwise "paths needed to run a toolchain" is not a checkable scope — see note |
| `fs.write.external` | (none) | OS sandbox | DENY |
| `secret.read` | `sensitive_paths` | OS sandbox | **`NOT_ENFORCED` (capability-wide). `DENY` holds only by path and only on the structured surface** — **the cell now OPENS with the weak claim, which is the structural change round 28 required and did not make.** Round 28 appended a correction and left the `DENY` label standing in front of it, so a reader using the Default column alone still took away the stronger, false outcome — the same strong-claim-then-correction ordering that round 28 blocked in STV §4 and P1c(10c). **In this sheet the operative cell must carry the weaker claim, because the weaker claim is the true one and the Default column is the part a reader quotes.** (Review round 28, CHECK 2, blocking.) (review round 27, CHECK 2, blocking). Round 27's point is correct: the sheet defines `DENY` as holding on every surface, so a cell that says `DENY` and then explains the exceptions is making the label false and the explanation true. The honest cell asserts the **weaker** of the two as its outcome and records the stronger as the goal. `DENY` applies to the enumerated locations and patterns *by path*, on the structured surface (floor). The capability as a whole is `NOT_ENFORCED` for (i) credentials embedded in a readable in-project file such as `.git/config`, (ii) credentials at a project location not on the pattern list, and (iii) — added in review round 14, CHECK 8 — **an enumerated secret reached by another name**: the profile denies the secret's *path*, and `SECURITY_TEST_VECTORS.md §2` records that an in-workspace hardlink to a world-readable secret inode is not re-walked after admission, so a path-name denial does not deny the content. Round 13 corrected the scope from the whole capability to the enumerated list; round 14 corrected the surface, because a list-scoped `DENY` that is silently surface-scoped is still an over-claim on the surface it does not cover. The floor is real; it is a floor over a **list**, and on the shell surface it is a floor over **path names** |
| `net.outbound` | `authority_profile_payload.shell` conflates it | OS sandbox + policy | **DENY until P1c ships.** When P1c ships it releases **only** the literal-IP / localhost-port path (criteria 1, 2, 3, 4, 5, 7, **9 and 10**); **hostname-scoped `ASK` stays `DENY` for the life of this plan** (P1c criterion 8 is unsatisfiable within it) — round 18, CHECK 5 |
| `proc.spawn` | implicit in shell/job | policy | **`ALLOW` for catalogued spawns that pass the audited API and receive a plan — and the cell cannot be stronger, because the complement is `UNKNOWN`** (P1a (1b) binds only callers that use the audited API; CP-04 records uncatalogued entries as `UNKNOWN`, and the runtime-bypass residual is phase P1a-R, `NOT_ENFORCED` since round 30). **A raw `fork`/`posix_spawn`, a plugin-crate spawn, or a re-exported alias is not in this cell's `ALLOW`** |
| `proc.signal.external` | `stop_job` confirm | policy (dispatch) / **nothing** (in-child) | `ASK` mediated; **`NOT_ENFORCED`** in-child — a sandboxed shell's `kill` is not observed |
| `git.local.read` | review tools | policy | ALLOW |
| `git.local.write` | edit tools | policy | ALLOW |
| `git.remote.write` | `release` = `user_task_scoped` | policy + sandbox network profile | **DENY until P1c ships** (gated by P1c `ACCEPTANCE_CRITERIA` 6) |
| `agent.spawn.<id>` | `CODING_AGENT_RUN` scope | policy (dispatch) / **nothing** (in-child) | `ASK` mediated; **`NOT_ENFORCED`** in-child — a sandboxed shell may exec any binary in the workspace |
| `plugin.load` | plugin gateway | policy | **DENY until P8 ships** — see note |
| `danger.full_access` | (none) | **local control plane only** | OFF |

Explicitly **not** added: per-syscall capabilities, per-binary allowlists,
per-environment-variable grants, per-MCP-tool capabilities. They add surface
without closing any invariant in `SECURITY_INVARIANTS.md`.

> **`ASK` is a default, not a capability (corrected in review round 9).** Two
> defaults above were changed from `ASK` to **`DENY` until a phase ships**, because
> a capability whose approved outcome has no enforcement path is a false
> guarantee — the same defect class as an unenforceable approval. The reviewer of
> round 9 caught `fs.read.external`: it defaulted to `ASK` while no phase in
> P0–P8 turned an approved external-read request into an enforceable filesystem
> profile. P1b ships exactly one workspace-only profile, so an approved external
> read would have had nowhere to go.
>
> - `fs.read.external` → resolved by a new phase **P1d — external-path profile
>   extension**, which compiles an operator-approved external path into that
>   spawn's profile and re-runs the same negative tests. Until P1d, external
>   reads are `DENY`. **Qualified in review round 20**: "external" here means
>   outside the project root **and** outside the toolchain-read allow-list P1b
>   already ships, since a toolchain that cannot read its own headers and
>   libraries cannot build, and the plan does not propose to break that. The
>   allow-list is a **named P1b deliverable with negative tests**, not a
>   category — the same "unbounded exception" shape round 9 caught this row for
>   when it defaulted to `ASK`.
> - `net.outbound` → already gated on P1c; its default is now stated as `DENY`, and the **path** released when P1c ships is named in the cell above rather than left as an unqualified "network `ASK`" (round 18, CHECK 5)
>   rather than `ASK`, so no document advertises a network `ASK` that does not
>   exist yet.
>
> The general rule this establishes: **a capability defaults to `ASK` only in the
> same phase that makes the grant enforceable.** An `ASK` default with no
> enforcement path is a promise to the user and a hole in the policy engine.
> `fs.write.external` was already `DENY` and stays there — an approved external
> *write* would need the same profile extension as P1d, and there is no case for
> it in this plan.
>
> `plugin.load` was caught by the same sweep and changed from `ASK` to `DENY until
> P8 ships`. `IMPLEMENTATION_PLAN.md` P0 criterion (6) already requires plugins to
> be **disabled by a hard gate** before P8's restrict-only contract exists, so an
> `ASK` default contradicted a gate that had already been agreed two phases
> earlier. `ASK` here would have implied a mediated approval path for a hook
> contract that is undefined (I17). It is now `DENY`, consistent with the gate.
>
> The remaining `ASK` defaults are deliberately narrowed rather than removed, and
> the reason is the **per-action mediation gap** the round-10 reviewer identified as
> the single recurring theme of this review.
>
> **Dispatch policy is a per-*call* enforcement point, not a per-*action* one.** It
> governs a named MCP tool invocation. It does **not** govern what an already-running
> sandboxed child does next: `kill -9 <pid>`, launching an agent binary, or invoking
> `git` inside a permitted shell are all ordinary process activity inside the
> sandbox and never return to the dispatch boundary. An `ASK` default that implies
> those are mediated would be the same false guarantee as an unenforceable network
> grant.
>
> Therefore:
>
> - `proc.signal.external` and `agent.spawn.<id>` are now split: **`ASK` for a
>   mediated call** (a tool invocation that goes through dispatch), and
>   **`NOT_ENFORCED` for the same effect attempted inside a running shell.** Inside
>   the sandbox there is no OS-level or per-syscall boundary in this plan that would
>   observe them, and none is claimed. (Round 10 wrote this cell as `DENY`; the
>   round-11 repair corrected the table above and this bullet was the residual the
>   round-12 reviewer caught — see FIX 2 in `review/codex-architecture-review.log`.)
>   The realistic consequence, which the vocabulary requires this row to name: a
>   sandboxed shell may signal sibling processes it can see, and may exec any
>   binary in the workspace. The workspace is the confinement unit, so this is
>   within the stated posturing, not a boundary escape.
> - `git.remote.write` is already `DENY until P1c ships` — a remote mutation is an
>   outbound network operation and is gated on P1c `ACCEPTANCE_CRITERIA` 6.
> - `git.local.read` / `git.local.write` stay `ALLOW`. That is sound for the same
>   reason the others are not: they are *allowed* inside the sandbox already, so no
>   mediated approval is being promised for something that happens unmediated.
>
> The general rule, now stated once and covering every axis: **an `ASK` default is
> a claim about an enforcement mechanism. Name the mechanism, and if the effect can
> also be produced inside a running child, say explicitly which of the two is
> covered.**
>
> Round 11 sharpened this into the four-valued vocabulary above, because the rule
> has now been broken in both directions: an `ASK` where only the mediated path is
> covered (round 9), and a `DENY` where nothing is enforced at all (round 10).
> `NOT_ENFORCED` exists so that the second failure has a correct answer. Round 12
> found that the introduction of the vocabulary had itself been incompletely
> propagated — the explanatory bullets in this same section still carried the old
> word — which is why the rule now requires the sweep to be mechanical.

---

## 5. Net new code estimate

| Item | Reuse type | Estimated new Rust LOC |
|---|---|---|
| **P1 prerequisite:** normalize ≥10 direct `Command::spawn()` sites onto the managed path (or attach the hook at each) | `REUSE_EXISTING` + refactor | 250–500 |
| Sandbox backend trait + macOS Seatbelt adapter (**first and only backend in P1**) | `SUBPROCESS_ADAPTER` + `REIMPLEMENT_SMALL_CORE` | 400–600 |
| macOS SBPL minimal profile compiler (security-critical; negative-tested) | `REIMPLEMENT_SMALL_CORE` | 250–400 |
| Sandbox plan propagation to every execution surface (plan must reach each spawn; surfaces that cannot express it must deny) | new glue | 250–450 |
| Sandbox attestation + `sandbox_backend = none` capability gating | new glue | 150–300 |
| Linux `bwrap` adapter (**only after macOS verified**; preferred over writing a launcher) | `SUBPROCESS_ADAPTER` | 150–250 |
| Linux Landlock fallback (only if `bwrap` unavailable **and** justified vs. denying) | `REIMPLEMENT_SMALL_CORE` | 250–400 |
| **Network descriptor compiler + locally-owned enforcing proxy (P1c)** | `DESIGN_ONLY` (proxy machinery `NOT_ASSESSED` in `codex-network-proxy`) | 400–700 |
| **External-path profile extension (P1d)** | `REUSE_EXISTING` (reuses P1b's compiler and per-spawn plan; new code is only the path-rule builder) | 200–400 |
| Policy engine (rules, effects, wildcard, pre-effect floors) | `REIMPLEMENT_SMALL_CORE` | 350–500 |
| Mode model extension 2 → 4 | `REUSE_EXISTING` | 100–150 |
| Approval request/decision + transport trait | `REIMPLEMENT_SMALL_CORE` | 300–450 |
| **Local control channel: IPC isolation, token mint/verify, reachability tests** | new (security-critical) | 250–500 |
| Local approval UI (CLI/desktop) | `DESIGN_ONLY` | 300–600 |
| Session grants + storage + expiry | `REIMPLEMENT_SMALL_CORE` | 200–350 |
| Auto-reviewer + delegation-limit enforcement | `DESIGN_ONLY` | 450–800 |
| Plugin hook contract (restrict-only + re-authorization on mutation) | `DESIGN_ONLY` | 200–350 |
| Audit extensions | `REUSE_EXISTING` | 100–200 |
| Platform verification / integration test harness (macOS first) | new tests | 200–400 |
| **Total** | | **≈ 4.8k – 8.3k LOC** |

> **Estimate corrected (adversarial review rounds 1, 6, 7, 9).** An earlier revision
> gave ≈ 3.0k–4.9k LOC and omitted spawn normalization, per-surface plan
> propagation, control-channel isolation, and platform verification. A later one
> gave ≈ 4.1k–7.2k but omitted the `P1c` network phase. The `P1c` row above was
> itself still missing at round 7, so the printed total was not the sum of the
> rows; it has now been added. Round 9 added the `P1d` row after the reviewer found
> that `fs.read.external` defaulted to `ASK` with no phase able to enforce it.
> **The total is now the arithmetic sum of the rows above** (min 4,750 /
> max 8,300, 19 rows), so it tracks `IMPLEMENTATION_PLAN.md` (P0, P1a, P1b, P1c,
> P1d, P2–P8). It is still "glue and small cores", not a re-implementation of Codex.
>
> Two round-9 additions deserve to be visible in the estimate rather than buried:
> **P1d** exists only because an advertised `ASK` with no enforcement path is a
> false guarantee, and the `RELAXED_BACKEND` work in P1c is profile-widening logic
> inside the already-counted P1c range. Neither adds a new framework.

Deliberately excluded: vendored sandbox crate (≈ 2.4k LOC + transitive), Codex
Guardian subsystem (~9.5k LOC), any policy UI beyond a minimal confirm dialog,
RBAC/multi-person approval, Windows FS/network confinement.

This is the answer to the brief's central question: **the honest delta is a few
thousand lines of glue and small cores, not a re-implementation of Codex.**
