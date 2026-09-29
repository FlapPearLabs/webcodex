# REFERENCE_ARCHITECTURE.md

Target architecture for a secure, low-friction, Codex-style local coding agent on
top of WebCodex, for ChatGPT Web over MCP.

Baseline: `yyjeqhc/webcodex @ 7301186b98527cb4ebc0191f6033474e9abf7c20`.
Repo/commit evidence for every upstream reference is in `OSS_RESEARCH_EVIDENCE.md`.

---

## 1. Current WebCodex execution flow (as-built)

```
ChatGPT Web / MCP client
   │  HTTPS + Bearer (shared key / wc_pat)
   ▼
Cloudflare Quick Tunnel  →  127.0.0.1 (loopback)
   ▼
webcodex-server                        ── src/ (crate `webcodex`, bin webcodex-server)
   │  · MCP JSON-RPC facade            ── src/mcp.rs, src/mcp/protocol.rs
   │  · authn (bearer / PAT)           ── src/auth/
   │  · session guards, path policy    ── src/tool_runtime/
   │  · authority decision (once)      ── src/tool_runtime/permissions/
   │  · audit + evidence sinks          ── src/action_audit*, src/tool_request_trace.rs
   ▼  RunnerRequest / RunnerOperation
webcodex-runner                        ── crates/webcodex-runner
   │  · dispatch                       ── webcodex_runner/dispatch.rs
   │  · shell / process / job          ── shell.rs, job_manager.rs, detached_job.rs
   │  · LSP / MCP / plugin / agent     ── lsp/, mcp_gateway.rs, plugin.rs, coding_agent.rs
   ▼  std::process::Command
webcodex-process                       ── crates/webcodex-process
   │  · ManagedChild::spawn            ── unix.rs:43 / windows.rs
   │  · private process group (unix)   ── process_group(0)
   │  · Job Object + KILL_ON_JOB_CLOSE ── windows.rs
   ▼
OS (no sandbox)
```

**Verified properties of the current state:**

- The MCP surface advertises **only** the tools capability:
  `SOURCE = webcodex @ 7301186b :: src/mcp/protocol.rs:106 ::
  legacy_initialize_payload` → `capabilities: { tools: { listChanged: false } }`.
  No elicitation, no prompts, no client-capability-gated flow.
- The authority decision is evaluated **once**, server-side, before mutation:
  `:: src/tool_runtime/dispatch.rs:1886 :: evaluate_permission_for_tool`,
  documented in `docs/agent/permission-model.md §4`.
