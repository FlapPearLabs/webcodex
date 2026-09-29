# SECURITY_INVARIANTS.md

Security invariants for making WebCodex a safe, low-friction, Codex-style local
coding agent for ChatGPT Web.

Scope: research/design stage. No production code is changed by this branch.
Status of each invariant is one of:

- `HOLDS_ALREADY` — verified present in current WebCodex source. **A claim of this kind requires a named observation in `OSS_RESEARCH_EVIDENCE.md §2`; an audit that did not look for a property is not evidence that the property is absent, and "by omission" is not a verification (I6, corrected in round 17).**
- `PARTIAL` — partially present; gap identified. **Used where a bounded
  observation supports part of the property and the rest rests on a survey that
  was never performed** (I1, I2, I7, I18, and the pre-existing partials).
- `UNVERIFIED` — **no observation speaks to the property in either direction.**
  This is distinct from `PARTIAL`: `PARTIAL` means something was observed and
  something else was not, whereas `UNVERIFIED` means the audit never looked.
  I6 is the only current instance. **Added in review round 20, following a
  round-19 finding that the file used a status it had not declared** — the value
  was intelligible in context but outside the declared vocabulary, which is the
  same class of defect as a claim with no named observation behind it: a reader
  cannot check a status whose meaning is not written down. The distinction
  matters because the two call for different work: `PARTIAL` needs a gap closed,
  `UNVERIFIED` needs a search run.
- `ABSENT` — not present; must be built.

Evidence convention: `SOURCE = repo @ commit :: file:line :: symbol`.
See `OSS_RESEARCH_EVIDENCE.md` for the full evidence ledger.

Baseline: `yyjeqhc/webcodex @ 7301186b98527cb4ebc0191f6033474e9abf7c20`.

---

## 0. Design axioms

- **A1. Model output is never a security boundary.** Only local, host-owned code
  and OS facilities may grant or deny capability.
- **A2. Two layers, never one.** Policy (ALLOW/ASK/DENY) and OS sandbox are
  separate layers with separate failure modes. Conflating them is the single
  most common defect in this class of system.
- **A3. Fail closed.** Every ambiguous, failed, timed-out, or unparseable path
  resolves to DENY or ASK — never ALLOW.
- **A4. Minimal sufficient capability set.** Capabilities are added only when a
  concrete code path requires them; not for taxonomy completeness.

---

## 1. Invariant ledger

### I1. The model cannot grant itself higher privilege
`PARTIAL (review round 18, CHECK 1 — was recorded as `HOLDS_ALREADY (policy layer)`)`

Authority mode is resolved from the process environment, not from tool
arguments:
`SOURCE = webcodex @ 7301186b :: src/tool_runtime/permissions/policy.rs:64 ::
EffectiveAuthorityConfig::from_env`.

That single observation establishes a **resolution site**, which is a real and
useful control: mode is host-owned rather than call-owned, so a tool argument
cannot be the thing that selects it.

**What was asserted beyond the evidence, and is withdrawn (round 18, CHECK 1).**
The previous text said "There is **no** MCP tool, and **no** tool argument, that
selects or mutates the authority mode." That is a **repository-wide absence
claim**, and nothing in `OSS_RESEARCH_EVIDENCE.md §2` establishes it. What the
evidence offers is two bounded observations: the resolution site above, and a
check of the **tool manifest** for any tool that takes or changes a mode
(quoted in `REFERENCE_ARCHITECTURE.md §10`). A manifest check covers **what the
tool surface declares**; it does not cover (a) tools registered outside the
manifest path, (b) a tool argument that mutates an authority-relevant field
without naming itself after the mode, or (c) a code path that writes the process
environment of a running child or of the runner itself. None of those was
searched. Under round 17's rule — *an unperformed check is not a negative
result* — the honest form is a scoped observation plus an unperformed check, not
a global negative. Establishing it is a **named, cheap task**: enumerate the
registered tool set at runtime, and grep for writes to authority-relevant config
outside the resolution site; until that is done the claim stays `PARTIAL`.

`github`-grade control, restated at the strength the evidence supports: mode is
an operator-owned env var **at the resolution site**.

**Residual gap:** privilege is not technically bounded today. Under the default
mode there is no OS-level ceiling, so "cannot self-escalate" holds only for the
policy label, not for actual capability. Resolved by I4/I5.

### I2. `approved=true`-style tool arguments are not human approval
`PARTIAL (review round 18, CHECK 1 — was recorded as `HOLDS_ALREADY`)`

The **named** observations below establish that a server-side permission decision
is produced before dispatch and attached to the result, and that
`confirmation_required` is a hard-deny signal rather than a grant. What the
previous text additionally claimed — that approval state is "not read from tool
arguments **anywhere**" — is again a repository-wide absence, and no
comprehensive argument schema was enumerated to support it. The two shipped
vectors `PI-08` (`approved=true`) and the manifest assertion in P3 criterion 9
bound the *declared* surface; they do not bound every field of every tool
argument schema. Scoped claim, unperformed check, `PARTIAL`.

The permission decision is produced server-side before dispatch and attached to the result:
`SOURCE = webcodex @ 7301186b :: src/tool_runtime/dispatch.rs:1886 ::
evaluate_permission_for_tool` and
`:: src/tool_runtime/permissions/mod.rs:66 :: add_permission_to_result`.

`confirmation_required` exists but is a *hard-deny* signal, not a grant:
`SOURCE = webcodex @ 7301186b :: src/tool_runtime/permissions/mod.rs:124 ::
is_hard_denied_output`.

Hermes reinforces the pattern: a transport reply must echo a
`request_id` **and** a SHA-256 `request_digest` computed over a canonical form
that includes `session_key`; a mismatched reply is rejected as `stale`:
`SOURCE = NousResearch/hermes-agent @ 79dbb145 :: hermes_cli/approval_transport.py:167 ::
_validate_decision`.

**Requirement:** any future approval field must be a *correlation token*
(id + digest), never a boolean.

### I3. Danger mode cannot be enabled by the remote agent
`ABSENT`

No danger/full-access mode exists in WebCodex today. This is a design
requirement for the mode we add:

- The mode transition must be performed on a **local trusted control plane**
  (CLI on the host / local UI), never through the MCP tool surface.
- The MCP surface must contain **no tool** whose effect is to raise authority.
- Transition must require a locally generated, single-use token.

Reference shape: Codex expresses the same idea as a *permission profile*
(`:read-only` / `:workspace` / `:danger-full-access`) that is part of the host
config, not a tool argument:
`SOURCE = openai/codex @ 69f71405 :: codex-rs/protocol/src/models.rs:410-436 ::
BUILT_IN_PERMISSION_PROFILE_* / PermissionProfile`.

### I4. The workspace sandbox must cover the whole process tree
`ABSENT (WebCodex) / PARTIAL (Codex reference)` — **the Codex-reference half of this status was downgraded in round 27, and the reason is the distinction the row below it already draws: `transform` establishes that an ATTACH MECHANISM exists at a spawn point, which is not the same property as covering every descendant and every escape route.** `HOLDS_ALREADY` claims the whole-tree coverage that the invariant's own title demands; a mechanism that attaches the sandbox is evidence for the mechanism, not for the coverage. Which spawn surfaces attach it, and whether a descendant can escape the resulting confinement by re-execing a non-attached binary, are **not established by the cited line** and are not established anywhere else in `§2`. (Review round 27, CHECK 1, blocking.)

WebCodex has process-**tree ownership** but no sandbox:
`SOURCE = webcodex @ 7301186b :: crates/webcodex-process/src/unix.rs:43-64 ::
ManagedChild::spawn` (child becomes leader of a private process group;
`kill(-pgid, SIGKILL)` for the whole group).