- There is **no approval queue and no approve/deny CLI**:
  `docs/agent/permission-model.md §2` ("There is no separate Connector
  command-approval queue or host-side task approval namespace"), `§8`
  ("No approval UI or notification system"), and no `approve` token in
  `crates/webcodex-cli`.
- There is **no OS sandbox**: zero occurrences of `seatbelt`, `sandbox-exec`,
  `landlock`, `bubblewrap`, or `seccomp` anywhere under `src/`, `crates/`, or
  `docs/`.
- Execution-plane process creation **mostly** goes through
  `ManagedChild::spawn` / `spawn_with_options`, but **it is not a single
  chokepoint**:
  `SOURCE = webcodex @ 7301186b :: OSS_RESEARCH_EVIDENCE.md §2.1a/§2.1b`.

  | Path | Non-test sites | Notes |
  |---|---|---|
  | Managed path (`ManagedChild::spawn*`) | ~20 across 9 crates/binaries | shell, jobs, detached job, MCP gateway, plugin, nested agent, validation, LSP, browser, desktop, catalog, runner main |
  | **Direct `Command::spawn()`** | **≥ 10** | incl. `remote_shell.rs:109`, `ssh.rs:1142`, `detached_job.rs:1924/1941/2409`, `persistent-shell/lib.rs:1965`, `src/tool_runtime/helpers.rs:146` |
  | Control-plane (tunnel/server/bootstrap) | 7 files | `src/project_entry*.rs`, `src/server_listener.rs` — operator-initiated |

  **Platform asymmetry (important):** `remote_shell`, `ssh`, and
  `persistent_shell` use `ManagedChild` on **Windows** and a direct
  `Command::spawn()` on **Unix**. macOS — the primary target — therefore takes
  the *direct* path. This was not noticed in the first revision.

  **Also withdrawn:** the earlier claim of "zero direct bypasses in the runner"
  was false (it came from a `grep` that silently matched nothing). See
  `OSS_RESEARCH_EVIDENCE.md §2.1a`.

**The defect this architecture must fix:** policy exists, sandbox does not.
`trusted_agent` auto-authorizes consequential tools "after hard safety", but
"hard safety" has no OS component on the shell surface, which the baseline
demonstrated empirically (`cd /tmp`, `ls ~`, `curl` all succeeded unapproved).
A second defect follows from the inventory above: even once a sandbox exists,
it cannot be attached at one function until the direct spawn sites are
normalized onto a single path.

---

## 2. Target flow

```
                    ┌──────────────────────────────────────────┐
                    │  LOCAL TRUSTED CONTROL PLANE             │
                    │  (host CLI / local UI — never MCP)       │
                    │   · set mode (incl. danger)              │
                    │   · answer approvals                     │
                    │   · grant/revoke session grants          │
                    └───────────────┬──────────────────────────┘
                                    │ signed, single-use, local-only
                                    ▼
ChatGPT Web ──MCP──▶ webcodex-server ──▶ Security Broker ──▶ webcodex-runner
                            │                    │                  │
                            │                    │                  ▼
                            │                    │        Execution Broker
                            │                    │        (RunnerOperation)
                            │                    │                  │
                    ┌───────┴───────┐    ┌───────┴───────┐  ┌───────┴────────┐
                    │ Policy Engine │    │ Approval Eng. │  │ Sandbox Backend│
                    │ ALLOW/ASK/DENY│    │ once/session/ │  │ Seatbelt/bwrap │
                    │ + DENY floors │    │ always/deny   │  │ /Landlock      │
                    └───────────────┘    └───────┬───────┘  └───────┬────────┘
                                                 │                  │
                                        Approval Transport   sandbox::apply
                                        (local UI now;       (every spawn site)
                                         MCP later if it   ┌──────┴──────┐
                                         ever exists)      │  OS sandbox │
                                                           └─────────────┘
```

The brief's proposed candidate architecture is **confirmed with two changes**:

- **Change 1 (keep):** Approval must be answered on a **local** control plane,
  because WebCodex's MCP surface does not and will not expose an approval
  channel (§1, §7). The brief anticipated this fallback; the research makes it
  the primary design, not a fallback.
- **Change 2 (adjust):** the Sandbox Engine sits **below** the Execution Broker,
  not as a sibling of the Policy Engine. Sandboxing is not a decision; it is an
  execution property that must apply to every descendant (I4/I5). It is applied
  at the **sandbox attach surface** — the pair {managed spawn path, shared hook}
  — because no single spawn function exists to attach to (§1, §4).

---

## 3. Trust boundaries

| # | Boundary | Inside | Outside | Crossed by |
|---|---|---|---|---|
| B1 | Model ↔ Host | untrusted model output | host code | MCP `tools/call` |
| B2 | Transport ↔ Host | public tunnel | loopback server | bearer auth + revocation |
| B3 | Policy ↔ Execution | decisions | capabilities | `RunnerOperation` |
| B4 | Host ↔ OS | sandboxed child | kernel | `exec` under sandbox profile |
| B5 | Workspace ↔ System | project tree | rest of FS | path resolution + sandbox rules |
| B6 | Local control ↔ Remote | operator | MCP client | local-only IPC |
| B7 | Plugin ↔ Core | plugin hooks | policy engine | restrict-only hook contract |

**B1 is the boundary the model influences by way of a declared tool call, and
it must never be the one that decides capability.** ~~Every other boundary is
enforced by host code or the OS.~~ **The second sentence is withdrawn in review
round 23, CHECK 3.** Round 20 removed "only" from this sentence and correctly
declined to enumerate routes — but the replacement still asserted that *every*
other boundary **is enforced**, which is a claim about six boundaries at once,
and the paragraph immediately below records that the Browser/Computer route to
**B6** (local control ↔ remote) is `UNKNOWN` and presently `NOT_ENFORCED`.
A blanket enforcement claim one line above a named `NOT_ENFORCED` route is the
"repair beside a claim" shape (L13) with the repair *above* the claim instead of
beside it.

**What replaces it, and a second overclaim withdrawn in the same round (review
round 24, CHECK 3, newly-raised finding 3).** The round-24 version replaced the
universal claim with a per-boundary split — "B2, B3, B4 and B7 are enforced by
host code or the OS" — and **that was also wrong, in a new direction: B4 and B7
are not enforced today.** This plan's own submitted facts say WebCodex has **no
OS sandbox** (B4 is what P1a/P1b build) and **no restrict-only plugin contract**
(B7 is what P8 builds). A replacement that is a false particular is not a fix.
**The table above is an architectural map, not evidence**: its `Crossed by`
column names the *proposed* crossing mechanism for each boundary, and reading it
as a statement of present enforcement is precisely the category error the
round-23 producer sweep found. So the honest statement separates the two
states, and only the first is supportable today:

| Boundary | Mechanism named above | Present state in the baseline | Phase that builds it |
|---|---|---|---|
| B1 | `MCP tools/call` | host-owned; the model cannot decide capability | — (already so) |
| B2 | bearer auth + revocation | **UNVERIFIED** — the boundary *map* names this mechanism and the plan depends on it, but **no observation in `§2` indexes bearer authentication or revocation**, so it cannot be called observed (review round 25, CHECK 3; the correction was computed in round 26 and dropped before writing, and is applied here). Treated as **assumed-present and unexamined**: a bearer-auth failure here would be a transport-layer break, not a sandbox escape, which is why no phase in this plan gates on it — and also why it is not recorded as a guarantee. Making it a gate is a P3 work item, not a P1 one | — |
| B3 | `RunnerOperation` pre-effect gate | **partly enforced**: a pre-effect permission gate and a post-effect classifier exist; a **central pre-effect hard floor does not** (I9) | P2 |
| B4 | `exec` under a sandbox profile | **ABSENT** — no OS sandbox exists in the baseline | P1a, P1b |
| B5 | path resolution + sandbox rules | **partly**: path checks exist; the sandbox half is absent with B4 | P1b, P1d |
| B6 | local-only IPC | **`NOT_ENFORCED`** on the Browser/Computer route, which is `UNKNOWN` | P3 (7b), CP-07 |
| B7 | restrict-only hook contract | **ABSENT** — today's plugin path is undefined w.r.t. I17 | P8, with the P0 (6) plugin gate in the meantime |

**So: of the six boundaries other than B1, the baseline enforces **none**
unconditionally on indexed evidence, enforces two partly (B3, B5), does
not enforce three (B4, B6, B7), and has one boundary (B2) whose mechanism
is assumed but unexamined.** The round-25 version of this sentence said
"one unconditionally (B2)", which was a present-state claim with no
observation behind it — **the same error as the design-table read
the round-24 review caught, in the opposite direction and one row over.**
Round 25 fixed the false rows and introduced this one; a corrected table
whose summary is not re-derived from the corrected rows is not a corrected
table. The sentence that survives from the original is the first one,
which is the part the evidence supports: a declared tool call influences B1,
and B1 does not decide capability. The route inventory and its `UNKNOWN`
remain in the paragraph below. **This sentence was rewritten in review round 20,
CHECK 3.** Round 19 added a qualification *immediately after* it and left the
sentence standing — the exact "repair beside a claim" shape round 19 itself had
just named as a rule, which is the fourth time this pattern has appeared in this
review. The previous wording, "B1 is the only boundary the model can influence
**through the tool surface**", was still false on its own terms: **Browser/CDP
and Computer/UI-automation are tool-surface operations**, and they can cross B6 by
driving a host user-interface. The rewritten sentence drops "only" and asserts
what the evidence supports — that a declared tool call influences B1, and that
B1 does not decide capability — without claiming to enumerate every route by
which model output can reach host state. The route inventory and its `UNKNOWN`
remain in the paragraph below.

**The "only" is scoped, and the scope excludes whole surfaces (corrected in
review round 18, CHECK 3 — a newly-raised finding).** The previous sentence was
unqualified, and the submitted operation enum contains model-facing
**Browser/CDP** and **Computer/UI-automation** operations alongside shell and
job operations. Those surfaces let the model drive a **host user-interface**
which, on a general-purpose desktop, may itself display a dialog, click a
button, or drive a local terminal that is *not* under the runner's sandbox
profile. No boundary between "model-influenced MCP tool call" and "model-
influenced host UI interaction" was demonstrated, and the sandbox profile
governs processes, not synthesised input events. Two consequences, both stated
rather than smoothed over:

1. **The claim is narrowed** to: a **declared tool call** influences B1, and B1
   must never be the boundary that decides capability. The word "**only**" is
   **removed, not merely qualified** (review round 20, CHECK 3) — round 19 kept
   it inside the narrowing itself, so the sentence still asserted uniqueness while
   the paragraph beneath it said a second channel exists. Whether the model can
   influence a further channel — a host UI reached by synthetic input — is **not
   established** and is `UNKNOWN`.
2. **A new obligation is created.** Any Browser or Computer surface that is
   model-reachable must be treated as a **host-interaction surface** for the
   purposes of P3 criterion 6, `REFERENCE_ARCHITECTURE.md §10` control-channel
   reachability, and I3. The test that exists today ("a sandboxed shell cannot
   reach the control channel") does **not** cover them, and P3 does not pass
   until a named test covers the UI-mediated route as well. Until that test
   exists, the honest status of the local-approval boundary against these surfaces
   is `NOT_ENFORCED`, and `IMPLEMENTATION_PLAN.md P3`'s "provably not reachable
   from the model" is narrowed accordingly.

---

## 4. Control plane vs execution plane

| | Control plane | Execution plane |
|---|---|---|
| Component | `webcodex-server` (`src/`) | `webcodex-runner` (`crates/webcodex-runner`) |
| Reachable by model | Yes (MCP tools) | **Not directly callable over MCP** — it is reached *indirectly*, via a `RunnerOperation` that the control plane issues in response to a model tool call (corrected in review round 13, newly-raised finding). The execution plane **does** carry model-directed work; "No (transport only)" was too strong and contradicted this document's own target flow |
| Owns | authn, session guards, **policy decision**, audit | process spawn, jobs, sandbox attach |
| Spawns processes | Yes — tunnels, servers, `git` inventory, **and `src/tool_runtime/helpers.rs:146`** | Yes — all tool-driven execution |
| Must never | spawn model-directed processes directly | make policy decisions |

**Reachability of control-plane spawns — stated as a constraint, not a fact
(corrected in review round 4).** An earlier revision asserted that control-plane
spawns are "not model-reachable". That is **not established**: `src/tool_runtime/`
is server-side code that handles tool requests, `helpers.rs:146` was classified
model-reachable in `OSS_RESEARCH_EVIDENCE.md §2.1a`, and §8 there records that
the call graph has not been proven. The defensible statements are:

- Control-plane spawns are **intended** to be operator-initiated (bootstrap,
  tunnels, server/runner launch, `git` inventory).
- Whether that intent holds for every `src/tool_runtime/*` spawn is an **open
  P0 question**, answered by the spawn inventory and the CP-04 manifest test.
- Until then, no document in this branch may state categorically that any
  `src/` spawn is unreachable from a tool call.

**Rule:** the policy decision is made in the control plane (already true:
`dispatch.rs:1886`); the sandbox is attached in the execution plane (new). This
split keeps the decision auditable and the confinement un-bypassable.

**Sandbox-attach invariant (corrected in rounds 3 and 13):** the sandbox is not
attached at a single function today, and this design does not claim it will be —
the wording that survived until round 13 said it "must ultimately be" attached at
one, which contradicted the two-part surface specified three lines later and the
≥10 unmanaged direct spawn sites recorded in `OSS_RESEARCH_EVIDENCE.md §2.1a`.
The corrected statement is: **the attach surface is the pair {managed spawn path,
shared hook}, and convergence onto one normalized function is a stated goal of P0,
not a precondition of this design.** The reason is recorded here rather than
assumed away: `ManagedChild::spawn` covers the majority of sites, while at least
ten non-test direct `Command::spawn()` sites (including model-reachable ones on
Unix) bypass it — see §1 and `OSS_RESEARCH_EVIDENCE.md §2.1a`. Therefore:

1. **Normalize first.** Every **catalogued** model-reachable spawn site must
   either route through the managed path or call a shared
   `sandbox::apply(&mut Command, &SandboxPlan)` hook immediately before spawning,
   **and the spawn API refuses to create a child that carries no plan at all**.
   This is a prerequisite step in P1a. **Corrected in review round 18, CHECK 4/11
   and a newly-raised finding**: this step previously read "**every**
   model-reachable spawn site", which is the universal enumeration claim that
   `SECURITY_TEST_VECTORS.md` CP-01 and CP-04 abandoned two rounds ago — an
   uncatalogued site exists by construction, so the step as written could not be
   completed and would have been discharged by assertion. The two-part form is
   the achievable one, and the second half is the part that carries the security
   weight: the refusal is **unconditional with respect to the catalogue** — a
   caller that reaches the spawn API without a plan is refused whether or not
   it appears in `research/spawn-surface-inventory.md`, so an uncatalogued
   site cannot obtain an unconfined child by going through that API.
   **This is not the same as binding uncatalogued callers, and the earlier
   sentence that said so is withdrawn (review round 23, CHECK 4).** Round 22
   separated the cooperative refusal from the detective accounting here and in
   `IMPLEMENTATION_PLAN.md` P1a, but this earlier summary retained the stronger
   form — the fourth consecutive round in which a withdrawn claim survived in
   an earlier section than the one that withdrew it (L13). What the refusal
   does **not** reach, per P1a(1b'), is a raw `fork`/`posix_spawn`, a plugin-crate
   spawn, a re-exported alias, or a site on the build-time control-plane
   exemption list. Those are the cases the catalogue cannot enumerate, and the
   residual they form is `UNKNOWN`, not closed.
2. **Attach at the hook, not at one caller.** With normalization complete the
   attachment surface is the *pair* {managed path, shared hook} — it is **not** a
   single function. `ManagedChild::spawn` remains the majority caller, and the
   hook is what the remaining sites call; correctness depends on **coverage of
   all sites**, not on the existence of one function.
   > Corrected after adversarial review round 2: an earlier revision said
   > normalization "makes one attach point". Allowing the shared-hook alternative
   > explicitly does not create a single function, and claiming otherwise
   > re-introduced the very overstatement this section was written to remove.
   > The invariant that matters is *no reachable spawn without a plan*, enforced
   > by the CP-01…CP-06 coverage tests, not a call-graph shape. **The word
   > "enforced" is withdrawn here in review round 21, CHECK 11.** This summary
   > restored the closure claim that CP-02, CP-04 and P1a (1b') have each
   > withdrawn in detail: CP-02 asserts attachment for the **catalogued** set
   > plus a **cooperative** API refusal, CP-04 leaves non-catalogued reachability
   > `UNKNOWN`, and (1b') states the refusal does not bind raw `fork`/`posix_spawn`,
   > unaudited entry points, exempt control-plane callers, or plan widening. A
   > summary that says "enforced by CP-01…CP-06" is stronger than any of them, and
   > a reader who consults the summary rather than the criteria would draw the
   > stronger conclusion. Corrected form: **CP-01…CP-06 enforce attachment for the
   > catalogued set and refuse uncooperative callers; they do not enforce the
   > universal property, which is `UNKNOWN` and gated separately by P1a (1c)**.
3. **Inheritance is a property of the sandbox facility, not of the process
   group.** These are two different mechanisms and must not be conflated:
   - *Process group / Job Object* = ownership for signal delivery and tree
     termination (`unix.rs:43-64`, `windows.rs`). It says nothing about
     confinement.
   - *Sandbox inheritance* = the platform facility is entered **before `exec`**,
     so descendants created by `fork`/`exec` under it remain confined (e.g. a
     Seatbelt profile applies to the process and its children; a
     bubblewrap/namespace sandbox is created once and inherited).
   Being in the same process group is therefore **not** evidence of sandbox
   inheritance, and the design must state which mechanism provides which
   guarantee. A child that can create a new namespace/session or otherwise
   leave the facility is the escape case that `SECURITY_TEST_VECTORS.md` EX-13/
   EX-14 exist to probe.

---

## 5. Security Broker

> **Correction (adversarial review round 1, CHECK 2).** An earlier revision of
> this section listed `is_hard_denied_output` as a pre-execution "floor". That was
> wrong. In the current code that function runs **after** execution and only
> decides whether to attach soft authority metadata to the finished result. It is
> a *result classifier*, not an enforcement point. Pre-effect denial and result
> classification are distinct concerns and are separated below.

### 5.1 Where denial actually happens today (verified ordering)

`SOURCE = webcodex @ 7301186b :: src/tool_runtime/dispatch.rs:1883-1912` — the
authoritative ordering comment is explicit:

```
// Authoritative single evaluation (kernel must not re-evaluate).
// Order: session/auth guards above → permission gate → mutation below.
// Path/sensitive hard checks still run inside tools; hard-deny filter
// suppresses permission attach so soft policy never overrides them.
```

| Stage | Location | Runs | Effect on the action |
|---|---|---|---|
| Session / auth guards | `dispatch.rs` (above :1883) | pre-effect | deny |
| Policy gate (mode-level) | `dispatch.rs:1886 evaluate_permission_for_tool` → `:1893 permission_execution_denied_result` | **pre-effect** | deny; returns before mutation |
| Path / sensitive hard checks | *inside individual tools* ("still run inside tools") | pre-effect per tool | deny |
| `is_hard_denied_output` | `dispatch.rs:2006` (after execution) | **post-effect** | classifies the result; suppresses the soft authority attach; does **not** undo the action |

**Consequence:** today there is a genuine pre-effect denial point for *policy*
denials, but the "hard" path/secret checks are enforced **per tool**, not at one
central floor. That is exactly the kind of scattered enforcement that produces
the gaps recorded in `SECURITY_INVARIANTS.md` (e.g. the shell surface has no
equivalent check at all). The central floor is therefore a *proposal*, and it
must be evaluated in the pre-effect stage — not inherited from a post-hoc
classifier.

### 5.2 Proposed Security Broker

One module, one entry point, placed in the control plane so decisions are
auditable next to the existing permission records.

```
SecurityBroker.evaluate(call) -> Verdict

  input:
    tool_name, tool_args_hash, project_id, session_id,
    capability_request (derived), mode, transport_state

  A. PRE-EFFECT ENFORCEMENT (must complete before any mutation)
     A1. FLOORS        hard DENY; mode-independent; unreviewable   ← I9
     A2. POLICY        ALLOW / ASK / DENY per capability           ← I8
     A3. APPROVAL      only if ASK: once|session|always|deny       ← I10..I12
     A4. SANDBOX PLAN  profile handed to the execution plane       ← I4, I13

  B. POST-EFFECT RECORDING (must not decide anything)
     B1. classify the outcome for the ledger
     B2. attach the decision record; suppress soft metadata on hard denial

  output: Verdict {
    outcome, reason, policy, risk, request_id,
    grant, sandbox_plan, audit_record
  }
```

Ordering within stage A is normative: **Floors → Policy → Approval → Sandbox
plan.** A reviewer verdict (see §8) participates only in A3 and only within the
delegation limits defined in §8.

Mapping onto the existing codebase:

| Broker stage | Existing anchor | Change |
|---|---|---|
| A1 Floors | *no central pre-effect floor exists*; per-tool path/secret checks only | new central pre-effect floor; migrate per-tool checks behind it |
| A2 Policy | `permissions/policy.rs:192 decide_for_required_tool` | add rule model + `ask` outcome |
| A3 Approval | *(none)* | new: port Hermes transport contract (§7) |
| A4 Sandbox plan | *(none)* | new: compile a permission profile → platform profile |
| B1/B2 Recording | `permissions/mod.rs:124 is_hard_denied_output`, `:156 permission_summary_from_events` | keep as **post-effect classification only**; convert prose matching → structured error kinds |

**Non-negotiable from this correction:** no document in this branch may cite
`is_hard_denied_output` as evidence that a hard check *prevented* an action.

---

## 6. Sandbox backend

A trait with per-platform implementations, applied **pre-`exec`** at the
**sandbox-attach surface** defined in §4 — which is the *pair* {`ManagedChild::spawn`,
the shared hook at direct spawn sites}, not a single site. This section previously
said "attached inside `ManagedChild::spawn`", which contradicted the pair specified
in §4 and the shared hook P1a introduces; corrected in review round 14, CHECK 4.
A caller of the trait that is neither of the two is an uncovered spawn site, and
CP-01/CP-02 fail for it.

```rust
pub trait SandboxBackend {
    fn name(&self) -> &'static str;
    /// Rewrite `cmd` so the child runs confined. Must be applied pre-exec.
    fn apply(&self, cmd: &mut Command, plan: &SandboxPlan) -> Result<(), SandboxError>;
}
```

Reference mechanism (from Codex): **argv rewriting**, not process wrapping after
the fact — `sandboxing/src/manager.rs:352 SandboxManager::transform` prepends the
platform launcher and rewrites argv:

- macOS: `/usr/bin/sandbox-exec -p <SBPL> <program> <args...>`
  (`sandboxing/src/seatbelt.rs:62`, `manager.rs:434-471`).
- Linux: prefer system `bwrap` (`sandboxing/src/bwrap.rs
  find_system_bwrap_in_path`); fall back to a Landlock+seccomp launcher
  (`linux-sandbox/src/landlock.rs`).
- Windows: restricted token / MXC (`sandboxing/src/windows.rs`,
  `windows_mxc.rs`) — **out of scope for this stage**; WebCodex's Windows Job
  Object remains the only control there.

**Profile axes** (from `protocol/src/models.rs:422 PermissionProfile::Managed`):
`file_system` and `network` are independent. The sandbox plan carries both, and
the network axis maps to distinct launcher rules (Codex's localhost / DNS :53 /
open-outbound split at `seatbelt.rs:336-346`).

### 6.1 Scope discipline (adversarial review round 1, CHECKS 5 and 9)

An earlier revision presented the sandbox as a small adapter and described
network independence as if it were already actionable. Both were overstated.

**(a) Profile construction is security-critical code, not glue.** A launcher
binary is reusable; deciding *which paths and hosts may be reached* is the
security-critical part, and a single wrong rule in a generated profile fails
**open** without an error. Consequences for the plan:

1. **Ship one platform and one profile first** (macOS + a read/write-workspace /
   no-network profile), with negative tests that prove a
   deliberately-wrong-in-the-permissive-direction rule is caught.
2. **No backend ⇒ deny**, not "run unsanded". A missing `bwrap`, an unsupported
   kernel, or a profile-generation error must produce a first-class
   `sandbox_backend = none` state that *gates capabilities*, rather than silently
   degrading.
3. Every additional backend (Linux `bwrap`, Linux Landlock) must be justified by
   an **assessed reuse alternative**, not added for coverage. If the platform can
   use `bwrap` (a well-tested external project), prefer it over writing a
   Landlock launcher.

**(b) How an `ASK` becomes an enforced per-request network profile.** The target
design must name the mechanism, not just the axis. Required mechanism sketch:

- The network decision is made **per spawn**, not per session: the approved
  request carries an explicit network descriptor (`deny` | `localhost-only` |
  `allow:[host:port,…]`).
- That descriptor is compiled into the sandbox profile for **that child**, so a
  grant issued for one spawn is not inherited by a *different* spawn.
  **This is per-spawn, not per-action (corrected in review round 7).** For
  long-lived children (`persistent_shell`, `script`, `job`, nested `agent`) the
  process performs many actions after a single spawn, so the grant authorizes all
  of that child's later egress. There is no per-action boundary at the process
  level. Two requirements follow: (i) the grant a human approves must be worded
  as **process-scoped** ("this session's shell may reach `example.com:443` for
  its lifetime"), never as "this one command"; (ii) long-lived surfaces must be
  **excluded from network `ASK` outright** and kept at `DENY`-only, until a
  per-action mediation point is designed — rewording the approval does not make
  them eligible. Recorded as `KNOWN_LIMITATION` in
  `OSS_RESEARCH_EVIDENCE.md §8`.
- **Long-lived children are also not re-evaluated when the authority mode
  changes, and no stop or revocation mechanism is proposed for them. Added in
  review round 20, from a round-19 finding that `SECURITY_TEST_VECTORS.md`
  FS-08 and MD-02 require `READ_ONLY` to deny writes on *every* surface while
  this section permits long-lived surfaces to exist at all.** The two are only
  consistent under an assumption neither document states: that a mode change
  terminates or re-profiles the children already running. As designed, a
  `persistent_shell`, `script`, `job` or nested `agent` spawned under `AUTO`
  keeps its compiled profile after the session switches to `READ_ONLY`, so
  FS-08's "any write, on any surface" is `DENY` at the **decision** layer and
  `NOT_ENFORCED` at the **already-running-process** layer. **This plan does not
  claim otherwise.** Three options exist and none is free: (i) a
  kill-on-mode-change rule for long-lived children, which is disruptive to real
  work and is a product decision rather than a security one; (ii) re-compiling
  the profile of a running child, which most OS sandbox facilities do not
  support; (iii) accepting the gap and recording it. **This plan takes (iii) for
  now and marks it explicitly**, because the honest cost of (i) falls on the
  operator and the honest cost of (ii) is an unimplemented platform feature.
  Until an operator decides, the residual is `NOT_ENFORCED` and MD-02's
  session-end leg is `NOT_ENFORCED` for the same reason round 17 found for its
  own session-end leg.
- The descriptor must be propagated to **every** execution surface that can open
  a socket: shell, jobs, detached jobs, nested agents, MCP/plugin gateways,
  browser (CDP), and LSP servers. A surface that cannot express the descriptor
  must be treated as `deny`.
- **SSH is a special case and must not be counted as locally sandboxed
  (corrected, review round 2).** Confining the local `ssh` *client* process
  constrains only the client's own filesystem and socket usage. Once the client
  authenticates, the **remote host** executes commands in a trust domain this
  sandbox does not govern at all. Therefore: (a) `ssh` is a **capability**
  (`exec.remote`) that requires its own ALLOW/ASK/DENY decision, not merely a
  network grant; (b) the sandbox plan must not be described as confining remote
  execution; (c) the descriptor to propagate locally is only "may this client
  open a socket to that host:port", and any claim about what happens on the far
  side must be recorded as **outside the boundary**.
- **DNS is decided, not deferred.** Allowing `*:53` to a resolver creates an
  outbound channel (tunnelling over DNS). The default is therefore: **deny DNS**;
  name resolution happens through a **local enforcing proxy** on a
  control-plane-owned socket parameterised by the request's allow-list, or not at
  all. If no local enforcing proxy exists, the enforceable descriptor is
  **IP:port only** and hostname-scoped grants are **not offered** (an unenforced
  hostname allow would be a false guarantee). Codex's managed-network/proxy
  machinery (`enforce_managed_network`, `codex-network-proxy`) is the candidate
  implementation, and is currently marked **not assessed** in
  `OSS_RESEARCH_EVIDENCE.md §7.1`; reading it is a P1 task.
- **The proxy must be bound per spawn, or it enforces nothing (added in review
  round 9).** "Control-plane-owned" answers *who runs the proxy*. It does not
  answer *which destinations a given child may reach* — and without that, a proxy
  reachable by every sandboxed child cannot enforce a per-spawn grant: the first
  child to connect gets whatever the proxy permits globally, and every later
  child inherits it. Ownership is not enforcement. Each spawned child must
  therefore receive a **per-spawn, unforgeable capability** (an inherited fd, or
  a socket bound to a per-spawn credential the child can neither mint nor widen),
  and the proxy must hold an allow-list keyed to that capability. A child with no
  valid capability gets `DENY`; a child presenting spawn A's capability gets
  **only** spawn A's destinations. This is a **precondition** of using a proxy at
  all: without it, hostname grants remain unavailable however carefully the proxy
  itself is operated (`IMPLEMENTATION_PLAN.md` P1c `PROXY_SPAWN_BINDING`).
- **Localhost rules are also egress.** `localhost:*` permits reaching any local
  service, including other developer tooling and potentially the host control
  plane. Localhost grants must be port-scoped to the specific local service the
  action needs, and must explicitly exclude the control-plane channel (§10). A
  grant must never be phrased as "localhost allowed".
- Enforcement must hold for **grandchildren** (a `python3` socket, a `curl`
  launched by `node`), which is a property of the pre-`exec` facility (§4), not
  of argument inspection.

Until (b) is implemented and tested, the network axis remains **design-level**.
The *current-state* observation stands and is unaffected: WebCodex projects
`network` as `auto` together with shell/git/write
(`permissions/policy.rs:234`), i.e. today there is no network axis at all.

**Declared guarantee (must be stated in docs and tests):** the sandbox is
established before `exec` and is inherited by the entire descendant process
tree, **unless the child escapes the sandbox facility itself**. Platform
escape surfaces (e.g. helpers that clear their own confinement) are explicitly
in the test vectors rather than assumed away.

---

## 7. Approval lifecycle

Modeled on Hermes' transport contract, which is the closest existing
implementation of "policy ≠ transport":
`SOURCE = NousResearch/hermes-agent @ 79dbb145 :: hermes_cli/approval_transport.py`.

```
SecurityBroker: policy = ASK
        │
        ▼
ApprovalEngine.create(capability, args_digest, session, project, pattern_scope)
        │   request_id = uuid
        │   digest      = sha256(canonical(request, session_key))
        │   allowed_choices = once [+ session] [+ always] + deny
        ▼
ApprovalTransport.present(request)         ← local UI today
        │
        │  timeout / error / busy / interrupted / invalid / stale
        │  ─────────────────────────────────► DENY   (every failure)  I10
        ▼
ApprovalDecision { request_id, request_digest, choice }
        │
        ▼
validate: request_id matches ∧ digest matches ∧ choice ∈ allowed_choices
        │  any mismatch ────────────────────► DENY (stale / invalid)
        ▼
apply choice:
   once     → this request only
   session  → grant (session_id, capability, pattern)                       I11
   always   → persist grant (project_id, capability, pattern)
   deny     → reject; cascade to sibling pending requests in the same session
        ▼
emit audit record (decided_by, decided_at, lifetime, digest)                I12
```

Transport implementations planned (only the first is in scope now):

1. **Operator CLI over a unix domain socket** — primary, and the only approval
   channel (round 33). The runner creates the socket outside every writable root,
   mode `0600`, owned by the operator's uid; the operator answers with
   `webcodex approve|deny <request_id>` carrying the single-use token.
   Auto-cancel on timeout, non-interactive defaults to deny
   (`pi @ 11894012 :: examples/extensions/timed-confirm.ts`,
   `:: examples/extensions/dirty-repo-guard.ts` — the *timeout semantics* are
   reused; its **window** is not, and the reason is the next sentence).
   **Why not a desktop dialog:** a dialog is a surface a `Browser/CDP` or
   `Computer/UI-automation` route can synthesise input into, so a decision read
   off it is not established to be a human decision. A unix socket requires a
   filesystem permission the sandbox denies and a peer credential the kernel
   checks — a **different capability**, one that input synthesis does not confer.
   The cost is real and is named: approval is a terminal action, not a pop-up.
2. MCP elicitation — **not available**; see §13.
3. Gateway/remote push — future, must never be able to approve danger mode.

---

## 8. Auto-review lifecycle

The reviewer is a **second model call**, not a code path inside the policy
engine. Codex's shape is the reference and it is an **extension**, not core:
`SOURCE = openai/codex @ 69f71405 :: ext/guardian-reviewer/`,
`ext/guardian-v2/`, `core/src/guardian/{decision,review,reviewer_config}.rs`.

```
Policy says ASK (or mode = approve_for_me)
        │
        ▼
ReviewerRequest {
   planned_action,          ← the concrete action (argv, cwd), not prose
   capability_request,
   transcript_excerpt,      ← bounded; UNTRUSTED input
   sandbox_violations,
   network_intent,
   policy_context, floors,
   deadline
}
        │
        ▼
Reviewer model call (isolated context, own policy prompt, own model selection)
        │
        ▼
ReviewerVerdict = ALLOW | ESCALATE | DENY
        │
        ├── ESCALATE → presented to the **operator over the P3 unix-socket channel** (round 33; round 32 made this `DENY` by withholding the affordance, which removed the product's approval UX and left four downstream documents describing an approval this plan no longer delivered — the socket restores it)
        ├── ALLOW    → permitted ONLY IF no floor fired
        └── DENY     → denied
        ▼
timeout / error / malformed ──────────────────► DENY
```

**Normative constraints:**

- **Actor/reviewer separation:** the reviewer must be a different invocation with
  a different context from the acting agent. Codex resolves a separate reviewer
  model and injects policy instructions it controls
  (`guardian/reviewer_config.rs::build_guardian_review_session_config`).
- **Delegation limits (adversarial review round 1, CHECK 7).** An earlier
  revision said the reviewer "may only narrow", then allowed reviewer `ALLOW`
  where policy said `ASK`. That is an authorization *upgrade* and is only
  acceptable if the host has explicitly delegated that decision. The corrected
  rule:

  1. Reviewer `ALLOW` is honored **only** for action classes the host policy has
     explicitly placed in the delegated set, and only within fixed argument
     limits declared by that policy. The delegated set is a *host-owned
     allowlist*, never a reviewer-supplied value.
  2. Anything outside the delegated set (danger-mode-adjacent actions, network
     egress to a new host, credential paths, agent spawning, remote git
     mutation, `danger.full_access`) is **human-only**. The reviewer may only
     return `ESCALATE` for it; `ALLOW` is coerced to `ESCALATE`.
  3. Reviewer `DENY` always applies. Reviewer output can never widen the
     delegated set.
  4. By default the delegated set is **empty**, so `APPROVE_FOR_ME` behaves as
     "ask a human, with the reviewer available as an advisory signal" until an
     operator opts specific classes in.
- **Never implicit allow:** a missing or failed reviewer is never an allow —
  Codex states this explicitly at `guardian/decision.rs:44` ("`None` requests the
  existing user flow. No contributor is never an implicit allow.").
- **Floors first:** the reviewer is consulted only after floors pass, and its
  ALLOW cannot resurrect a floor DENY (`SECURITY_INVARIANTS.md` I9).
- **Fail closed:** `ReviewDecision::default()` is `Denied`; `TimedOut` is a
  first-class non-allow outcome (`codex @ 69f71405 :: protocol/src/protocol.rs:4159`).
- **The reviewer's input is untrusted data, and its verdict is not a security
  boundary.** A reviewer is a probabilistic model. Its transcript input may
  contain injected instructions, and its output can be manipulated by
  adversarial content. Therefore the enforceable property is *structural*, not
  behavioural:
  - the reviewer's output must be parsed into a **closed enum**, and anything
    unparseable is DENY (`SECURITY_TEST_VECTORS.md` AR-05);
  - the reviewer's decision must pass through the delegation limits above, which
    are implemented in host code;
  - a reviewer `ALLOW` can never exceed the host-declared delegated set.

  An earlier revision of this document wrote that an injected transcript must
  leave the verdict "unchanged" (`AR-08`/`PI-09`). That is not a testable
  security property and has been removed; the tests now assert the structural
  constraints instead.

---

## 9. Session grant lifecycle

Grant = `(project_id, capability, resource_pattern, lifetime)`.

```
issued  → active → (expired | revoked | session_end)
```

| Property | Rule | Reference |
|---|---|---|
| Scope | project + capability + pattern. Never global. | `opencode :: permission/saved.ts` (`(project_id, action, resource)`) |
| Lifetimes | `once` (request), `session` (session id), `always` (persisted) | `codex :: ReviewDecision::{Approved, ApprovedForSession, ApprovedExecpolicyAmendment}` |
| Keying | `(session_key, pattern_key)` for session grants | `hermes :: tools/approval.py:249,350` |
| Expiry | session grants die with the session; a new session re-asks | **two different observations, do not conflate them** (corrected in review round 16, newly-raised): `hermes :: tools/approval.py:285` `clear_session(session_key)` is a **revocation of already-issued session grants** — that is the reference for this property. `opencode :: permission.ts:220-247` (finalizer cascades `reject` to pending requests) establishes only that **unanswered requests are denied when a session ends**; it is evidence about *pending* requests, not about revoking a grant that was already issued, and it was previously cited here as if it were the latter. If no revoke path exists, a grant issued near a session's end would survive into a state the design assumes cannot occur |
| Replay | a decision binds `request_id` **and** `request_digest`; mismatch ⇒ `stale` ⇒ DENY | `hermes :: approval_transport.py:167` |
| Danger mode | **never** grantable by a session grant | I3 |

**Anti-widening rule:** a grant may only cover the capability and pattern
explicitly present in the request that produced it. OpenCode implements this by
persisting only `request.save[]` resources
(`opencode :: permission.ts:250-256`) — adopt that constraint.

---

## 10. Danger mode lifecycle

`DANGER_FULL_ACCESS` exists as a *local* state, never as a remote capability.

> **Correction (adversarial review round 1, CHECK 3).** An earlier revision
> claimed local-only IPC plus a single-use token made remote activation
> *impossible*. The supporting evidence establishes only two things: (a) WebCodex
> advertises no elicitation capability and its CLI has no approve/deny command,
> and (b) no tool in the manifest changes authority. It does **not** establish
> that the local control channel is unreachable from the model, because the model
> can run shell, browser, and plugin operations, and no IPC isolation was
> demonstrated. The claim is therefore downgraded from *impossible* to
> *must be established and tested*.

**What the evidence supports today:**

- The **tool manifest** was inspected and contains no tool that accepts or
  mutates an authority mode, and `crates/webcodex-cli` contains no
  `approve`/`deny`/mode command. **Qualified again in review round 20, CHECK 1:
  this inspection does not itself appear in `OSS_RESEARCH_EVIDENCE.md §2`.** It
  is a claim about a manifest that was read, reported here in prose, and never
  entered into the indexed evidence set — so under the rule this review has
  enforced since round 17 it cannot support `HOLDS_ALREADY` and cannot even
  support the `PARTIAL` I1 now carries without a reader being able to check it.
  The honest form is the one already in force: I1 is `PARTIAL`, the gap is named,
  and **entering the manifest inspection as a numbered observation is a cheap,
  named task** which this document now requires before the claim is relied on
  anywhere. Until then, treat the manifest check as *reported, not indexed*. **Restated in review round 20, CHECK 1.** The
  previous wording — "No MCP tool accepts or mutates an authority mode
  (**verified by inspection of the tool surface**)" — is a repository-wide absence
  claim, and a manifest inspection cannot establish it: tools registered outside
  the manifest path, an argument that mutates an authority-relevant field without
  naming itself after the mode, and code that writes a running process's
  environment are all outside what a manifest check observes. `SECURITY_INVARIANTS.md`
  I1 was demoted to `PARTIAL` for exactly this reason, and this bullet was the
  same claim in a second place. The honest form is the bounded observation above
  plus the unperformed survey I1 names. **One narrow consequence is genuinely
  available and is kept:** the *declared* tool surface exposes no mode-mutating
  tool, which is what P3 criterion 9 asserts as a manifest test.
- Authority mode is resolved from the process environment at startup
  (`permissions/policy.rs:64`), i.e. it is host-owned, not call-owned. This is a
  resolution-site observation and carries no claim about other sites.

**What is NOT established and must be built and tested:**

1. **Control-channel reachability.** The local approval/control channel must be
   proven unreachable from any model-driven operation. Concretely: it must not
   listen on a TCP port reachable from inside the sandbox, must not be a
   filesystem path inside the writable workspace, and must not be callable by a
   spawned child. **This enumeration covers the process route only, and the
   process route is not the whole surface (review round 18, CHECK 3).** A
   model-reachable **Browser/CDP** or **Computer/UI-automation** operation can
   synthesise input events against a host user-interface, and a host UI that
   renders a confirmation — or that spawns a terminal the profile does not
   govern — is a route to the control channel that no per-process sandbox rule
   denies. So the item is restated with an explicit second clause:
   **(1a)** the process-route unreachability above, proven by test; **and (1b)**
   for every model-reachable UI-interaction surface — the set is the finite,
   enumerable surface list this plan maintains (Browser/CDP, Computer/
   UI-automation, and additions recorded as they ship), **not** an open-ended
   "all host interaction" quantifier — a **named test** asserting
   that the surface cannot present, auto-answer, or synthesise input to a
   confirmation or mode-change affordance. Requirement (1b) has **no
   implementation today** and its status is `NOT_ENFORCED`. **Until round 30 this document said P3 does not pass on (1a) alone, which contradicted the plan's own P3 (7b') — and the contradiction was the blocking finding, because a phase gated on both "isolate the UI route" and "withhold the UI route" cannot be satisfied by any state of the world** (L62). Resolved by narrowing, not by wording: **P3's single release condition is (1a) plus withholding every host-UI surface from the approval affordance, and (1b) is `SPECIFIED` in phase P3-R, which no phase waits on.** **This document therefore no longer gates P3 on (1b), and still claims (1b) nowhere.** Candidate mechanisms: a host-owned process-local channel
   (inherited fd / unix socket at a path outside writable roots), a named pipe
   with an OS-level ACL, or a desktop IPC that the sandbox denies. **A test must
   assert that a sandboxed shell cannot reach it** (`SECURITY_TEST_VECTORS.md`
   MD-10, AG-*).
2. **Proof of a human decision.** A digest is *correlation*, not authorization.
   `hermes :: approval_transport.py:55-74` computes `sha256` over the request
   including `session_key`; that binds a reply to a request, but an attacker who
   can read the request can compute the digest. The local decision must therefore
   additionally carry something the model side cannot produce: a token minted on
   the host control plane and never exposed to the MCP surface, verified with a
   constant-time comparison, and single-use.
3. **Authority over the switch.** Who may change the mode is a configuration
   question, not a tool question. The mode is set only by an operator action on
   the host; there is no runtime API for it.

4. **Same-user identity boundary (raised as a remaining weakness in review round
   2; now a named requirement).** Excluding the channel from the *sandbox profile*
   is necessary but not sufficient: a model-spawned process runs as the **same
   host user** as the server, so any channel authenticated purely by
   same-user possession (a plain unix socket with default permissions, a file in
   `$HOME`, an inherited fd) is reachable by it. The design must therefore pick
   at least one real boundary and test it:

   | Candidate boundary | Property | Test |
   |---|---|---|
   | Sandbox profile denies the channel's path/socket | reachable-by-path is removed for sandboxed children | child cannot `connect()`/`open()` it (assert from inside a sandboxed shell) |
   | Peer-credential check on a unix socket (`SO_PEERCRED` / `LOCAL_PEERCRED`) + an allow-list of the *control-plane* pid | identity, not just same-user | a second same-uid process spawned by the agent is rejected |
   | One-shot token minted on the control plane, never present in the sandbox filesystem or environment, verified constant-time | forgery resistance | a child cannot read or guess it; replay is rejected |
   | Desktop/OS-mediated prompt (e.g. a native dialog owned by the host app) | out-of-band | a terminal-only attacker cannot answer it |

   A same-user channel guarded **only** by file permissions is explicitly
   insufficient and must not ship as the sole control. The chosen boundary and
   its test are P3 acceptance criteria (channel + identity), not P3 polish.

Rules (now stated as requirements with the above caveats):

- No MCP tool sets or clears the mode. This is verifiable today and must be
  asserted by a manifest test (vector MD-08).
- Enabling requires a locally minted single-use token that is never exposed to
  the MCP surface; it is **not** sufficient to require a locally-generated value.
- It is **not** persisted in project or session config; a process restart
  returns to the default mode.
- It never relaxes floors (I9).
- Every activation is audited with `decided_by`, timestamp, and reason.

```
OFF ──(local control plane: explicit operator action + single-use token)──▶ ON
 ▲                                                                          │
 └────────(logical session end · process exit · TTL · explicit revoke)────────┘
```

**Default on session end: restore the safe mode.** This must be enforced by the
runner/server lifecycle, not by a prompt instruction.

**"Session" is defined, because the state machine depends on it (review round 13,
CHECK 3).** Round 12 named the transition owner and round 13 found the boundary
still unworkable: keying the activation to the *runner process lifetime* and
calling that "the host session" leaves no answer for the ordinary case where an
individual logical session ends while the runner stays alive. Three distinct
lifetimes are therefore named:

| Lifetime | What ends it | What it clears |
|---|---|---|
| **logical session** | the client session that issued the activation ends, disconnects past a grace period, or the runner's session registry drops it | the danger-mode activation |
| **runner process** | process exit or restart | the activation, and the single-use token store with it |
| **TTL / explicit revoke** | operator-configured expiry, or a local revoke | the activation |

The activation is keyed to a **logical session id owned by the host** — an id the
host mints, the model never supplies, and the model cannot read or write. **That
such an id exists in WebCodex today is a hypothesis this design depends on, not an
established fact** (corrected in review round 15, CHECK 3: the earlier text said "the
logical session id the runner **already mints**", and no observation in
`OSS_RESEARCH_EVIDENCE.md §2` records one; the only session-keyed grants in the
evidence are Hermes' (`H-17` `approve_session(session_key, pattern_key)`, `H-21`
`clear_session(session_key)`), which is a *reusable pattern*, not a statement about
WebCodex's own session registry). The binding is therefore specified as a
**prerequisite, not a description**, and it has two possible states:

| State | Condition | Per-session expiry | MD-09 session-end leg |
|---|---|---|---|
| **bound** | the host demonstrably mints a logical session id for the activating request and can observe that session's end | enforced | passes |
| **unbound (default until demonstrated)** | no such id, or the host cannot observe the session's end | **`NOT_ENFORCED`** | `NOT_ENFORCED` |

The default is `unbound`. A design that assumed a session id existed and then found
one did not would silently degrade to process-lifetime scoping — exactly the
substitution round 13 rejected — so the fallback is stated rather than assumed away:
when unbound, the activation expires on **TTL only**, bounded by the operator, and
the host must say so rather than claim per-session expiry it does not implement. The
pre-existing substrate for the `bound` row is **weaker than round 15 claimed, and is demoted
here accordingly (review round 16, CHECK 3)**: `W-42` records only a list of *error-kind
strings* (`session_guard_denied`, `unknown_session_id`, `session_project_mismatch`) emitted by a
classifier. That is evidence the code **distinguishes** those cases, not evidence that a session
registry **exists**, and a classifier can name a condition no component currently produces. So
the submission establishes neither a registry, nor a session/project association, nor end-of-life
observation — which is the last thing the `bound` row needs. `bound` is therefore a **P5 work item
with a named deliverable** (demonstrate a host-minted logical session id for the activating
request, and an observable end-of-life signal for it), not a formality, and while it is unmet the
`unbound` row governs: `DANGER_FULL_ACCESS` is **not offered** and resolves to `DENY`, with
MD-09's session-end leg `NOT_ENFORCED`.

**Transition owner (named in review round 12).** The owner of every arrow in the
state machine above is the **runner/server lifecycle**. It holds the activation as
in-process state keyed to the runner's own logical session lifetime, and it clears that
state at logical session end, process exit, TTL, or explicit local revoke (the three
lifetimes are tabulated immediately above). **Only the `bound` state above delivers
the logical-session row; in the default `unbound` state the runner/server lifecycle
can clear the activation on process exit, TTL, or local revoke, and the
logical-session row is `NOT_ENFORCED`.** This resolves
what was previously an unsatisfiable pair of requirements: `IMPLEMENTATION_PLAN.md`
P5 criterion 1 asserted that mode is "never" resolved from session state, while
this section requires a per-session activation that expires. The reconciliation is
that **"session state" in P5 means model-reachable or MCP-writable session state,
which is excluded absolutely; the host's own in-process activation is host-owned
state and is the mechanism this section requires.** P5 criterion 1 was rewritten in
round 12 to say so, and the two documents now name the same owner. No MCP tool sets
or clears the mode in either reading (MD-08).

---

## 11. Hermes integration

Hermes is a child agent launched through the runner. It must inherit the
sandbox exactly like any other child.

```
RunnerOperation::CodingAgent / mcp_gateway / plugin
        │
        ▼
spawn site ── sandbox::apply(&mut Command, &SandboxPlan)  ◀── applied here
        │        (managed path OR the shared hook at a direct-spawn site)
        ▼
hermes … (child)
```

- Hermes inherits the parent's sandbox because the sandbox is established
  **pre-`exec`** and descendants inherit the *facility*. It does **not** inherit
  confinement merely by joining the process group — the process group provides
  tree ownership for signal delivery/termination, not confinement (§4).
- **Two different escapes, and only one of them is about confinement.**
  (Corrected in review round 3, which correctly rejected an unsupported
  "new session" claim.)
  - `setsid()` / `process_group()` / Job-Object breakaway escapes the **process
    group**. That defeats *tree ownership*: the runner may no longer be able to
    signal or reap the descendant. It does **not** by itself remove an applied
    sandbox profile — a Seatbelt policy is on the process and is inherited, and
    it cannot be un-applied from inside.
  - Escaping the **facility** is a different matter and is **facility-specific**.
    Candidate surfaces: creating an unprivileged user namespace (Linux), invoking
    a helper that is itself unsandboxed, or a platform service that performs the
    operation on the child's behalf. Each must be enumerated **per backend** and
    tested (`SECURITY_TEST_VECTORS.md` EX-13/EX-14), rather than assumed either
    way. This document does not assert that a new session escapes confinement.
- **The sandbox path must actually be reached.** On Unix, the agent/job spawn
  sites include direct `Command::spawn()` callers (§1, `OSS_RESEARCH_EVIDENCE.md`
  §2.1a). Until P1a normalizes them, "Hermes is sandboxed" would be false on
  macOS.
- **Hermes' own approval system is upstream of, and independent from, ours.**
  Hermes has `manual | smart | off`
  (`hermes @ 79dbb145 :: hermes_cli/approval_mode.py:19`) and its own floors that
  survive `--yolo` and `mode=off` (`tools/approval_floors.py`). Two independent
  approval systems chained is a real hazard: `off` on Hermes must **not** be
  reachable from our MCP surface; it is Hermes profile config, changed locally.
- Nesting rule: our mode may only *narrow* what Hermes may do. Running Hermes
  with `mode=off` under our `approve_for_me` would silently discard a control —
  so our runner must pin Hermes' mode, and a pinned mismatch is a hard DENY.
- Known limitation from the baseline: interactive `stdin`/resume control is
  incomplete (`HERMES_RESUME_MODEL = PARTIAL`) and the official path is ACP
  (`docs/agent/acp-coding-agent-run.md`), not shell nesting.

## 12. Codex integration

Two distinct roles — do not conflate them:

**(a) Codex as a sandbox reference** — §6. Design-only; no code is vendored
(`COMPONENT_REUSE_MATRIX.md §2`).

**(b) Codex as a child agent** — spawned through the runner like any other
child, on the ACP path (`docs/agent/acp-coding-agent-run.md`), not by shell
nesting. The baseline recorded double-sandbox interlock and 47.5k tokens burned
with no useful output when Codex was nested over `run_shell`; the ACP adapter is
the supported channel.

**Double-sandbox rule:** when a nested agent brings its own sandbox, the effective
confinement is the *intersection*. The runner must therefore:

1. pass an explicit, narrower-than-default permission profile to the child agent,
2. never pass `danger-full-access` to a nested agent, and
3. treat a child that reports full access under a restricted parent as a
   **violation**, not a success.

## 13. Prompt-injection attack walkthrough

Threat: the model reads a malicious file (README, issue text, dependency source,
tool output) instructing it to exfiltrate.

| Step | Attacker wants | What stops it | Status today |
|---|---|---|---|
| 1 | Get the injected text into context | nothing (by design — content inspection is **not** a boundary, I6). The parenthetical "content is returned verbatim" is **withdrawn** (review round 18, CHECK 7): I6 was demoted to `UNVERIFIED` because no observation establishes whether a transformation is applied to file content on the way back, and a demoted claim does not survive in a weaker form elsewhere. The step that *is* established is the negative one the design actually needs: **no capability increase and no inspection-as-control** | accepted (by design), with the verbatim sub-claim `UNVERIFIED` |
| 2 | Read `~/.ssh/id_rsa` via a structured tool | path policy + secret path ⇒ hard DENY | PARTIAL (prose matching; I7) |
| 3 | Read it via shell (`cat`, `python`, `node`) | **OS sandbox**: credential dirs outside writable/readable roots | **ABSENT — this is the open hole** |
| 4 | Re-spell the escape (`../`, symlink, absolute, subshell) | OS-level enforcement, not command parsing | **ABSENT** (I14) |
| 5 | Exfiltrate over the network | `network.outbound` ALLOW/ASK/DENY, independent of FS | **ABSENT** (I13) |
| 6 | Ask the harness to approve itself | no boolean approval arg on the **declared** tool surface; approval requires a local decision + digest | **`PARTIAL` (I2)** — round 20, CHECK 1. This cell previously read `HOLDS (I2)`, which asserted a clean present-tense protection while I2 had been demoted in round 18. The step that *is* established is the manifest observation; the universal "no approval-like argument anywhere" is the part I2 records as unperformed. **`PARTIAL`, not `HOLDS`** |
| 7 | Turn on danger mode | no MCP tool can change mode | ABSENT→designed (I3) |
| 8 | Spawn a subagent that is less restricted | child inherits sandbox; our mode may only narrow | ABSENT (I5) |
| 9 | Persuade the reviewer | reviewer sees the action, not just prose; transcript is untrusted data; floors precede reviewer | designed (I9) |

**Conclusion:** every step from 3–5, and 8, is currently unbounded. Steps 1, 2,
6 are already correct or partially correct. Closing steps 3–8 requires **five**
distinct pieces of new work, not two:

| # | Piece | Closes |
|---|---|---|
| 1 | Spawn normalization — **a two-part attach surface, not one path** (corrected in review round 14, CHECK 4: "route all model-reachable spawns through one path" contradicted the permitted design, in which the managed path and the shared sandbox-attach hook are *two* sites and normalizing to a single function is not required): the `ManagedChild::spawn*` path **and** the shared `sandbox::apply` hook, so that the spawns that pass through those two sites reach confinement without all spawns sharing one call site. **This row does not claim whole-tree confinement, and must not be read as claiming it** (review round 21, CHECK 11): closing every enumerated model-reachable spawn surface is prerequisite 1's *input*, and the enumeration itself remains open — `SECURITY_INVARIANTS.md` I7 is `PARTIAL` because a call-graph edge set cannot be proven complete for a raw `fork`/`posix_spawn`, and I5 is `PARTIAL` because a `bash -c` grandchild is not itself mediated. **The residual is `UNKNOWN`, not `PARTIAL`**: no claim is made here about spawns that bypass both sites, and the plan states that the enumerated set is a lower bound (`IMPLEMENTATION_PLAN.md` P1a scope note). The correct statement is that this work makes the *enumerated* surface confined; whether that surface is *all* of the model-reachable surface is a separate open question that this row does not answer | prerequisite for 2 |
| 2 | Sandbox backend + profile compiler | steps 3, 4, 5, 8 |
| 3 | Capability model + policy rules | step 5 scoping |
| 4 | Approval engine **and a local control channel** (mint, present, verify) | steps 3–5 when `ASK` |
| 5 | Auto-reviewer **and child-agent integration/pinning** | step 8, `APPROVE_FOR_ME` |

An earlier revision described this as "exactly two new mechanisms". That
understated the work — the local control channel, the grant store, the reviewer,
and child-agent integration are each independently required. The count is
corrected here and the plan in `IMPLEMENTATION_PLAN.md` reflects all five.

---

## 14. Failure behavior

| Failure | Required behavior | Rationale |
|---|---|---|
| Sandbox backend unavailable (no `bwrap`, unsupported kernel) | **DENY** the capability that needs it; do not silently run unsanded | I18 |
| SBPL profile generation error | **DENY** | I18 |
| Invalid/unknown mode config | **DENY** consequential tools (already implemented) | `policy.rs:152-165` |
| Approval timeout | **DENY** | I10 |
| Transport exception | **DENY** (`error`) | I10 |
| Transport capacity exhausted | **DENY** (`busy`) | I10 |
| Late/mismatched decision | **DENY** (`stale`) | I11 |
| Reviewer timeout / error / malformed output | **DENY** | I9 |
| Reviewer says ALLOW after a floor fired | **DENY** | I9 |
| Plugin load failure / hook throws | **narrow** (block), never widen | I17 |
| Runner loses sandbox attestation | **DENY** further consequential tools this session | I4 |
| Session ends / process exits | revoke session grants; restore default mode | I11, I3 |
| Transport interrupted during wait | **DENY** (`interrupted`) | I10 |

Every row above must exist as a test vector (`SECURITY_TEST_VECTORS.md` §6).

---

## 15. Platform differences

| Concern | macOS | Linux | Windows |
|---|---|---|---|
| Sandbox facility | Seatbelt via `/usr/bin/sandbox-exec` + SBPL | `bwrap` (preferred) or Landlock + seccomp launcher | Job Object only (no filesystem/network confinement) |
| Reuse type | `SUBPROCESS_ADAPTER` + small SBPL compiler | `SUBPROCESS_ADAPTER` (`bwrap`); `REIMPLEMENT_SMALL_CORE` (Landlock fallback) | `REUSE_EXISTING` (Job Object for tree lifetime) |
| Network confinement | SBPL rules (localhost / DNS :53 / open) | network namespace in `bwrap` / Landlock network rules | **not available** |
| Process-tree kill | process group (`process_group(0)`, `kill(-pgid)`) | same | Job Object `KILL_ON_JOB_CLOSE` |
| Known caveats | `sandbox-exec` is Apple-deprecated; a wrong SBPL rule fails open silently → needs negative tests | Landlock needs a recent kernel; `bwrap` may be absent (then DENY); WSL1 unsupported | **No FS/network confinement** ⇒ treat Windows as `READ_ONLY`-equivalent until a real backend exists |
| Fallback if backend missing | DENY | DENY | DENY (capability-gated) |

**Platform posture statement to be published with the feature:** the
security claim is only as strong as the strongest available backend on the host
platform, and the server must report which backend is active (and refuse
consequential capabilities when none is).

---

## 16. Deviation log from the brief's candidate architecture

| Brief's element | Verdict | Reason (evidence) |
|---|---|---|
| Policy Engine (OpenCode-style) | **Adopted** | `opencode :: packages/core/src/permission.ts` — allow/deny/ask + rule triples + once/always/reject |
| Approval Engine (Hermes-style) | **Adopted** | `hermes :: hermes_cli/approval_transport.py` — digest-bound, fail-closed |
| Sandbox Engine (Codex reuse) | **Adopted with change** | Reuse is `SUBPROCESS_ADAPTER`, not a dependency; applied at the sandbox attach surface (managed path + shared hook) below the Execution Broker, because there is no single spawn function |
| Approval over MCP elicitation | **Not available** | WebCodex advertises only `tools`; architecture decision forbids depending on elicitation |
| Actor/Reviewer separation | **Adopted** | `codex :: guardian/decision.rs`, `ext/guardian-reviewer/` |
| Plugins may weaken policy | **Rejected** | WebCodex has no such rule; the restrict-only contract is a **requirement we impose** (`SECURITY_INVARIANTS.md` I17), not a property proven by Pi or Codex. Pi's hook does block on error and blocks by default in non-interactive mode, but its hook receives **mutable** `args` and can override results, so "restrict-only" is not an inherited guarantee |
| Single unified execution entry | **Already true for operations, false for spawning** | `RunnerOperation` is a single operation enum, but process creation has **no** single chokepoint — ≥10 direct `Command::spawn()` sites (P1a normalizes them) |