Codex shows the correct combination — sandbox **and** tree ownership applied at
the same spawn point:
`SOURCE = openai/codex @ 69f71405 :: codex-rs/sandboxing/src/manager.rs:352 ::
SandboxManager::transform` (rewrites argv to prepend the platform sandbox
launcher before spawn).

**Requirement:** the sandbox must be applied to **every** spawn surface, and it
must cover the whole descendant tree. The mechanism is not the process group —
these are two independent properties:

| Property | Mechanism | Provides |
|---|---|---|
| Tree **ownership** | process group (`process_group(0)`) / Windows Job Object | signalling the whole tree as a group; **best-effort** termination. It is **not guaranteed**: a descendant that leaves the group (`setsid()`, `process_group()`, Job-Object breakaway) is no longer reachable by `kill(-pgid)`. WebCodex's own `remote_shell`/`ssh`/`detached_job` paths already spawn outside `ManagedChild` on Unix, so this is a live property of the current code, not a hypothetical |
| Tree **confinement** | sandbox facility entered **before `exec`** | descendants created under the facility remain confined |

`ManagedChild` supplies the first (best-effort) and currently supplies nothing
for the second. Confinement does **not** follow from process-group membership,
and this document does not claim that membership guarantees termination either.

There is also **no single spawn surface** to attach to: see I5 and
`OSS_RESEARCH_EVIDENCE.md §2.1a`. The requirement is therefore "every reachable
spawn carries a plan", not "attach at one function".

Codex shows the confinement half done correctly — the platform launcher is
prepended to argv, i.e. the facility is entered at `exec`:
`SOURCE = openai/codex @ 69f71405 :: codex-rs/sandboxing/src/manager.rs:352 ::
SandboxManager::transform`.

### I5. Python / Node / Bash / Hermes / Codex subprocesses must not bypass the sandbox
`ABSENT`

**This is the highest-severity gap and it is confirmed empirical.** The prior
evaluation recorded `cd /tmp`, `ls ~`, and `curl https://example.com` all
succeeding under `run_shell` with no approval
(`webcodex @ 7301186b`; baseline: `WEBCODEX_EVALUATION.md` §SECURITY_MODEL B/C).

Structurally the fix is tractable in principle but the assumed shortcut does not
exist. `ManagedChild::spawn*` is a *common* path (~20 non-test sites across 9
crates/binaries), but **there is no single chokepoint**: ≥ 10 non-test sites
call `Command::spawn()` directly, several model-reachable on Unix — including
`remote_shell.rs:109`, `ssh.rs:1142`, `detached_job.rs:1924/1941/2409`,
`persistent-shell/lib.rs:1965`, and the server-side
`src/tool_runtime/helpers.rs:146`. There is also a platform asymmetry:
`remote_shell`, `ssh`, and `persistent_shell` use the managed path on **Windows
only**, so macOS — the primary target — takes the direct path. Full inventory:
`OSS_RESEARCH_EVIDENCE.md §2.1a`; the prerequisite refactor is
`IMPLEMENTATION_PLAN.md` P1a.

> **Correction (adversarial review rounds 1–2, CHECK 4).** An earlier revision of
> this invariant claimed "17 production call sites and zero direct spawns" and a
> "single chokepoint". Both parts were wrong and are withdrawn.

**Requirement:** because the sandbox is established pre-`exec` and descendants
inherit the *facility* (not the process group — see I4), the guarantee must be
stated as *"inherited by the whole tree unless the child escapes the sandbox
facility itself"*, and the platform-escape surface must be part of the test
vectors (`SECURITY_TEST_VECTORS.md` EX-13/EX-14). It must also be stated plainly
that this guarantee is currently **not delivered on any surface**, because no
sandbox exists and because no single attach point exists yet.

### I6. Prompt-injection detection is not a security boundary
`UNVERIFIED` (review round 17, newly-raised — was recorded as `HOLDS_ALREADY`)

**The requirement is settled; the "already holds" claim is not.** Content inspection
of tool results is a model-adjacent heuristic and therefore not a boundary, so **do
not add "injection detection" as a control** — add capability limits instead. That
much does not depend on any observation.

What *was* asserted, and withdrawn: "WebCodex performs no content-level inspection of
tool results and returns file content verbatim." **Neither claim follows from the
submitted raw fact list.** `OSS_RESEARCH_EVIDENCE.md §2` records the permission
classifier, the tool-dispatch boundary, the session-guard error kinds and the spawn
inventory; it records **no observation of any content-inspection code path in either
direction**, and an absence of evidence in the audit is not evidence of absence in
the tree. The same applies to "verbatim" — no observation establishes that a
transformation is or is not applied to file content on the way back. Marking this
`HOLDS_ALREADY (by omission, correctly)` asserted a verified property on the strength
of a search that was never run, which is the same defect class as round 15's
"the runner already mints" and round 16's reading of `W-42`: **an unperformed check
is not a negative result.**

Establishing it is a cheap, named task — grep the tool-result path for any
transform, redaction, or scanning stage, and record the result — and until it is
done the correct status is `UNVERIFIED` rather than either `HOLDS_ALREADY` or a
claimed gap. Note the direction of the risk: if content inspection *does* exist
somewhere, it is a model-adjacent heuristic sitting where this document says no
boundary is, which is worth knowing for a different reason than the one this
invariant was written for.

**Requirement:** do not add "injection detection" as a control. Add capability
limits instead.

### I7. Reading a malicious README must not increase technical permission
`PARTIAL`

Path/sensitive-path policy rejects absolute and `..` probes on the structured
read/write surface. The classification surface is:
`SOURCE = webcodex @ 7301186b :: src/tool_runtime/permissions/mod.rs:124 ::
is_hard_denied_output` (classifies `sensitive path`, `path must be
project-relative`, `path cannot contain parent traversal`, `absolute paths are
not allowed`, `path traversal` as hard denials).

**Gap 1 — it is a classifier over a tool's own error string, applied after the
fact** (see `REFERENCE_ARCHITECTURE.md §5.1`; the filter runs at
`dispatch.rs:2006`, post-execution). It reports that a check fired; it is not
itself the check. The actual path checks live inside individual tools, which is
why the shell surface has no equivalent.

**Gap 2 — hard denial is decided by substring matching over prose.** A
message-wording change silently disables the classification. This must be
upgraded to a **structured** error kind, not prose.

**Gap 3:** the shell path is not covered by any of this (see I5).

### I8. ALLOW/ASK/DENY policy and OS sandbox must be separate layers
`PARTIAL`

WebCodex already keeps policy out of the sandbox slot — but only because it has
no sandbox at all. The policy layer is explicitly documented as a *decision
layer above hard safety*:
`SOURCE = webcodex @ 7301186b :: docs/agent/permission-model.md §3, §6`.

**Requirement:** keep `PolicyEngine` and `SandboxBackend` as separate modules
with separate tests, and assert in tests that changing the policy outcome does
not change the sandbox profile and vice versa.

OpenCode is a pure policy engine with no OS sandbox
(`SOURCE = anomalyco/opencode @ 3c893f0a :: packages/core/src/permission.ts`);
Codex has both. Do not copy OpenCode's engine and assume it provides confinement.

### I9. Auto-reviewer cannot override a hard DENY
`HOLDS_ALREADY (as a result classifier) / must be built as a pre-effect floor`

> **Clarification (adversarial review round 1, CHECK 2).** WebCodex's existing
> hard-deny check runs **after** execution and only decides whether to attach
> soft authority metadata to the finished result
> (`SOURCE = webcodex @ 7301186b :: src/tool_runtime/dispatch.rs:2006`, where
> `is_hard_denied_output` filters the permission attach, and
> `:: src/tool_runtime/permissions/mod.rs:124` which defines the classifier).
> It does **not** prevent an effect. So "hard DENY cannot be overridden" is true
> of the *classification and metadata* today, and must become true of
> *enforcement* once a central pre-effect floor exists.

WebCodex: hard-deny classification is explicitly independent of authority mode,
and hard-denied output suppresses the soft authority attach —
`SOURCE = webcodex @ 7301186b :: src/tool_runtime/permissions/mod.rs:122-154`;
`docs/agent/permission-model.md` invariant 6.

Hermes implements the same idea as "floors" that run *before* yolo / mode=off:
`SOURCE = NousResearch/hermes-agent @ 79dbb145 :: tools/approval_floors.py:1-8,23-51 ::
_match_user_deny_rule` ("not even with --yolo, /yolo, or approvals.mode=off").

Codex: `ReviewDecision::default()` is `Denied`, and the reviewer returning
`None` never means allow:
`SOURCE = openai/codex @ 69f71405 :: codex-rs/core/src/guardian/decision.rs:44`.

**Requirement:** the DENY floor is evaluated in the **pre-effect** stage (before
any mutation), the reviewer is consulted only after it passes, and a reviewer
verdict of "allow" must not resurrect a floor-denied action.

### I10. Approval timeout / transport failure fails closed
`ABSENT (no approval exists) / reference-implemented (Hermes)`

Hermes normalizes every failure mode to `deny`: `busy`, `error`, `timeout`,
`interrupted`, `invalid`, `stale`:
`SOURCE = NousResearch/hermes-agent @ 79dbb145 :: hermes_cli/approval_transport.py:99-185`.

Codex models timeout as a first-class non-allow outcome:
`ReviewDecision::TimedOut`
(`SOURCE = openai/codex @ 69f71405 :: codex-rs/protocol/src/protocol.rs:4159`).

**Requirement:** adopt the Hermes failure taxonomy verbatim; every code path
returns DENY.

### I11. Session grants bind to session + project + rule scope
`ABSENT (no grants) / reference-implemented (all three)`

- Codex: `ApprovedForSession` (session-scoped) vs
  `ApprovedExecpolicyAmendment` (pattern) vs `ApprovedMcpPolicyAmendment`
  (cross-session) — three distinct grant lifetimes.
- OpenCode: `Reply = once | always | reject`; `always` persists only the
  `request.save[]` resource list, keyed by `projectID` and `action`
  (`SOURCE = anomalyco/opencode @ 3c893f0a :: packages/core/src/permission.ts:250-256`,
  `:: packages/core/src/permission/saved.ts`).
- Hermes: grants are `(session_key, pattern_key)` pairs; `always` writes an
  explicit allowlist file.

**Requirement:** a grant is a tuple `(project, action, resource-pattern,
lifetime)`; there is no global "allow everything".

### I12. Approval records are auditable
`PARTIAL`

Decision records exist with a stable wire shape and a per-decision id:
`SOURCE = webcodex @ 7301186b :: crates/webcodex-core/src/workflow_session_contract.rs:538 ::
PermissionDecision { required, policy, request_id, status, reason, risk,
tool_name, project }` and `:: src/tool_runtime/permissions/mod.rs:156 ::
permission_summary_from_events`.

**Gap:** `PermissionOutcome` already enumerates `Approved` and `Pending`
(`"requested"`) — the data model anticipates an approval flow that the runtime
does not implement. Do not treat the enum's existence as evidence of an
approval queue.

**Requirement:** approval decisions add `decided_by` (local-control-plane
identity), `decided_at`, `grant_lifetime`, `request_digest`. Never log tool
parameters, file contents, or secrets. WebCodex already forbids these:
`SOURCE = webcodex @ 7301186b :: docs/agent/permission-model.md §7`.

### I13. Network permission is independent of filesystem permission
`ABSENT (no enforcement) / reference-implemented (Codex)`

Codex models network as its own axis inside the profile and emits distinct SBPL
rules:
`SOURCE = openai/codex @ 69f71405 :: codex-rs/protocol/src/models.rs:422-436 ::
PermissionProfile::Managed { file_system, network }`, and
`:: codex-rs/sandboxing/src/seatbelt.rs:336-346` (localhost-only vs DNS :53 vs
open outbound).

**Requirement:** `network.outbound` is its own capability with its own ALLOW/ASK/
DENY outcome. A filesystem-allow must never imply network-allow.

### I14. Workspace escape must not depend on shell spelling
`ABSENT`

Confirmed empirically: `cd /tmp && cat <outside>` and `ls ~` were allowed under
the default mode. Any control that parses the *command string* is defeated by
re-spelling. Enforcement must be at the OS level (I4/I5), with the policy engine
acting only as a friction/UX layer.

**Requirement:** no security decision may depend on shell-text parsing.
Text rules may only *raise* friction (ASK) on top of an OS boundary.

### I15. symlink / path traversal / realpath are in the threat model
`PARTIAL`

> **Correction (adversarial review round 1, CHECK 6).** An earlier revision
> asserted that WebCodex "canonicalizes project roots and rejects symlinks
> resolving outside allowed roots", citing
> `crates/webcodex-cli/src/webcodex_cli/project.rs`. That claim came from a
> *separate* baseline document that is **not** part of this branch's submitted
> evidence, and it was **not re-verified here**. It is downgraded to
> `UNVERIFIED_IN_THIS_RESEARCH`. The `source-level subject only` label is
> retained for the `is_secret_path` finding, which *was* read directly.

**What is verified in this branch:** the hard-deny classifier treats path
failures as denials (`permissions/mod.rs:124`), but by **prose match** on the
error string — so it is a classifier over a tool's own message, not a path
resolver of its own (see I7).

**Surface-specific requirement (corrected).** Path rules are not
one-size-fits-all. The design must state them per surface:

| Surface | Rule |
|---|---|
| Structured file tools (`read_files`, `apply_text_edits`, …) | reject absolute paths and `..`; resolve then contain within the project root; reject symlinks that resolve outside |
| Shell / process / script / job | **not** enforced by path parsing. Confined by the OS sandbox only (I4/I5). A path rule here is at most a friction (`ASK`) signal |
| Agent filesystem operations (ACP child tools) | enforced by the child's sandbox plan, not by inspecting its arguments |
| MCP / plugin gateways, LSP, browser | each declares whether it can express a path/network descriptor; if it cannot, it is `deny` |

Hermes' helper is the cleanest reference for the *resolver* half (used by its
structured surfaces, not its shell):
`SOURCE = NousResearch/hermes-agent @ 79dbb145 :: tools/path_security.py:8 ::
validate_within_dir` (resolve-then-`relative_to`, plus a cheap literal `..`
pre-check and a control-character check).

**Gap:** OpenCode's matcher is a **string** glob with no canonicalization
(`SOURCE = anomalyco/opencode @ 3c893f0a :: packages/core/src/util/wildcard.ts`).
Do **not** adopt it as the path guard; it is safe only as a policy layer behind a
real path resolver.

### I16. Secrets do not become readable merely because the project has shell access
`PARTIAL`

`is_secret_path` is cited from read/skill/diff/patch surfaces **as a
source-level finding from an earlier baseline pass, not as a named observation in
`OSS_RESEARCH_EVIDENCE.md §2`. Corrected in review round 20, from a round-19
finding: the symbol does not appear anywhere in the submitted evidence set, so it
cannot be cross-checked here, and under the rule this review has enforced since
round 17 — `HOLDS_ALREADY` requires a named observation — an uncitable finding
cannot support a status. I16 is therefore `PARTIAL` on the strength of its *gap*
statement (the shell surface is unbounded), which needs no citation, and **not**
on the strength of the symbol's existence.** Recording the citation gap is the
honest form; adding the observation is a cheap, named task and until it is done no
document may describe `is_secret_path` coverage as established — including
`COMPONENT_REUSE_MATRIX.md` row 13, which already says "coverage unverified in
baseline" and is consistent with this.

**Gap:** the shell surface is unbounded, so the guarantee currently reduces to
"the model does not try". With an OS sandbox and an explicit `secret.read`
capability that is DENY-by-default, this becomes structural.

**Requirement:** credential directories resolve OUTSIDE the writable roots and
the sandbox profile must not include them in readable paths.

### I17. Plugins may add restrictions but cannot silently weaken core policy
`PARTIAL`

> **Correction (adversarial review round 1, CHECK 8).** An earlier revision cited
> Pi and Codex as proving a "restrict-only" extension property. The cited
> observations do **not** prove that:
> - Pi's hook context includes the **mutable** `args` (`types.ts:103`), and its
>   `afterToolCall` can **replace** result content, details, and `isError`
>   (`types.ts:89`). A hook that rewrites arguments can widen what is executed;
>   a hook that rewrites results can hide an error.
> - Codex's restriction comes from the *host* refusing to honor a contributor
>   verdict outside delegation limits, not from the extension API being
>   incapable of asking.
>
> What the evidence *does* support is narrower and still useful: **a failing or
> absent hook narrows** — Pi throws `Extension failed, blocking execution`
> (`agent-session.ts:533-550`) and non-interactive sessions block by default
> (`dirty-repo-guard.ts`); Codex treats a missing reviewer contributor as "not an
> implicit allow" (`guardian/decision.rs:44`).

WebCodex has a plugin gateway and skill system, but no defined rule that a plugin
cannot loosen core policy.

**Requirement (strengthened — a blocking hook is not sufficient):**

1. **Argument immutability or re-authorization.** A pre-execution hook must
   either be unable to mutate the validated arguments, or any mutation must
   invalidate the prior policy decision and force **re-evaluation** (the
   post-mutation action is what gets authorized, never the pre-mutation one).
2. **Execution authority.** Plugins run under their own capability grant; a
   plugin's own process must be sandboxed at least as tightly as the action it
   is intercepting, and plugin code must not execute inside the policy process.
3. **Result-integrity limits.** A post-execution hook may not flip
   `isError → false` on a hard-denied or sandbox-violation result, and may not
   synthesize success for an action that did not run. Hard-deny classification
   must be attached **before** plugin result hooks and be immutable thereafter.
4. **Restrict-only default.** The hook result type must be structurally unable to
   express "allow" for an action the policy denied (see
   `IMPLEMENTATION_PLAN.md` P8).
5. **Failure narrows.** Load failure, timeout, or exception ⇒ block.

**Requirement:** plugin hooks may only narrow *by construction*; mutations are
either impossible or re-authorized; results may not launder hard denials.

### I18. When uncertain: DENY / ASK, never ALLOW
`PARTIAL (review round 18, CHECK 1 — was recorded as `HOLDS_ALREADY`)`

**The requirement is settled; the universal "already holds" claim is not.** One
branch of one classifier is observed to fail closed:
`SOURCE = webcodex @ 7301186b :: src/tool_runtime/permissions/policy.rs:152-165 :
human_approval_required / auto_authorize` — on `InvalidMode` it reports
`human_approval_required = true` and `auto_authorize = false`, with the comment
"Fail closed: do not advertise frictionless auto-authorization."

**What is withdrawn (round 18, CHECK 1):** the heading asserts a global property
— *every* uncertain path fails closed — and the single `InvalidMode` branch does
not establish it. `InvalidMode` is one fallible input to one classifier. An
uncertainty that arrives as a **valid-but-unmodelled** mode, an unmapped tool
name, a permission rule that matches nothing, or an error path that returns
`Option::None` to a caller that then defaults to allow, would not reach
`InvalidMode` at all. No enumeration of the classifier's decision branches was
performed, so the count of fail-closed branches against the count of total
branches is `UNKNOWN`. This is the same defect round 17 found in I6 and the same
one round 18 found in I1/I2: **an unperformed survey is not a negative result.**
The status is therefore `PARTIAL` — one branch demonstrated, the global property
unestablished — not `HOLDS_ALREADY`.

OpenCode's default rule when nothing matches is `ask`, which is corroborating
design evidence rather than a WebCodex observation:
`SOURCE = anomalyco/opencode @ 3c893f0a :: packages/core/src/permission.ts:76-86`.

**Requirement:** preserve this property in every new branch; add a test per
branch (see `SECURITY_TEST_VECTORS.md`).

---

## 2. Invariants that must NOT be introduced

- Content scanning of tool results as a security control (A1, I6). This is a
  design prohibition and does **not** depend on I6's status: I6 being
  `UNVERIFIED` means the current tree's behaviour is unestablished, not that the
  prohibition is optional.
- A boolean "approved" argument (I2).
- A shell-command blocklist as the primary containment (I14).
- An auto-reviewer sitting *inside* the policy layer (I9).
- A global "trust this project" flag with no capability scope (I11).
- A danger switch reachable from the MCP tool surface (I3).

## 3. Summary table

| ID | Invariant | Status | Blocking gap |
|---|---|---|---|
| I1 | No self-escalation | PARTIAL (label) / ABSENT (technical) | no OS ceiling; no tool-manifest/runtime enumeration |
| I2 | No boolean approval | PARTIAL | declared surface only; full argument-schema enumeration not performed |
| I3 | Danger mode local-only | ABSENT | control channel reachability unproven and unimplemented |
| I4 | Sandbox covers tree | ABSENT | no sandbox; inheritance is a facility property, not a process-group property |
| I5 | No subprocess bypass | ABSENT | no sandbox; **no single spawn chokepoint exists** (≥10 direct spawn sites) |
| I6 | Injection detection ≠ boundary | **UNVERIFIED** (round 17) | grep the tool-result path for any transform/redaction/scanning stage |
| I7 | Malicious README ≠ more rights | PARTIAL | hard-deny is a post-effect prose classifier; shell uncovered |
| I8 | Policy ≠ sandbox layering | PARTIAL | only one layer exists |
| I9 | Hard DENY not overridable | HOLDS (classifier) / to build (pre-effect floor) | no central pre-effect floor |
| I10 | Timeout/failure fail closed | ABSENT | no approval flow |
| I11 | Scoped session grants | ABSENT | no grant model |
| I12 | Auditable approvals | PARTIAL | no approval records yet |
| I13 | Network ⊥ filesystem | ABSENT | no network control; enforcement mechanism unspecified |
| I14 | Escape ≠ shell spelling | ABSENT | no OS boundary |
| I15 | symlink/traversal in model | PARTIAL | WebCodex canonicalization claim **unverified in this research**; string-matching only |
| I16 | Secrets ≠ shell reachable | PARTIAL | no shell containment |
| I17 | Plugins restrict-only | PARTIAL | hook contract unproven; mutations must be impossible or re-authorized |
| I18 | Uncertain → DENY/ASK | PARTIAL | `InvalidMode` branch only; classifier branch survey not performed |

## 4. Review-driven corrections to this document

Adversarial review round 1 (`review/codex-architecture-review.log`) produced
issues against an earlier revision. Corrections applied here:

| Check | Correction |
|---|---|
| 2 | I7/I9 clarified: `is_hard_denied_output` runs **post-effect** and is a classifier, not an enforcement point. |
| 6 | I15: the WebCodex canonicalization/symlink claim was sourced from a different document and is **not verified here**; downgraded. Path rules are now stated per surface. |
| 8 | I17: the "restrict-only" claim is no longer attributed to Pi/Codex as a proven property. Requirements strengthened to argument immutability-or-re-authorization, plugin execution authority, and result-integrity limits. |
| 11 | Cross-document: facts sourced from the prior evaluation report rather than re-read here are now labelled as such. |
