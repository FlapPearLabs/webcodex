# IMPLEMENTATION_PLAN.md

Minimal-increment implementation plan for the architecture in
`REFERENCE_ARCHITECTURE.md`.

Baseline: `yyjeqhc/webcodex @ 7301186b98527cb4ebc0191f6033474e9abf7c20`.
This branch contains **research only** — no phase below is started here.

Guiding constraint (from the brief): build the smallest thing that closes the
invariants. Every phase must name what it *reuses*; a phase that only adds novel
design is presumed over-engineered.

Ordering is by dependency, not by value. `P0` and `P1` together close the single
highest-severity gap (shell has no boundary).

## WHAT THIS PLAN DELIVERS, STATED BEFORE THE PHASE LIST

**What this plan delivers is stated here in full, because rounds 29-32 established that a limitation stated only in a footnote is not stated at all. Round 33 changed two designs, so two of the limitations those rounds recorded are gone: confinement now covers the whole process tree by attaching at the runner, and approval is delivered over a non-GUI channel.** Specifically, after every phase in this plan ships:

- **Confinement covers the catalogued, mediated spawn surface.** A sandboxed child and its descendants inherit the profile, and a caller of the audited spawn API cannot obtain an unconfined child.
- **Confinement covers the whole process tree, by attaching at the runner.** The sandbox profile is established before `exec` and inherited by every descendant, where it cannot be widened, so confining `webcodex-runner` constrains every child below it — including children this plan never catalogued. Two residuals remain and are named: a `setuid` `exec`, and a raw spawn inside a pre-existing library that runs before the runner's own attach.
- **Approval is delivered, over a non-GUI channel.** P3 ships the full approval path — `once`/`session`/`always`, digest binding, replay protection, audit — carried over a unix domain socket owned by the operator's uid. No model-reachable surface can answer it, because answering requires an IPC connection the sandbox denies. The cost is that the decision is an operator CLI action, not a pop-up.
- **No network `ASK` and no hostname grant is released** unless criteria 1, 2, 3, 4, 5, 7, 9 and 10 all pass, and the hostname path is unsatisfiable within this plan.

**A plan that closed the whole tree, offered a model-reachable approval, and released a hostname grant would be a different plan.** This one is scoped to what the identified mechanisms can enforce, and each limitation above is stated where a reader meets the claim it limits. **The two places this plan stops are narrow and stated: a `setuid` `exec` and a pre-attach raw spawn.** Both are named in P1's round-33 note, and neither is the whole-tree gap that rounds 29-32 were blocked on.

---

## P0 — Foundation: spawn inventory, structured denials, capability vocabulary

**Goal:** establish what actually spawns processes, make the existing policy
layer trustworthy, and create the seam the sandbox will attach to. No new
capability is granted; nothing changes for users.

| Item | Value |
|---|---|
| `FILES` | `src/tool_runtime/permissions/model.rs`, `mod.rs`, `policy.rs`, `crates/webcodex-core/src/workflow_session_contract.rs`, new `crates/webcodex-capability/`, `research/spawn-surface-inventory.md` |
| `MODULES` | `permissions` (extend), new `capability` module |
| `REUSED_COMPONENT` | `webcodex :: permissions/*` (REUSE_EXISTING); capability names anchored to existing `webcodex_core::authority::SCOPE_*` (`crates/webcodex-tool-contracts/src/metadata.rs:166-181`) |
| `NEW_GLUE_CODE` | ~300–450 LOC |
| `TESTS` | structured-kind classification tests; capability-derivation unit tests; regression test that invalid mode still fails closed; **a call-graph test over the bounded process-entry inventory that enumerates the model-reachable spawn sites it reaches, recording every other entry as `UNKNOWN` rather than omitting it** (review round 18: the previous wording — "a call-graph test that **every** model-reachable spawn site is enumerated" — asked for the one thing no finite test can deliver, so it would have passed vacuously or been quietly narrowed) |
| `ACCEPTANCE_CRITERIA` | (1) **`research/spawn-surface-inventory.md` exists** and classifies every non-test `Command::spawn()` / `ManagedChild::spawn*` site by reachability (model-reachable / control-plane / operator-CLI / test), **established by static call-graph analysis — not by inspecting the tool manifest, which cannot show indirect spawns, and not by runtime instrumentation, which shows only exercised paths** (`SECURITY_TEST_VECTORS.md` CP-04). Runtime spawn tracing is permitted **in addition**, as corroboration and as a drift detector for the inventory, but it can never be the establishing evidence. This criterion was aligned to CP-04 in review round 8, CHECK 11: the two gates previously disagreed, with P0 still accepting instrumentation as sufficient. This must resolve the two sites left unclassified in `OSS_RESEARCH_EVIDENCE.md §2.1a` (W-23h, W-23i) and the reachability of `src/tool_runtime/helpers.rs:146` and W-57…W-63. **The claim is bounded in the same way CP-04's is, not universally (aligned in review round 15, CHECK 11).** An open-ended shell plus arbitrary code execution means no static method can decide the reachability of a binary chosen at runtime, so "classifies **every** non-test spawn site" is not deliverable as written and must not be read as a universal proof. The gate P0 actually ships is the bounded one: (a) every **catalogued** spawn site — the transitive closure reachable from a model-reachable tool through paths the sandbox profile permits — carries a three-valued verdict (`REACHABLE` / `UNREACHABLE` / `UNKNOWN`); (b) **every non-catalogued site is `UNKNOWN` and is reported as such in the gate record**, and the gate record lists the residual rather than resolving it in a document; (c) the inventory is **re-verified whenever the sandbox profile's permitted path set changes**. A site whose verdict is `UNKNOWN` is *unestablished* — the same `NOT_ENFORCED` discipline as `COMPONENT_REUSE_MATRIX.md §4`, and never reported as "unreachable" or as "does not happen". This is a restatement of CP-04's second consequence (`SECURITY_TEST_VECTORS.md`), not a new, weaker requirement: the residual is surfaced to the operator at P0 (2) `is_hard_denied_output`'s prose checks are backed by structured `error_kind`/`failure_kind` values, and the classifier is **documented as post-effect** with no call site treating it as pre-effect; (3) every `ToolRisk` maps to ≥1 capability; (4) the `SandboxBackend` trait exists with **no selectable no-op implementation shipped** — a "no backend" state is representable as a *denial*, never as a pass-through backend (see P1 acceptance 6); (5) no behavioural change in `trusted_agent`/`restricted` outcomes (existing tests green), **with one deliberate exception**: (6) **a plugin/hook disable gate exists and plugins are disabled by default** until P8 defines the contract. Today's plugin path is undefined with respect to I17 (argument mutation, execution authority, result laundering), so it must not keep running by default. The gate must be a **hard gate, not merely a default**: no configuration, flag, or tool call may enable plugin hook execution before P8's contract exists (added in review round 5, CHECK 8 — "disabled by default" was too weak, since it did not prohibit re-enabling). P8 additionally depends on P1b, because a plugin process must be sandboxed before it may run at all |
| `ROLLBACK` | additive modules; revert = drop the `capability` module and restore the string classifier (single commit). **No security posture depends on P0**, except that the plugin disable gate (6) must be retained on any rollback of P0 — reverting the gate restores an undefined plugin path and is not permitted |

**Why first:** `is_hard_denied_output` decides "hard deny" by matching prose in
an error string, and it runs after the fact. Fixing that before building a
sandbox prevents building on a floor that can be quietly removed — and the
spawn inventory determines whether a single attach point is even achievable.

---

## P1 — Real sandbox: normalize the spawn surface, then confine

**Goal:** close the **named baseline escapes** on the **pathname boundary**. This is the phase that changes the security posture. It has **two steps**, and the first was missing from an earlier revision.

> **The whole-tree property comes from confining the runner, not from enumerating spawn sites (review round 33; blocking CHECK 4 in rounds 29-32, unchanged across four rounds).** Rounds 16-32 all attacked the same gap from the text side: `UNKNOWN` reachability was disclosed, footnoted, moved into a phase that blocks nothing, and re-disclosed. The gap is **not** a disclosure defect. It is an **attachment-point** defect, and it has a producer that 33 rounds never designed.
>
> **The mechanism:** a Seatbelt/Landlock/bwrap profile is established **before `exec`** and is **inherited by every descendant**, where it **cannot be widened** (a child cannot obtain a profile weaker than its parent's; `REFERENCE_ARCHITECTURE.md §3` records inheritance as the platform facility). Therefore confining **one** process — `webcodex-runner`, the single process from which every sandboxed child is created — constrains the **entire process tree** below it, *including* children created by paths this plan has not catalogued. Enumeration is then a **convenience for testing and for the pre-`exec` attach**, not the security property itself.
>
> **What this changes.** P1a-R's premise is that an uncatalogued spawn is unmeasured and therefore outside the boundary. With runner-level confinement the residual is reclassified: an uncatalogued spawn is **still confined**, because it inherits a profile it never had to ask for. What remains genuinely open is narrower and is stated as such: (i) a child that `exec`s a **setuid** binary may regain privilege the profile cannot express, and (ii) a **raw `fork`/`posix_spawn` in a pre-existing library** that runs *before* the runner's own attach is outside it. Neither is closed here, and neither is what CHECK 4 has been asking about.
>
> **This is the "minimal increment" the brief asks for, and the earlier shape was not.** Confinement at the single attach point is **one** mechanism. Normalising every spawn site (P1a) is **N** mechanisms plus a permanent inventory that must be re-verified whenever it drifts. Round 33 therefore **demotes P1a from prerequisite to consequence**: attaching at the runner does not require the spawn catalogue to be complete, because nothing about the boundary depends on the catalogue's completeness.
>
> **The profile is a UNION, and this is the mechanism round 33 had to add (review round 33, CHECK 4 and CHECK 5, blocking).** Round 33's first attempt confined the runner to *one* profile and let P1c/P1d compile approved grants into *child* profiles. **That is incoherent, and the reviewer is right that no criterion in the plan noticed:** a child cannot be granted access its own parent is denied, so every network grant, every external read, and every relaxed mode would either fail or require the runner to be less confined than its most privileged child — which is the shape the whole design exists to prevent.
>
> **The resolution, stated as a criterion so it is checkable.** The runner is confined to the **union of every profile any spawn could legitimately need**, computed from the policy engine's capability vocabulary rather than from a hand-written list, and **each child receives its own narrower profile at `exec`**. Three properties follow, and each is a test:
> 1. **Monotone narrowing.** A child's profile is always a **subset** of the runner's. A grant can therefore never widen past the parent, and inheritance cannot subtract from the runner's own reach.
> 2. **The union is finite and derived.** It is computed from the mode/capability table P5 already defines, so a new mode changes the union by construction rather than by an audit someone must remember to perform.
> 3. **A profile that would exceed the union is refused at construction**, in the same place and with the same failure mode as (1b)'s no-plan refusal — so an over-broad request is a build error, not a runtime surprise.
>
> **What this costs, stated plainly.** The union is broader than any single child's profile, so the runner itself can do more than any one job needs — a shell spawned by the runner inherits the union unless (1) narrows it at `exec`. **The security consequence is bounded but real: a compromise of the runner process itself is outside what the per-child narrowing can contain.** This plan confines *what children can reach*, and that is the property the brief asks for; hardening the runner binary against its own compromise is a different task and is not claimed here. `REFERENCE_ARCHITECTURE.md §3` already records the two facilities (per-`exec` attach, inherited by descendants) and this row names which one carries the boundary.

> **"Close" is bounded to the pathname boundary, and round 30 withdraws the unqualified "close the shell escape" this goal used to carry** (round 29, CHECK 6, blocking). The boundary this phase establishes is **path denial plus profile inheritance**, and `SECURITY_TEST_VECTORS.md` PE-06a records a case inside it that the phase does not close: on macOS a child can create a hardlink to an outside inode **after** admission, and criteria (13a) and (13b) — a workspace-entry preflight and a resolver-time `st_nlink` check — expressly do not close it. So the accurate claim is **"the demonstrated escapes fail on paths, and one alias-based residual is disclosed and remains"** — and, per review round 32 CHECK 6, the goal sentence must not be read as covering what PE-06a records: a test of the direct pathname passes while the same outside inode remains readable through a hardlink the child creates after admission. **PE-06a is a permanent macOS limitation of this plan, not a pending verification**, not "escape is closed". The non-goal paragraph below already said this; the goal line is brought into line with it, because a goal is the sentence a reader quotes.

> **What P1 does and does not establish (review round 21, CHECK 11).** P1a and
> P1b together make the **enumerated, catalogued** model-reachable surface
> confined. They do **not** establish that the catalogued set is the whole
> model-reachable set. P1a (1b) is a **cooperative** reduction at the spawn API,
> P1a (1c) is **detective and `NOT_ENFORCED` until its independent mechanism
> exists**, and CP-04 already records the uncatalogued residue as `UNKNOWN`.
> **The residual is therefore `UNKNOWN`, and P1 passing does not change that
> word** (round 20, CHECK 4/11 withdrew the two sentences that said it did).
> Every downstream phase that treats "P1 shipped" as "confinement is total"
> is reading a claim this plan does not make; where P5 or the
> definition of done previously implied it, the implication is withdrawn
> here.

**P1a — Normalize the spawn surface (prerequisite).** `ManagedChild::spawn` is
**not** the only spawn path today: there are ≥ 10 non-test direct
`Command::spawn()` sites, several model-reachable on Unix, and none of the
managed path covers macOS's persistent-shell / remote-shell / SSH branches (see
`OSS_RESEARCH_EVIDENCE.md §2.1a` and `REFERENCE_ARCHITECTURE.md §1`). Confinement
cannot be attached at one function until this is resolved.

| Item | Value |
|---|---|
| `FILES` | `crates/webcodex-runner/src/webcodex_runner/{remote_shell.rs,ssh.rs,detached_job.rs}`, `crates/webcodex-persistent-shell/src/lib.rs`, `src/tool_runtime/helpers.rs`, `crates/webcodex-workspace/src/{workspace_checkpoint.rs,project_context.rs}` |
| `NEW_GLUE_CODE` | ~250–500 LOC (refactor, mostly deletions/redirection) |
| `REUSED_COMPONENT` | `webcodex :: crates/webcodex-process` (`ManagedChild`, `spawn_with_options`, `SpawnOptions`) — REUSE_EXISTING |
| `ACCEPTANCE_CRITERIA` | (1) **every spawn site attaches a plan, and a plan is not optional**. The operative requirements are (1a) and (1b) below; (1c) was a third condition until review round 30 moved it to phase P1a-R; this lead-in states the goal they jointly serve. **Corrected in review round 19 (mechanical sweep after round 18): the original wording of (1) was "every model-reachable spawn site either uses `ManagedChild::spawn*` or calls a shared `sandbox::apply(&mut Command, &SandboxPlan)` helper immediately before spawning", and it survived round 18 as a live requirement** — round 18 added the residual gate (1a)/(1b)/(1c) *next to* it without amending it, so the criterion simultaneously demanded an unachievable universal enumeration and supplied a bounded substitute. A reader executing the checklist would have had to decide which sentence binds. (1) is now a goal statement; (1a)/(1b)/(1c) are the acceptance conditions, and only those are checkable. The reasoning that follows is retained because it is the record of why the universal form was withdrawn. **This is a claim about the set of model-reachable spawns, and after round 16 the set is explicitly not enumerable (review round 17, CHECK 11).** CP-04 established that a bounded catalogue plus `UNKNOWN` non-catalogued entries is the honest form, which means "every model-reachable spawn carries a plan" is **not a property P1a can establish by enumerating sites**, and asserting it as a gate would let a `UNKNOWN` process-entry path be treated as covered the moment P1b ships. So the criterion is restated in the same two terms CP-04 uses, and a **residual condition was added** (as (1c), **now moved to phase P1a-R in round 30** — it is not an acceptance condition of P1a): (1a) every **catalogued** model-reachable site carries a plan, verified by the CP-02/CP-03 instrumentation; (1b) **the spawn API refuses to create a child without a plan** — `no_plan` is not a value a caller may choose at runtime; it exists only in a **build-time, audited list of control-plane sites** (operator CLI, desktop updater, bootstrap), each of which is outside the model-reachable set by construction rather than by assertion. A site not on that list and carrying no plan **fails the spawn** and is reported.
  **(1b'') the exemption list itself has an acceptance condition, which the
  round-22 reviewer correctly found missing (newly raised, round 22).** The
  list is described as making its entries "outside the model-reachable set **by
  construction**", and the very next sub-bullet concedes that this is a
  **premise, not an observation**. An unaudited premise on an exemption list is
  the same defect CP-04 already surrendered reachability for, one level in:
  if a list entry is wrong, every spawn at that site runs unconfined and
  nothing notices. So: **(1b''a) the list is a build-time artefact with a named
  owner, and each entry carries the same three-valued verdict CP-04 uses** —
  `REACHABLE` (model-reachable, therefore **not eligible for exemption**),
  `UNREACHABLE` (exempt), or `UNKNOWN`; **(1b''b) an entry whose verdict is
  `UNKNOWN` may not be used to spawn without a plan** — the exemption is
  refused and the site must carry a plan like any other; **(1b''c) an entry
  whose verdict is `REACHABLE` is a build failure**, not a warning, because it
  means the catalogue and the exemption list disagree; and **(1b''d) the list
  is re-verified whenever the spawn inventory changes**, on the same trigger as
  CP-04's re-verification. Absent (1b''b), an `UNKNOWN` entry is an unconfined
  spawn with a signed-looking name on it, which is the failure mode this whole
  criterion exists to remove. **AND (1b') the refusal is not, and is not claimed to be, a closure of the reachability question.** Round 19 tested this criterion against three counter-cases and correctly found that it binds none of them, so the overclaim is withdrawn here:
  - **Raw OS spawn.** A `libc::fork`/`exec*`/`posix_spawn` call, or a `Command` reached through a re-exported alias or a plugin crate outside this workspace, does not pass the API and is not refused by it. CP-03's lint gate covers direct `Command::spawn()` in the tree; it does not cover raw syscalls.
  - **Exempt caller.** A site *on* the build-time control-plane list may spawn without a plan. That list is audited at build time, but "outside the model-reachable set **by construction**" is a premise, not an observation — CP-04 has already surrendered universal reachability for uncatalogued entries, and the same doubt applies here.
  - **Plan widening.** The refusal asks whether a plan is **present**, not whether it is **narrow enough**. A caller holding a legitimate narrow plan can pass a widened one; presence is not integrity. The anti-widening rule in `REFERENCE_ARCHITECTURE.md §10` constrains *approval grants*, and no equivalent constraint on *plan construction* is stated anywhere.

  What (1b) therefore establishes is a **mechanism-level reduction**: a class of
  ordinary mistakes cannot yield an unconfined child. It is a **cooperative**
  control — it binds callers that go through it, and nothing else. Stated once,
  so no reader has to infer it: **this reduces the risk; it does not close the
  reachability question, and it is not a closure.** The independent residual is
  named in (1c) and is `NOT_ENFORCED` today, not discharged by (1b). and **The round-17 version of (1b) was a category error and is withdrawn (review round 18, CHECK 4/11 and a new defect).** It denied *non-catalogued executable paths*, which does not address the property at issue: an uncatalogued **spawn site** can launch a binary that **is** in the closure — `/bin/sh` is the obvious case — and path denial of *other* executables does nothing about it. Unknown reachability and executable identity are different properties, and only the first one is what CP-04 leaves open. The correct mitigation is at the **spawn API**, not the path table: the refusal is unconditional and applies to every caller, catalogued or not, so an uncatalogued site cannot obtain an unconfined child by naming a permitted binary. **The sentence "this closes the risk, not the uncertainty" is withdrawn in review round 20, CHECK 11.** Round 19 tested (1b) against a raw `fork`/`posix_spawn`, an unaudited entry point, and an exempt control-plane caller, and found the refusal binds none of them — so it does not even close the risk. The honest form is weaker on both sides: (1b) is a **cooperative reduction** that binds callers which use the audited API, and the reachability question is `UNKNOWN` independently of it.

**(1c) THE RESIDUAL IS ACCOUNTED FOR BY A PHASE THAT IS NOT A GATE ON P1a. **(1c) was P1a's third acceptance condition until review round 30; it is now phase **P1a-R**, disposition `SPECIFIED`, `NOT_ENFORCED`, blocking nothing, and the acceptance conditions of P1a are (1a) and (1b) only. Nothing below changes that, and any sentence further down that reads as a P1a condition is superseded by this one.** The property itself — every child created **without** passing the audited spawn API, a raw OS spawn, a plugin-crate spawn, a re-exported alias, is captured and reported by **one selected** mechanism Every child created **without** passing the audited spawn API — a raw OS spawn, a plugin-crate spawn, a re-exported alias — is captured and reported by **one selected** mechanism: **an `EndpointSecurity` (`ES_LOG`) subscription on the host, via the system-wide `exec` event** — **and the coverage claim is withdrawn, because selecting a producer is not the same as establishing what it can see** (review round 28, CHECK 4, blocking; graded as a NEW DEFECT of round 28, "an unselected producer into an overbroad selected one"). Round 28 asserted the subscription "observes every process creation on the machine regardless of which code path issued it", and that is overbroad in two specific ways. **(i) `exec` is not process creation.** A `fork()` with no following `exec` produces no `exec` event, so a child that forks and stays forked is invisible to an exec-only subscription. `posix_spawn` and `vfork` do exec, so the gap is narrow, but it is real and the criterion claimed every child. **(ii) An event is not an attribution.** The subscription reports that an exec occurred; nothing in this plan correlates that event with whether the spawning process went through the audited API, so the mechanism as specified cannot answer the question the criterion asks of it— "every child created **without** passing the audited spawn API" — **which requires either a per-spawn capability token the audited path stamps and the observer reads back, or a host-side audit-token process tree. Neither is designed here.** The honest statement is therefore: this is a **detective mechanism whose coverage is bounded and whose attribution is unbuilt**, and (1c) is not satisfied by naming it. **It also cannot prevent.** An `ES_LOG` subscriber observes; it does not deny, so a bypassed spawn is reported and still runs — which is why this residual is `NOT_ENFORCED` by construction and not merely pending. A second round-28 finding is recorded against the same criterion: the required entitled, signed, installed system extension is expressly not shipped by this plan, so the mechanism has a named producer *and* an unmet prerequisite, and naming the producer does not discharge the prerequisite.** That is the only mechanism selected, and it is selected because it is the only one of the three candidates whose coverage is a property of the OS rather than of this codebase: an interposer is bypassed by a statically linked or directly-invoked syscall path, and **the sandbox profile's own denial is prevention, not accounting** — it can stop a spawn and emit a narrower violation record, but it cannot report a child that was never submitted to the profile, so it cannot produce the report this criterion promises. (Review round 27, CHECK 4, blocking: round 26 removed the CI check and round 27 replaced it with an unselected set including that same non-producer — **a criterion that lists candidate producers and chooses none is the producerless placeholder round 26 blocked, one level up.**) **The residual is real and is stated rather than designed away: an `ES_LOG` subscription requires a signed, entitled, installed system extension, and this plan does not claim to ship one.** So (1c) carries a named producer AND a named prerequisite that is not yet met, and the sentence that used to sit here read "Until that mechanism exists, this residual is `NOT_ENFORCED` and P1a does not pass on (1b) alone", and **that sentence is withdrawn in round 31 (round 30, CHECK 4): it asserted a P1a gate that the P1a-R split had already removed, so P1a and P1a-R gave opposite pass conditions for the same phase.** The prerequisite is made non-aspiratory by P1a-R carrying it, not by a sentence inside P1a. **A CI source check is explicitly NOT an alternative here, and the earlier wording that offered it as one is withdrawn** (review round 26, newly-raised finding 1, blocking): a CI check reads *source*, while the property claimed is about *runtime children that actually exist*. The paragraph itself admitted the mismatch by describing the CI check as catching `Command::spawn` **call sites** rather than spawned processes, so the sentence was satisfiable with a mechanism that cannot produce its own stated report — **a test may observe a producer, never substitute for one.** A CI check remains valuable and IS required, but as a *complement* at a different layer: it asserts that no un-audited call site exists in the tree, which is a source property and is genuinely checkable there. It does not report stray children, and it is not substituted for the runtime hook. The report is a **named deliverable with an owner**, not a log line, and the independent mechanism is the point: a residual detected by the same component that creates it is not detected. **Until that mechanism exists, this residual is `NOT_ENFORCED`.**
>
> **(1c) IS NOT A GATE ON P1a. Split out as its own phase in review round 30,
> following round 29's strongest objection (L58).** Rounds 28 and 29 both
> recorded the facts — the extension is not shipped, `exec` is not process
> creation, and attribution needs a per-spawn token or an audit-token process
> tree that is not designed here — and then left (1c) inside P1a's
> `ACCEPTANCE_CRITERIA`, where its own text says P1a cannot pass without it.
> **That made the macOS-first path unsatisfiable by construction: the phase
> gated on a deliverable no phase in this plan produces.** Two rounds of
> honest disclosure did not fix it, because disclosure was never the problem;
> the gate was. So the condition is **removed from P1a's acceptance set** and
> becomes phase **P1a-R**, which is `SPECIFIED` and `NOT_ENFORCED` and is
> **not on the release path of any phase that ships confinement**.
>
> **What this changes and what it does not.** It does **not** weaken any
> control: (1b)'s refusal is untouched, and the residual was already labelled
> `NOT_ENFORCED` by every honest reading. What it removes is the *claim* that
> P1a completes something it cannot. **Universal process-tree confinement
> remains `UNKNOWN`, now without a gate that pretends otherwise** — which is
> the same word `CP-04` has carried since round 16, stated once instead of
> stated twice with contradictory consequences. P1a-R's disposition is
> `SPECIFIED`: a criterion exists, no producer is designed, nothing observes
> it, and no phase waits on it.

> **(1c) is DETECTIVE, not PREVENTIVE, and is therefore not part of a confinement
> gate. Corrected in review round 21, following round-20 CHECK 4.** Round 20
> accepted that the mechanism is independent and then treated the result as
> coverage. It is not, and the distinction is not pedantic:
> - A **CI source check** observes source text at build time. It cannot see a
>   spawn performed at runtime, by a plugin crate it does not build, or through a
>   path its pattern does not match. It reduces the *rate* of accidental
>   non-compliance; it does not prevent a single instance.
> - **Process accounting** observes a process **after** it exists. By the time the
>   hook fires, the child has already been created outside the plan and nothing
>   has stopped it. **Detecting an escape is not confining it.**
> - Neither mechanism touches **plan widening**: the refusal checks that a plan is
>   present, and a widened plan passes that check and is present.
>
> So (1c) is an **assurance** work item with real value — it produces an
> independent record of uncooperative spawns and it names an owner — and it is
> **not** a substitute for prevention. **The phrase "bounds how long an unconfined
> child can persist undetected" is withdrawn in review round 22.** It was mine and
> it was false: neither a CI source check nor an unspecified process-accounting
> hook supplies a **detection-latency bound**, and asserting one would be the same
> error as the closure claim in a different vocabulary — naming a guarantee nobody
> established. What (1c) actually provides is *evidence that a residual occurred*,
> on an unspecified timescale. If a latency bound is wanted, it needs a named
> mechanism with a stated observation interval, and no such mechanism is
> specified. The consequence is stated plainly:
> **P1a's acceptance does not rest on (1b) and (1c) together, and as of
> round 30 does not rest on (1c) at all.** What the plan ships is a cooperative refusal plus detection, and
> **universal process-tree confinement remains `UNKNOWN`** — gated by the honest
> reachability question CP-04 has carried since round 16, which no phase in this
> plan closes. Any future phase that wants to claim confinement must supply a
> *preventive* mechanism (an OS-enforced profile-inheritance or parent-death
> facility, or a platform on which an unconfined spawn is unrepresentable) and
> name it as such. A residual that is reported in principle and not detected in practice is not a disclosure. **Round 30 acts on the second half of that sentence: the residual is no longer an unclosed gate on P1a, because it is no longer a gate on P1a at all — it is phase P1a-R, `NOT_ENFORCED`, off the release path.** The residual is now *disclosed and unclaimed* rather than *disclosed and gated*, which is the only disposition available while no producer exists. (2) `SpawnOptions`/spawn path carries a mandatory plan field (or an explicit, audited `no_plan_reason`); (3) a compile-time or test-time guard fails when a new direct `Command::spawn()` is added; (4) the two unclassified workspace sites are resolved; (5) **the `INHERITED_DESCRIPTOR_GATE` is an explicit acceptance criterion of P1a, not only a named gate**: acceptance requires that the descriptor table is **successfully narrowed** at exec on the target platform for every covered spawn path, verified by a probe that a child cannot read an inherited descriptor to a denied path (the same probe `SECURITY_TEST_VECTORS.md` PE-09 uses), **and** that a backend which cannot perform the closure is recorded as `sandbox_backend = none` for the affected paths rather than silently proceeding. Added in review round 15, CHECK 6: the gate existed as a name while PE-09 still permitted `NOT_ENFORCED` "where descriptor closure is unavailable", so the row and the plan disagreed about whether closure was required. If closure cannot be demonstrated, P1a does not pass and the affected surfaces' `secret.read` stays `NOT_ENFORCED` as a recorded consequence, not as a silent one. | `FD_GATE` | **`INHERITED_DESCRIPTOR_GATE`** — at `exec`, close the child's descriptor table to a named allow-list (stdin/stdout/stderr, the job-control channel, and descriptors the plan passes explicitly) and set `FD_CLOEXEC` on every internal descriptor the runner opens. Added in review round 14 after `SECURITY_TEST_VECTORS.md` PE-09 exposed the gap: a path-deny profile cannot deny a descriptor the child inherited, so denying `~/.ssh/id_rsa` by path leaves an inherited fd to that inode fully readable. A backend that cannot rewrite the child's descriptor table makes PE-09 `NOT_ENFORCED` and must say so. **This is a gate, not a refinement** — without it, `secret.read` is not enforced for the shell surface even for enumerated paths |
| `ROLLBACK` | pure refactor with identical behaviour when no sandbox plan is present; revert is a normal commit |

## P1a-R — Runtime-bypass accounting (SPECIFIED; `NOT_ENFORCED`; not on any release path)

**This phase was created in review round 30 by splitting a gate out of P1a, and
it is deliberately the weakest-claim phase in the plan.** It exists so that the
open question "did a child bypass the audited spawn API?" has a name, an owner,
and an honest disposition, without that question being able to block anything.

| Item | Value |
|---|---|
| `DISPOSITION` | **`SPECIFIED`.** A criterion exists, no producer is designed, nothing observes it, and **no phase waits on it.** The three-value vocabulary is used deliberately: it is not `DELIVERED` (nothing delivers it) and not merely `PARTIAL` (nothing is half-built), and calling it `UNVERIFIED` would imply an observation was made and found inconclusive. **Round 33 re-scoped this phase after P1 moved the attach point to the runner: the question it was created to answer is now largely answered by the attach itself, and only the two residuals in P1's round-33 note are left. It is retained, smaller, rather than deleted, because a named owner for "did a child get an unconfined profile" is still worth having** |
| `GOAL` | Produce an **independent record** of children created without passing the audited spawn API. **The goal is a record, not a prevention, and not a closure of the reachability question** |
| `PRODUCER_CANDIDATE` | An `EndpointSecurity` (`ES_LOG`) subscription reading the system-wide `exec` event, as selected in P1a (1c) and carried forward unchanged. **Two properties of this candidate are already established as insufficient and are not re-argued here:** (i) `exec` is not process creation, so a `fork()` without a following `exec` is invisible; (ii) an event is not an attribution — nothing in this plan correlates the event with whether the spawning process used the audited API |
| `UNMET_PREREQUISITE` | The subscription requires a **signed, entitled, installed system extension. This plan does not ship one.** Recorded as an unmet prerequisite, not as a scheduled deliverable |
| `WHAT A COMPLETION WOULD REQUIRE` | A per-spawn capability token that the audited path stamps and the observer reads back, **or** a host-side audit-token process tree. **Neither is designed here**, so the criterion is not merely pending — it is unaddressed |
| `RELATION TO CONFINEMENT` | **None preventive.** An `ES_LOG` subscriber observes; it does not deny. A bypassed spawn would be reported *and still run*. **This is why the phase is split out rather than kept in P1a: a detective that cannot prevent must not sit in a confinement gate** |
| `BLOCKS` | **Nothing.** No phase's acceptance depends on this one. P1a passes on (1a) and (1b) alone |
| `IF IT IS NEVER BUILT` | The consequence is the one `CP-04` has carried since round 16 and states today: **universal process-tree confinement is `UNKNOWN`.** Uncatalogued spawn entries remain reachable and unmeasured, and the definition-of-done table says so in its own row rather than in a caveat |

**Why this is a phase and not a deleted paragraph.** The reason (1c) cannot simply
be struck is that the question is real: a raw `fork`/`posix_spawn`, a
plugin-crate spawn, or a re-exported alias is not refused by P1a (1b), because
(1b) binds callers that go through the audited API and nothing else. Deleting
the criterion would delete the record that the question is open. Keeping it in
P1a made the plan unsatisfiable. **A phase with no producer and no dependents is
the disposition that is honest under both constraints at once** — the question
stays visible, and nothing pretends to answer it.

---

**P1b — Confine.** Scope is deliberately narrow.

| Item | Value |
|---|---|
| `FILES` | `crates/webcodex-process/src/lib.rs`, new `crates/webcodex-sandbox/` (`backend.rs`, `plan.rs`, `seatbelt.rs`), and the P1a call sites |
| `MODULES` | `sandbox` crate; `webcodex-process` attach |
| `REUSED_COMPONENT` | `/usr/bin/sandbox-exec` invocation pattern + SBPL concepts from `codex :: sandboxing/src/{manager.rs:352,seatbelt.rs:62}` — SUBPROCESS_ADAPTER + `REIMPLEMENT_SMALL_CORE`; `codex-process-hardening` (currently **not assessed**, see `COMPONENT_REUSE_MATRIX.md §2`) to be read before writing hardening code |
| `NEW_GLUE_CODE` | ~800–1300 LOC |
| `SCOPE` | **macOS only. One profile only:** read/write inside the project root, read system paths needed to run a toolchain, **no network**. Linux is a later, separately-justified phase. **"System paths needed to run a toolchain" is now a bounded, checkable scope (review round 21).** `COMPONENT_REUSE_MATRIX.md §4` makes `fs.read.external` `DENY` outside the project root *and* this allow-list, and names the allow-list a **deliverable of this phase with negative tests** — but P1b's own criteria did not require producing it, so the rescope was a promise with no gate behind it. The **TOOLCHAIN_READ_ALLOWLIST** is therefore an explicit acceptance criterion of this phase — **criterion (12) below**. (The number has moved twice and both moves were mine: round 21 wrote "(6)" and placed the gate in **P1a's** table, where it collided with P1a's own numbering; round 22 re-inserted it as "(10)" but into a `(cont.)` row that still sits *before* the P1b heading, so the gate was still in the wrong phase's table and "(10)" was already used here. Round 23 puts it in this phase's own row and numbers it (12). See L19.): an enumerated path list, plus a negative test proving a path *outside* it is refused, plus a positive test proving each entry is actually needed by a build. Without the enumeration, "paths needed to run a toolchain" is a category rather than a scope, which is the same unbounded-exception shape round 9 caught `fs.read.external` for |
| `TESTS` | `SECURITY_TEST_VECTORS.md` §1–§4 (workspace read/write, external read/write, symlink/`..`/absolute, python/node/bash/subshell escape, network outbound/localhost) against the real profile |
| `ACCEPTANCE_CRITERIA` | (1) `cd /tmp && cat <outside>` **fails** (the exact baseline probe that currently succeeds); (2) `ls ~` fails; (3) `curl https://example.com` fails (no-network profile); (4) `python3`/`node`/`bash` grandchildren are confined identically; (5) a re-spelled path cannot escape; (6) **no backend ⇒ capability denied**, reported as `sandbox_backend = none`, and there is **no selectable pass-through backend**; (7) a deliberately over-permissive rule is caught by a negative test; (8) security decisions never depend on shell-text parsing; (9) **the profile carries explicit secret deny-rules for outside-tree credential locations and for in-project secret patterns** (`.env`, `*.pem`, `id_rsa`, `.npmrc`, cloud credential files, agent token stores) — a profile that merely grants "read the project root" does **not** pass; (9a) **`.git/config` is NOT on the in-project deny list, and the resulting gap is stated rather than papered over (reconciled in round 8, corrected in round 9).** Denying it while P5 promises uninterrupted local Git read/write under `AUTO` made the two requirements unsatisfiable — Git needs its own config to function. But the round-8 substitute (a *policy* against `url.*.insteadOf` and external `credential.helper`) did **not** establish a credential boundary either: a policy governs mediated tool calls, and `cat .git/config` is an ordinary byte read by a confined process, not a mediated call. A token embedded directly in a `url = https://user:token@host/...` remote is covered by neither setting. Therefore: (i) the file is readable, (ii) **an external `credential.helper` is `DENY` as a P2 policy check on Git invocations — but only for a `git` invocation that goes through tool dispatch.** The round-10 reviewer caught the same per-action gap already accepted for network grants: `git` run *inside* a permitted shell, or by a build step (`npm`, `make`, a script), never returns to the dispatch boundary, so a dispatch-time policy check cannot see it. The honest statement is therefore that this control covers the **mediated** path only; inside a running shell the helper may still execute, and no OS-level or per-syscall boundary in this plan would prevent it. It still reduces blast radius where it applies, and it is (iii) **a credential embedded in `.git/config` remains readable and transmittable; this is a `KNOWN_LIMITATION`** recorded in `OSS_RESEARCH_EVIDENCE.md §8`, not a protected case. Denying the file was the wrong layer: it breaks correct operation, does not even stop the leak it aimed at, and a remote URL is readable through that file anyway | (10) the secret-pattern list is operator-extensible and its **incompleteness is stated**, not implied away (`SECURITY_TEST_VECTORS.md` NET-11 note); (12) **the `TOOLCHAIN_READ_ALLOWLIST` is enumerated and tested, and it is a criterion of *this* phase (review round 23 — round 21 placed this gate in **P1a's** table as "(6)", where it collided with P1a's own numbering; round 22 deleted it from P1a and then re-inserted it as "(10)" into an `ACCEPTANCE_CRITERIA (cont.)` row that still sits *before* this heading, so the gate was in the wrong phase's table for a second consecutive round, and "(10)" was already taken here by the secret-pattern list).** The gate belongs to the phase whose `SCOPE` it bounds, which is this one. `COMPONENT_REUSE_MATRIX.md §4` makes `fs.read.external` `DENY` outside the project root **and** outside this allow-list, so the list is what makes that default true rather than approximately true — which is why the matrix's citation of it as a bound is only supportable from here. Three requirements, all checkable: **(12a) the list is enumerated** as a deliverable, not described as a category; **(12b) a negative test proves a path outside the list is refused** by the compiled profile, and that refusal is the one `SECURITY_TEST_VECTORS.md` FS-06/FS-07 and EX-01…EX-12 are conditioned on; **(12c) a positive test proves each entry is actually required** by a representative build, so the list cannot quietly become a general read-anything grant. A list that fails (12c) is a hole with a name on it, which is worse than the unbounded version it replaced. **Until (12a)–(12c) pass, the toolchain-read scope is a category rather than a scope and `fs.read.external`'s stated default is provisional.**

(13) **the hardlink defence is the pair of named steps and both are implemented: (13a) a workspace-entry preflight, without which the link-count rule has no input, since the rule counts links at a point in time and a pre-existing alias must be counted before any mediated path resolution returns it; and (13b) a resolver-time `st_nlink` check, without which a link created an hour into a session escapes a start-up-only check. On macOS these two steps are the **whole** of the hardlink defence, since the kernel rule PE-06a relies on is absent there. (13) does not close PE-06a on macOS; it is the most this plan can do, and PE-06a remains a `KNOWN_LIMITATION`.** (11) **the two unassessed reuse options are assessed before any profile is compiled** — `DIRECT_DEPENDENCY` via a pinned git revision, and `VENDOR_SUBSET` of a bounded piece, each evaluated against the same four criteria `COMPONENT_REUSE_MATRIX.md §2` applies to the assessed options (publication/API surface, staleness and update path, licence and notice obligation, LOC delta), with the verdict recorded. **This is the gate that makes §2's recommendation provisional rather than merely cautious**: if either option is assessed as equal-or-better on all four criteria, the `SUBPROCESS_ADAPTER` recommendation is reopened and P1b does not start against it. Added in review round 14, CHECK 1 — the round-13 fix stated this gate in the matrix while the plan contained no criterion or deliverable that would produce it, which made it a promise in one document and an absence in the other |
| `ROLLBACK` | **No unsafe runtime switch.** An earlier revision proposed `WEBCODEX_SANDBOX=none` to "restore today's behaviour"; that contradicts fail-closed and is removed. The only supported disable is a **build-time** or unreleased-core configuration used by maintainers, documented as unsafe and unavailable in release builds. Operationally, the rollback is to stop exposing the affected capability (READ_ONLY posture), not to run unsanded |

**Explicit non-goal in P1b:** do not add approval UX. P1b removes the *specific
escapes demonstrated in the baseline* on the covered surfaces; P2/P3 make the
remaining friction *comfortable*. P1b does **not** claim escape is impossible —
it delivers a qualified guarantee: confinement is inherited by the whole tree
**unless a child leaves the sandbox facility**, and pre-existing in-workspace
hardlinks are addressed by the link-count rule with one stated residual
(`SECURITY_TEST_VECTORS.md` PE-06). Stating the limit is part of the acceptance
criteria, not a caveat bolted on afterwards.

**The link-count rule requires two implementation steps this phase previously did
not ask for (review round 21, from a round-20 finding).** `PE-06`/`PE-06a` speak
of a *link-count rule*, and a link-count rule is only meaningful if the count is
actually consulted. **Both are now numbered criteria of this phase's own `ACCEPTANCE_CRITERIA` row — (12) the toolchain-read allow-list and (13) the hardlink pair — rather than prose beneath the table, which is what round 24 got wrong (newly-raised finding 1: the pair sat *after* the table and its `ROLLBACK` row while the sentence claimed it was in the checklist).**
**(i) a workspace-entry preflight** that enumerates entries in the project root
and reports any with `st_nlink > 1`, so an operator learns about a pre-existing
hardlink instead of the sandbox silently permitting it; and **(ii) a resolver-time
`st_nlink` check** in the path-resolution layer, so a link created after start-up
is caught by the same rule rather than by luck. Without (i) the rule has no
input, and without (ii) it has no timing coverage — a build that creates a
hardlink an hour into a long session would otherwise pass a check that only ran
at start-up. On macOS the kernel rule is absent, so (i) and (ii) are the *whole*
of the hardlink defence there and PE-06a remains a permanent
`KNOWN_LIMITATION` regardless.

**The toolchain-read allow-list is an acceptance criterion, not a description
(rounds 21 and 22).** `COMPONENT_REUSE_MATRIX.md §4` now scopes
`fs.read.external` to "outside the project root **and** the toolchain-read
allow-list", and names that allow-list a deliverable of this phase with negative
tests. Until P1b required producing it, the rescope was a promise with no gate
behind it — the same unbounded-exception shape round 9 caught this capability
for. **Round 21 wrote this gate as "criterion (6) below" and put it in P1a's
table by mistake, where it collided with P1a's numbering; it is criterion (10) of
this phase, and the numbering was corrected in round 22 rather than left to be
discovered by whoever implemented it.** **Criterion (12) below is that gate: an enumerated list, a negative test proving a path outside it
is refused, and a positive test proving each entry is genuinely needed by a
build.**

---

## P1c — Network descriptor and enforcing proxy (required before any network `ASK`)

**Added in review round 5 (CHECK 5).** Without this phase the plan could offer a
network `ASK` whose approved outcome has **no enforceable execution path**: P1b
ships a no-network profile, and no other phase committed to implementing the
per-request descriptor or the local proxy. An approval that cannot be enforced is
not an approval.

| Item | Value |
|---|---|
| `FILES` | new `crates/webcodex-sandbox/src/net.rs` (descriptor → profile rules), local proxy in `src/` (control-plane-owned socket), per-spawn plan plumbing from P1b |
| `MODULES` | `sandbox::net`, proxy service |
| `REUSED_COMPONENT` | **to be read first:** `codex-network-proxy` (currently `NOT_ASSESSED`, `OSS_RESEARCH_EVIDENCE.md §7.1`) for managed-proxy / MITM plumbing; `codex :: sandboxing/src/seatbelt.rs:336-346` for rule shape — `DESIGN_ONLY` until assessed |
| `NEW_GLUE_CODE` | ~400–700 LOC (descriptor compilation + proxy + per-spawn plumbing) |
| `SCOPE` | **Two distinct enforcement paths, stated separately (clarified in review round 6).** (i) **IP:port descriptors are proposed to compile directly into the sandbox profile as literal `remote ip "<addr>:<port>"` rules — this is a DESIGN PROPOSAL, not an observed mechanism.** The reviewer of round 8 correctly caught that the submitted Codex evidence shows only **localhost** and **DNS** rule shapes (`seatbelt.rs`), **not** arbitrary destination matching, so the phrase "the same mechanism Codex uses" over-claims and is withdrawn. `COMPONENT_REUSE_MATRIX.md §2` records the arbitrary-destination rule shape as `NOT_ASSESSED`. Acceptance criterion 7 below therefore exists: the rule shape must be **compiled and negatively tested** before any IP:port grant is offered. (ii) **Hostname-scoped descriptors would require a locally-owned proxy**, because a name cannot be decided by an IP rule without resolving it at connect time. Hostname grants stay **unavailable** regardless, because (i) is itself unverified. There is no third "direct socket enforcement path" for hostnames. **Consequence: until criterion 7 passes, the entire network axis offers only `DENY`, and no network `ASK` exists at all.**<br><br>**A control-plane-owned proxy does not enforce anything by ownership alone (corrected in review round 9).** The round-9 reviewer caught that §6.1 and this phase required "a shared local enforcing proxy" without ever binding a given child to that child's approved destination list. Ownership answers *who runs the proxy*; it says nothing about *which destinations this spawn may reach*. A proxy reachable by every sandboxed child, holding no per-spawn state, cannot enforce a per-spawn grant — the first child to connect gets whatever the proxy allows globally, and every other child inherits it. The missing binding is explicit in criterion 8 |
| `TESTS` | `SECURITY_TEST_VECTORS.md` §4 columns A/B/C; NET-03 (DNS); NET-02b (control plane); NET-10 (grandchild); confinement must hold for grandchildren |
| `ACCEPTANCE_CRITERIA` | (1) a descriptor is compiled into the profile **for that spawn**; (2) DNS is **denied by default**; name resolution works only through a locally-owned proxy, parameterised by the request allow-list; (3) the control-plane channel is unconditionally denied (NET-02b); (4) localhost grants are port-scoped and never expressed as "localhost allowed"; (5) a surface that cannot express the descriptor is `DENY`; **(9) the compiled profile carries an explicit `DENY` rule for the cloud metadata address `169.254.169.254` (and its IPv4-mapped/IPv6 forms), independent of any literal-IP *allow* grant — a metadata deny is a distinct rule from criteria 1–7, not a consequence of them. `SECURITY_TEST_VECTORS.md` NET-08 was an `OUTCOME_STRICT` `DENY` with no producing criterion until round 24; this is that criterion. Its expressibility is unverified (`COMPONENT_REUSE_MATRIX.md §2` records destination matching as `NOT_ASSESSED`), so **(9a) the rule is compiled and positively tested — a fetch to the metadata address is observed to fail **on each enforcement path that the same release gate opens** — and (9b) if the profile cannot express it, outbound access is `DENY` by default for the whole address class rather than the rule being dropped.** A metadata deny is a floor, not a boundary: it does not stop a proxy-mediated fetch, DNS rebinding, or an attacker-chosen metadata host, and NET-08's cell is not upgraded by it. **The (9a) probe is therefore scoped per path, not once: a passing direct-socket probe discharges the compiled-profile path only, and the proxy-mediated path stays closed until it has its own probe** (review round 26, newly-raised finding 2). One probe covering two paths is the per-action/per-call confusion of round 10 in the network axis: the profile rule and the proxy decision are different mechanisms, and a pass on one says nothing about the other. The hostname path is in any case unavailable (criterion 8), so the practical effect today is that the metadata floor holds on the literal-IP/localhost path, which is the only network path this plan releases. **(10) the compiled profile denies a listener bind to a wildcard address (`0.0.0.0`, `::`) — NET-09, also unproduced until round 24. A bind is not a destination, so the `remote ip` rule shape of criteria 1–7 does not reach it, and the cited SBPL reference includes an **allow-bind** shape, so the rule's direction has to be established rather than assumed. (10a) the bind rule is compiled and negatively tested: a child that binds a wildcard address is observed to fail; (10b) loopback binds continue to work, since denying `127.0.0.1` would break ordinary tooling and is not the requirement; (10c) **if the profile cannot express a bind-deny at all then criterion (10) FAILS, the network axis is `DENY`, and `NET-09` is recorded as `NOT_ENFORCED` with the profile's bind behaviour named as the reason. That is the whole of branch (ii), and it is stated first because it is the operative outcome** (review round 28, CHECK 5 and CHECK 9, blocking: this cell opened "`NET-09` is `DENY` only by the profile's own bind default" and appended the failure eleven words later, so the cell still read as a conditional `DENY` to anyone scanning it. **A cell that opens with the strong claim and closes with the correction is the defect, not a formatting preference** — the same ordering round 28 blocked in the matrix's Default cell and in STV §4. The withdrawn phrasing, and why it is wrong, are recorded immediately below rather than in front of the outcome.)** — it is failed, and failed is the only reading this plan permits. (Round 26 CHECK 5: the earlier wording said the criterion "is satisfied only when that default is *observed* to deny a wildcard bind", which supplied a SECOND, weaker meaning of "criterion (10) passes" alongside the branch-(i)-only meaning in (N-a). Two meanings for one gate is the defect this review has now found three times, and it is the same shape as the round-20 criterion-6 failure: an appended correction rather than a replaced one. The observation-based path is withdrawn for a second, independent reason as well — **observing a default-deny cannot produce one**, so a test would be standing in for a mechanism that does not exist, which is the producerless property six producer sweeps removed. The two branches below are the whole of criterion (10).**) A bind-category default-deny was named here in round 24 **without naming a producing component, and is withdrawn** (review round 24, new defect 2): a bind *test* can observe a default-deny, it cannot produce one, and a fallback with no mechanism is the same producerless property this review has spent six rounds removing. The honest form has two branches, and both are stated so an implementer cannot pick the convenient one: (i) if a bind-deny rule compiles, the criterion is met by (10a); (ii) if it does **not** compile, the plan must either locate the profile's bind default and test that it denies the wildcard bind, **or record `NET-09` as `NOT_ENFORCED` with the profile's bind behaviour named as the reason.** **This "locate the default and test it, **or** record `NOT_ENFORCED`" is a second branch (ii) and it is withdrawn** (review round 27, CHECK 5, blocking). It offered an implementer a choice between two dispositions for the same condition, which is the same two-meanings defect that (N-a) fixed in round 27 — **fixed in one place, left alive in another, one paragraph below.** There is now one disposition for branch (ii) and it is not a choice: **criterion (10) fails, the network axis is `DENY`, and `NET-09` is recorded as `NOT_ENFORCED` with the profile's bind behaviour named as the reason.** The bind default may still be *documented* if an implementer finds it, but documenting it does not convert the failure into a pass and cannot produce a test that discharges (10). In branch (ii) a wildcard bind remains possible inside the sandbox, which is a `KNOWN_LIMITATION` disclosed rather than designed around. **It may not be recorded as `DENY` on the strength of a rule shape nobody has shown to exist.** (6) **the release gate is per enforcement path. The opening sentence of this criterion has now been rewritten three times, and the reason is recorded so it does not happen a fourth: each earlier fix appended a correction to the sentence rather than replacing it, so for several rounds this cell simultaneously opened with one gate and closed with another (round 17 CHECK 5, then round 20 CHECK 5 and CHECK 9).** Until the criteria for its own path pass, a network action resolves to `DENY`, including under `AUTO` and `APPROVE_FOR_ME`. **Both gates are stated here, in the opening, and the detailed sub-bullets below are a restatement of this sentence rather than an additional condition on it:**
  - **no phase may offer a HOSTNAME-scoped network `ASK` until criteria 1, 2, 3, 4, 5, 7, 8, 9 and 10 pass** — **and since criterion 8 is unsatisfiable within this plan, no hostname `ASK` is ever released by it. The range "1–8" that opened this bullet is withdrawn: it named criterion (6) — this bullet's own opening rule, which is the sentence being governed, not a criterion governing it — and it omitted the mandatory floors (9) and (10), so it was simultaneously circular and incomplete (review round 28, CHECK 6, blocking; L49). This list is the compiled-profile list plus (8), which is the proxy's own per-spawn binding requirement.**
  - **no phase may offer a LITERAL-IP or LOCALHOST-PORT network `ASK` until criteria 1, 2, 3, 4, 5, 7, 9 and 10 pass.** (Criteria 9 and 10 added in round 25; this sentence and the compiled-profile sub-bullet below are the same gate and are edited together — the round-20 failure was editing one copy.) Criteria 2 (DNS default-deny) and 3 (control-channel unconditional denial) are **profile** properties, not proxy properties, so they bind this path exactly as they bind the other; a compiled profile that omitted them would simply lack them, not inherit them. **This is the complete release gate for that path, it is identical to `P5` criterion 6, and it is the only statement of the literal-IP/localhost release condition in this cell.** A second, shorter sentence repeating "criteria 1, 4, 5 and 7" stood here until review round 22 — round 21 rewrote the opening of (6) and marked the sub-bullets a restatement but left this sentence untouched inside the same paragraph, so the cell asserted two different gates two lines apart. **Criteria 2 and 3 are part of this gate; a release on 1, 4, 5 and 7 alone is not permitted by this plan, whatever any other sentence says.** There is no third path, and no unqualified "network `ASK`" exists. Criterion 8 is `PROXY_SPAWN_BINDING` and is **part of the release gate, not a refinement of it**: until every child holds a per-spawn unforgeable capability and the proxy holds a matching per-spawn allow-list, a reachable proxy is not an enforcing proxy, and hostname grants stay unavailable (corrected in round 10 — the binding was previously described in its own row while the gate stopped at criterion 7, so the two did not agree on what releases a network `ASK`). **Split by enforcement path in review round 16, CHECK 5 — as written the gate was self-defeating.** Criterion 8 is unsatisfiable within this plan (see the criterion-8 row), so "criteria 1–8 pass" could never hold, which silently blocked **every** network `ASK` including the literal-IP and localhost grants that are enforced by the compiled sandbox profile and have nothing to do with the proxy. The gate now reads per path:
  - **Proxy path (hostname-scoped grants): criteria 1, 2, 3, 4, 5, 7, 8, 9 and 10.** **Criterion (6) is deliberately NOT in this list, and its removal is round 28's correction of round 27's own regression.** Round 27 rewrote this bullet to add (4), (9) and (10) and, in doing so, typed **criterion (6) into its own release gate** (review round 27, CHECK 6, blocking; listed as a new defect of the round-27 revision). A gate that lists itself is circular and unsatisfiable in the same breath: satisfying the path's precondition would require the path's precondition. (6) is the OPENING RULE of this criterion — the per-path statement that until a path's own criteria pass, its actions are `DENY` — so it is the sentence being *governed* here, not a criterion *governing* it, and listing it is a category error rather than an oversight. The floors round 27 added — (4) localhost scoping, (9) metadata-address deny, (10) wildcard-bind deny — are kept, and (8) is kept because it is the proxy's own per-spawn binding requirement. This list is therefore the compiled-profile list plus (8), with the circular member removed. (Corrected in round 27, review round 26 CHECK 9: this bullet listed 1, 2, 3, 5, 6, 7 and 8, omitting **(4) localhost scoping** and the two floors (9) metadata-address deny and (10) wildcard-bind deny that round 24 added as mandatory. The omission of (9)/(10) is the same ordering error round 25 caught on the literal-IP path — a network grant released while a required network `DENY` has no criterion behind it — repeated on the proxy path one bullet later. (4) is included because a hostname grant that also permits an unscoped localhost reach is the same defect (4) exists to prevent, and (9)/(10) are floors for the network axis as a whole, not for one enforcement path. Note that this path remains unavailable regardless: criterion 8 is unsatisfiable within this plan, so the corrected list is honest rather than enabling.** Unchanged, and because criterion 8 is declared unsatisfiable within this plan, **no hostname `ASK` is ever released by this plan** — that is the accepted consequence, not an oversight, and it is why §4 column B is headed FUTURE-PHASE TARGET.
  - **Compiled-profile path (literal-IP and localhost-port grants, i.e. `NET-02`/`NET-04`): criteria 1, 2, 3, 4, 5, 7, **9** and **10** — criteria 9 and 10 added in round 25 (review round 24, newly-raised finding 2). The metadata-address deny and the wildcard-bind deny are network floors, and **releasing a network `ASK` while a required network `DENY` has no criterion behind it is the same ordering error as releasing a write grant before the sandbox exists.** ******A grant is not a gate, and neither is a rule that is merely written. The round-25 wording here said criteria 9 and 10 "need not *pass*" for the grant to be offered, which made the widened gate a list rather than a gate — the round-25 reviewer is right, and the defect is this review's own recurring shape turned on my own fix: the numbers were edited in all three sites (round 25's L36) while the *meaning* was edited in one and contradicted the other two. That sentence is withdrawn. The gate is now executable and means what it says:**
  - **(N-a) criteria (9) and (10) must PASS before any literal-IP or localhost-port `ASK` is offered.** For (9), passing means the (9a) probe observes a metadata fetch failing. For (10), passing means **branch (i) only** — the (10a) probe observes a wildcard bind failing because a bind-deny rule compiled.
  - **(N-b) if (10) resolves to its branch (ii), then (10) has NOT passed, so by (N-a) no network `ASK` is offered at all.** This is deliberate and it is the boring failure direction: an unexpressible bind-deny means the network axis stays closed, exactly as criterion 8's unsatisfiability already closes the hostname axis. **A test row may not record `NET-09` as `DENY` while this gate is unmet** — `SECURITY_TEST_VECTORS.md`'s `NET-09` cell is corrected for exactly this reason.
  - **(N-c) the network axis therefore has exactly two states: either every floor in criteria 1, 2, 3, 4, 5, 7, 9 and 10 passes and IP/port `ASK` is available, or the axis is `DENY`. There is no third state in which a scoped grant is advertised while a required floor is unmet.** identical to the opening rule above, which is now the operative statement. (The previous wording of this sub-bullet read "criteria 1, 4, 5 and 7 only" and was corrected in round 20; the opening sentence above it was corrected in round 21. Both halves must move together, and a future change to either must change both.) These are enforced by the kernel on the child's own sockets at spawn time, so they are gated on the rule shape being **compiled and negatively tested** (criterion 7), and they are **not** gated on criterion 8 because there is no proxy in the path and therefore no attribution problem to solve. **Criteria 2 and 3 are nevertheless required, and round 19 caught this row still listing "1, 4, 5 and 7 only".** The round-18 reasoning — that DNS default-deny and control-channel denial belong to the proxy path — was wrong in a specific way: those two criteria are **profile properties, not proxy properties.** A compiled profile that omits the DNS rule and the control-channel rule does not thereby inherit them from the proxy; it simply has neither, and a literal-IP grant would then be released into a profile where a child can resolve arbitrary names and reach the control channel by a path the grant never constrained. Requiring 2 and 3 here costs nothing — both are compiled into the same profile as criteria 1/4/5 — and their absence was a live hole, not a redundancy. This is the only network `ASK` this plan delivers, and it is a deliberately narrow one. **The same limit applies at every level that mentions a network `ASK` without qualification** — `COMPONENT_REUSE_MATRIX.md §4`, the phase sequencing below, and P5 — and each of those now names the path it means, because an unqualified "network `ASK`" in a summary is read as the union of the paths rather than the one that survives.
  The distinction is the substance of criterion 8's own finding: an unconfined-by-proxy mechanism (the profile) fails differently from a proxy-mediated one, and holding both to the proxy's release gate understated what the plan delivers while overstating what it can enforce. A reader must be able to tell **which** network capabilities this plan actually releases; before this fix, that answer differed depending on whether the reader noticed criterion 6 or criterion 8; (7) **the arbitrary-destination `remote ip` rule shape is demonstrated, not assumed** — and the test uses a **literal IP**, never a hostname (corrected in round 9: with DNS denied, a hostname-scoped test cannot exercise the no-proxy path at all). Using a literal address, a profile granting `203.0.113.10:443` must (a) permit exactly that destination, (b) **fail** for a different IP on the same port, and (c) **fail** if the rule is compiled with a permissive wildcard; a one-character mutation toward permissiveness must fail the suite. **The positive leg needs a reachable endpoint** (corrected in round 10): a denial elsewhere cannot show the permitted destination *works*. **Corrected again in round 11 — a loopback listener proves the wrong property.** A listener on `127.0.0.1` exercises the *localhost* rule path, which SBPL expresses directly and which the document already covers; it does **not** show that an arbitrary non-loopback destination is permitted while adjacent destinations are denied. Substituting loopback for `203.0.113.10` changed the property under test rather than supplying a fixture for it.<br><br>The test therefore needs **two distinct fixtures**, because two distinct properties are being claimed:<br>(a) **Localhost rule** — a loopback listener on a fixed port; grant that exact address; assert success; assert a different loopback port fails. This is the NET-02 case.<br>(b) **Arbitrary-destination rule** — a **non-loopback** destination that the test machine can actually reach on the granted port, e.g. a second address bound to a local interface on the same host, or a reserved documentation-range host the CI environment can route to. `203.0.113.0/24` (RFC 5737 TEST-NET-3) is the right *specimen* to name in a profile, but it must be paired with a statement of how the test reaches it; if the environment cannot route there, the arbitrary-destination property is **not demonstrated** and criterion 7 fails, rather than silently degrading to the loopback case. Assert the positive connection **succeeds**, then that an adjacent address on the same port **fails**, then that a permissive wildcard **fails**.<br><br>The reason this is written out rather than left to the implementer: **a test that only ever observes `DENY` is compatible with a profile that denies everything**, and a loopback-only test is compatible with a profile that has no arbitrary-destination support at all. Both pass vacuously. **Hostname reachability is tested separately** and only through the proxy (§4 column B); mixing the two in one test proves neither. **If SBPL cannot express the literal-IP rule, the enforceable descriptor is reduced to `localhost-port-only` and everything else stays `DENY` — and the localhost-port path is NOT thereby released** (review round 28, newly-raised finding, blocking-adjacent: this fallback contradicted its own gate). The literal-IP and localhost-port paths share one release gate, and that gate requires criterion (7) to pass, and criterion (7) is the demonstration that a profile can express an arbitrary destination. **So a profile that cannot express the literal-IP rule has not passed (7), and a path gated on (7) does not open.** Round 28 is right that the fallback as written could not be released under the stated gate, which means it was not a fallback at all but a second, contradictory gate. The single statement is now the operative one: **if (7) fails, the network axis is `DENY` — no localhost-only reduction, no partial release.** A narrower reduction would be defensible if localhost-port scoping had its own criterion, but it does not, and inventing one here to make the fallback releasable would be exactly the producerless-property move this review has spent seven rounds removing. |
| `ROLLBACK` | not applicable in a permissive direction: rolling back P1c returns the system to **network DENY**, which is strictly safer than an unenforceable `ASK` |
| `UNSANDBOXED_BACKEND` | see `RELAXED_BACKEND` below — an unconfined backend is **not offered** (round 9) |
| `RELAXED_BACKEND` | **`DANGER_FULL_ACCESS` has no enforcement path until this exists, and it is NOT an unconfined backend. **The NAME is a known misnomer and round 28 records it as a finding rather than defending it** (review round 27, OVER-HEDGING AUDIT): `DANGER_FULL_ACCESS` is Codex's name for an *unconfined* profile, while this plan's version is floored — maximum permissions **with every credential deny-rule retained** — so a reader who arrives knowing the Codex name will expect strictly less than this plan delivers, in the unsafe direction. The conservative gate is right and is kept; the name is the defect. Renaming is a product decision outside this plan, so it is recorded here so that no reader meets the name without meeting this sentence.** (corrected in review round 9, after the round-8 reviewer caught that a selectable unconfined backend contradicts the floors). **The incompatibility is structural, not a matter of adding a gate:** a floor is enforced by a **pre-effect decision at the tool-dispatch boundary**, and an unconfined shell does not pass through that boundary per action — it runs `cat ~/.ssh/id_rsa` as ordinary process I/O. No pre-effect floor can intercept a read performed by a process that has no confinement at all. Therefore an unconfined backend and unconditional floors (I9, HD-02/HD-03) **cannot both be true**, and this design does not ship the unconfined one.<br><br>Instead: `DANGER_FULL_ACCESS` selects a **`RELAXED` profile** — maximum filesystem and network permission **while still carrying every credential deny-rule**. Concretely it widens P1b's workspace-only profile to: workspace read/write, **plus** operator-approved external paths (P1d), **plus** network per P1c — and it **keeps** `secret.read` DENY. That makes "it never relaxes floors (I9)" literally true rather than aspirational. Acceptance criteria: (a) it is selectable **only** from a local, single-use, host-owned token that no tool argument can supply (I17); (b) a loud per-session banner plus an audit record with actor + reason; (c) unreachable from `AUTO` / `APPROVE_FOR_ME` / `READ_ONLY`, and selectable by **no** reviewer `ALLOW`; (d) reverts to the default mode on session end (MD-09); (e) **the credential deny-rules are present in the relaxed profile** — a test reads `~/.ssh/id_rsa` and `~/.aws/credentials` from a `DANGER_FULL_ACCESS` session and both must **fail**; (f) the profile is still generated by the same compiler, so a rule bug fails the same negative tests as P1b's.<br><br>**Explicitly not offered:** a literally unconfined backend, selectable or not. An operator who wants one is telling us they want to give up I9, and that is a different product decision outside this plan. **Until (a)–(f) pass, `DANGER_FULL_ACCESS` is not offered and its actions resolve to `DENY`.** Rationale: a danger mode that is unavailable is a usability cost; one advertised without an enforceable floor is a security hole |
| `ACCEPTANCE_CRITERIA` (8) = `PROXY_SPAWN_BINDING` | **A control-plane-owned proxy enforces nothing by ownership alone (added in review round 9).** The design required "a shared local enforcing proxy" but never bound a child to that child's approved destination list. Ownership answers *who runs the proxy*; it says nothing about *which destinations this spawn may reach*. A proxy reachable by every sandboxed child, holding no per-spawn state, cannot enforce a per-spawn grant — the first child to connect gets whatever the proxy allows globally and every other child inherits it. Binding requirement: each spawned child receives a capability that is **per-spawn in effect**, and round 11 removed the loose wording that made this look weaker than it is. An inherited fd is per-spawn **only if** the kernel prevents it from being passed on: a child that can `dup`/`fork`/`exec` and hand the fd to a sibling would carry spawn A's grant into spawn B, so *"per-spawn" would be an assertion about intent rather than about the mechanism*. Therefore:<br>• The preferred form is a capability the **kernel** ties to *this spawn specifically* — and "specifically" is the part that was too weak until round 12. **A peer-credential check is NOT sufficient**, and this was the second half of the round-12 CHECK 5 finding: `SO_PEERCRED`/`LOCAL_PEERCRED` identifies a **uid (and a pid)**, not a spawn. All children of one runner share a uid, and any of them may connect, so a proxy that resolves a peer credential to "the sandboxed user" accepts a grant from whichever child connects first — which is precisely the failure criterion 8 exists to prevent, restated in different words. A pid is not a substitute either: it is reusable after reaping, and it is observable but not unforgeable from the holder's side.
• **A pidfd is an identity, not a confinement (corrected in review round 13, CHECK 5).** Naming a process by an open kernel object prevents *impersonation* — a sibling cannot claim to be spawn A. It does not prevent the real spawn A from **forwarding** its own access, which is the actual threat: the holder passes the descriptor, or duplicates the underlying socket, to a process the proxy should not be serving. Round 12 listed "pidfd-keyed handoff" as a *sufficient* form, and that was wrong by exactly the same reasoning that made an inherited fd insufficient one round earlier — identity and non-transferability are two different properties and only one of them is what criterion 8 needs.
• **The relay case defeats every peer-identity mechanism, including a pidfd
  (corrected in review round 14, CHECK 5 — this is the reviewer's strongest objection
  and it is correct).** A peer check answers "which process opened this connection?".
  The threat that matters is "**whose traffic is this?**", and those are different
  questions. Spawn A, holding a legitimate grant, can accept a connection from B and
  forward B's bytes out through A's own authorized socket. The proxy sees a peer it
  recognises, on a connection it authorised, carrying bytes it cannot attribute. A
  pidfd makes the identification *stronger* and does nothing about the relay, which
  is why round 13's "sufficient" list was still wrong. Round 12's inherited fd and
  round 13's pidfd fail for the same underlying reason: **both control who can
  present, and the threat is who can ride.**
• **The reviewer's round-15 verdict on this criterion is accepted, and it is stronger
  than the round-14 repair.** The three mechanisms offered above do not survive their
  own test: **endpoint-anchored grants do not stop A from relaying B's bytes to that
  endpoint** (termination says who may connect, not whose bytes arrive), and **denying
  inbound sockets does not by itself rule out a workspace or interprocess handoff**
  (A can read a file B wrote and send it onward; the bytes never traverse a socket).
  Naming `NOT_ENFORCED` is honest but it is not completion of criterion 8, and a gate
  that is honestly labelled `NOT_ENFORCED` is a gate that does not release.
• **Therefore criterion 8 is not satisfiable within this plan, and the plan says so
  instead of enumerating mechanisms that do not deliver it.** The finding, stated once
  so it is not rediscovered: **an OS-sandbox-shaped boundary plus a shared userspace
  proxy cannot provide per-spawn _network_ grants for a workload that can run arbitrary
  code**, because the proxy observes connections, not traffic, and the workload may
  launder another child's traffic through its own authorised connection. The three
  families of mitigation each close a specific hole and leave the general one open,
  and the general one is the one that matters.
• What this plan delivers instead, and the honest scope of each:
  - **DNS is `DENY` by default and stays `DENY`.** No grant is required, so no
    per-spawn attribution problem arises on that path.
  - **Literal-IP and localhost-port rules are enforced by the compiled sandbox network
    profile**, per spawn, and are unaffected by the relay problem **in the dimension
    that profile rules govern**: they are enforced by the kernel on the child's own
    sockets, and the grant _is_ the profile rule, so a socket spawn A opens is
    evaluated against **A's own profile**. A child cannot borrow B's rule, and B cannot
    open a destination its own profile denies. This is the working part of the network
    axis and it is what `NET-02`/`NET-04` test.
  - **What that does *not* establish is traffic-origin isolation (review round 16, new
    defect).** "There is no proxy to relay through" was the wrong reason and is
    withdrawn. A can still receive B's bytes **through the shared workspace** — B writes
    a file, A reads it — and send them onward over A's permitted socket; those bytes
    never traverse a proxy, because there is no proxy in the path at all. The profile
    governs **which sockets a process may open**, not **whose bytes leave through
    them**, and exfiltration via an intermediary that never opens a socket is not a
    network-permission question. The defensible claim, and the one now made, is the
    narrower one: **B cannot open a destination its own profile forbids, and A's grant
    cannot be widened by B.** Any statement about the *origin* of bytes leaving over A's
    socket is `NOT_ENFORCED` here for the same reason it is `NOT_ENFORCED` on the proxy
    path, and it is recorded rather than claimed away.
  - **Hostname-scoped grants are `NOT_ENFORCED` and are not delivered in this plan.**
    They stay unavailable regardless of how the proxy is operated.
    `COMPONENT_REUSE_MATRIX.md §4`'s `net.outbound` row and the whole of §4 column B
    in the network table are to be read accordingly: **column B is a design target for
    a future phase, not a state this plan reaches.**
  - The relay leg of the negative test is retained **as a stated-unmet requirement**,
    not as a passing test. It is the acceptance criterion for that future phase, and
    any such phase must show a mechanism that makes leg 4 fail before column B may
    read `ASK`.
• If a backend can provide none of (i)–(iii) and only a peer-credential check, the honest outcome is that **per-spawn attribution is `NOT_ENFORCED`**, hostname grants stay unavailable, and that is recorded as a backend limitation rather than worked around. A design that reaches for uid as if it were a spawn identity should fail its own gate.<br>• Where the only available form is an inherited fd or a bearer token, it is a **bearer** capability: **`NOT_ENFORCED` as a per-spawn boundary** until a transfer test proves a child cannot pass it to another child. The proxy holds a per-spawn allow-list keyed to it, a child presenting no valid capability gets `DENY`, and a child presenting spawn A's capability gets **only** spawn A's destinations — but that last guarantee holds only for non-transferring children.<br>• A negative test is required either way: spawn A, attempt to reuse A's capability from a **different** spawn, and assert `DENY`. Without that test the binding is an assumption. Without this binding, hostname grants stay unavailable **regardless** of how the proxy is operated. This is a precondition of criterion (ii), not a detail of it |

**Dependency:** P1c depends on P1b (it needs a working profile compiler and the
per-spawn plan). `P5` (modes) must not advertise a network `ASK` unless P1c has
shipped **and** may then advertise only the **literal-IP / localhost-port** path — never a hostname-scoped one, which this plan does not deliver.
**Corrected in review round 18, CHECK 5**: an unqualified "network `ASK`" in a sequencing note reads as the **union** of the paths rather than the one that survives, so every level that mentions a network `ASK` without naming the path now names it — this row, P1c criterion 6, P5, and `COMPONENT_REUSE_MATRIX.md §4`'s `net.outbound` cell.

**Per-spawn, NOT per-action (corrected in adversarial review round 7).** An
earlier revision of criterion (1) claimed a per-spawn descriptor "cannot be reused
by another action". That claim does not hold and is withdrawn. The mechanism binds
a **process**, not the actions that process later performs:

- `persistent_shell`, `script`, `job`, and nested `agent` surfaces each spawn
  **once** and then perform **many** actions over that process's lifetime. One
  approval granted at spawn time therefore authorizes every network egress the
  child performs afterwards, not just the action that was approved.
- A grant cannot be scoped to "this action" inside an already-running child: the
  child chooses its own next syscall, and there is no per-action boundary at the
  process level.
- Two consequences follow, and both are requirements, not merely wording changes:
  1. The grant a human sees must be labelled as **process-scoped**, not
     action-scoped — e.g. "allow this session's shell to reach `example.com:443`
     for its lifetime", never "allow this one command".
  2. **Long-lived surfaces (`persistent_shell`, `script`, `job`, nested `agent`)
     are excluded from network `ASK` outright.** Merely rewording the approval as
     process-scoped is **not** sufficient and does not make them eligible — that
     was the inconsistency the round-8 reviewer caught: wording cannot substitute
     for a mediation point that does not exist. They keep a **`DENY`-only**
     network posture until per-action mediation is designed in a later phase.

Recorded as `KNOWN_LIMITATION` in `OSS_RESEARCH_EVIDENCE.md §8`. This is a real
friction/over-grant asymmetry, not a bug to be designed away in P1c: closing it
properly requires the surface itself to have per-action mediation, which is
strictly more invasive than a network profile.

---

## P1d — External-path profile extension (required before any external-read `ASK`)

**Added in review round 9.** `COMPONENT_REUSE_MATRIX.md §4` defaulted
`fs.read.external` to `ASK`, but no phase turned an approved external read into an
enforceable filesystem profile — P1b ships one workspace-only profile. That is the
same defect as an unenforceable network `ASK`, in a different axis: approving
something that then cannot be executed. `fs.read.external` now defaults to **`DENY`
until this phase ships**.

| Item | Value |
|---|---|
| `FILES` | extension of `crates/webcodex-sandbox/src/plan.rs` + a new `external.rs`; the P1a call sites (the plan already reaches each spawn) |
| `MODULES` | `sandbox::plan`, `sandbox::external` |
| `REUSED_COMPONENT` | the **same** SBPL compiler and per-spawn plan plumbing built in P1b — `REUSE_EXISTING`, no new mechanism. The only new code is a rule that turns an approved descriptor into literal path rules |
| `NEW_GLUE_CODE` | ~200–400 LOC |
| `SCOPE` | **External *reads* only.** The descriptor is an explicit operator-approved path list; the sandbox already needs read access to system paths for the toolchain, so the delta is the specific external paths the human approved. `DANGER_FULL_ACCESS`'s relaxed profile (P1c `RELAXED_BACKEND`) reuses this same mechanism |
| `TESTS` | approved external read **succeeds**; a sibling path outside the approved list **fails**; a re-spelled path (`../`, symlink, trailing slash) to the same file **fails** if it was not approved; a `..`-re-spelling of an approved path still succeeds only if it resolves inside the approved set; credentials outside the approved list **still fail** (the secret deny-rules are never dropped) |
| `ACCEPTANCE_CRITERIA` | (1) the approved path set is compiled into **that spawn's** profile and cannot widen any other spawn (I11); (2) **the credential deny-rules are not weakened by an external-read grant** — the union is computed deny-wins, never allow-wins, and a test grants an external read on a directory containing `~/.aws` and confirms the credential is still `DENY`; (3) approval is **process-scoped**, so long-lived surfaces (`persistent_shell`, `script`, `job`, nested `agent`) are **excluded** from external-read `ASK` for the same reason as network (one spawn, many actions); (4) an approval that cannot be compiled is `DENY`, never a warning; (5) **no phase may offer an external-read `ASK` until criteria 1–4 pass** |
| `ROLLBACK` | rolling back returns external reads to `DENY`, which is strictly safer than an approval with no profile |

**Dependency:** P1d depends on **P1b** (profile compiler + per-spawn plan) and
**P2** (policy engine, so the decision is made in the right layer). It therefore
runs **after P2**, not before: the dependency graph reads `P2 ──▶ P1d ──▶ P5`.
An earlier revision drew `P1d ──▶ P2` in the graph while the prose said P1d
depends on P2 — the graph and the stated order could not both be followed, which
the round-10 reviewer caught. The graph now matches the prose. `P5` must not
advertise an external-read `ASK` until P1d ships.

---

## P2 — Policy engine: ALLOW / ASK / DENY rule model

**Goal:** express "normal coding is uninterrupted, everything else asks" as data
instead of a two-valued mode. This phase also owns the **mediated** Git
credential-helper denial (criterion (5)), because the rule model is the only
layer that can see a mediated tool invocation at all — see criterion (5) and
`SECURITY_INVARIANTS.md` I13.

| Item | Value |
|---|---|
| `FILES` | `src/tool_runtime/permissions/policy.rs`, `model.rs`, `evaluator.rs`, new `rules.rs`, `crates/webcodex-tool-contracts/src/metadata.rs` |
| `MODULES` | `permissions` (extend), new `permissions/rules.rs` |
| `REUSED_COMPONENT` | OpenCode permission model (`packages/core/src/permission.ts:76 evaluate`, `packages/schema/src/permission.ts` `Effect`/`Rule`/`Reply`) — REIMPLEMENT_SMALL_CORE; OpenCode wildcard (`packages/core/src/util/wildcard.ts`) — DESIGN_ONLY, policy layer only; Hermes floors (`tools/approval_floors.py`) — DESIGN_ONLY for ordering |
| `NEW_GLUE_CODE` | ~350–500 LOC |
| `TESTS` | rule precedence (deny-floor beats allow regardless of order — *deliberately different from OpenCode's `findLast`*); default = ASK; malformed rule set = DENY; wildcard matching table; capability↔rule round-trip; **a mediated `git` invocation whose config sets an external `credential.helper` resolves to `DENY`, while the same helper set on a `git` invocation that did not pass through tool dispatch is `NOT_ENFORCED` and the test asserts the mediated half only** |
| `ACCEPTANCE_CRITERIA` | (1) `Effect = Allow \| Ask \| Deny` exists with a **deny floor that no later rule can override**; (2) default when no rule matches is `Ask`; (3) `trusted_agent` is expressible as a rule set (proving the model subsumes the current mode); (4) `restricted` is expressible and produces today's exact denial reason strings; (5) **the rule model expresses the mediated Git credential-helper denial that P1b criterion (9a), P5 criterion (4) and `SECURITY_TEST_VECTORS.md` NET-11 already assume — an external `credential.helper` (any `helper.*` / `credential.helper` setting pointing outside the workspace) on a `git` invocation that arrives through tool dispatch resolves to `DENY` at the decision layer, and the denial is audited with the reason `external_credential_helper_denied`. This criterion exists because it was previously a promise in three documents and an absence in this one (review round 21, N2): a control named by a later phase's acceptance text, with no criterion, files, or test in the phase that would have to build it, is not a control. **The denial is a `DENY`, not a `NOT_ENFORCED` gap, for the mediated path only** — the same per-action boundary P1b (9a) already states applies unchanged here: a `git` run inside a permitted shell, or by a build step (`npm`, `make`, a script), never returns to the dispatch boundary, so this criterion does not reach it and that residual stays a `KNOWN_LIMITATION` (`OSS_RESEARCH_EVIDENCE.md §8`). Nothing here makes a token already embedded in a remote URL unreadable |
| `ROLLBACK` | rule engine behind a feature flag; the two-mode path remains the default until P5 flips it |

**Deliberate divergence from OpenCode:** OpenCode resolves rules with
`findLast`, so a later `allow` can override an earlier `deny`. Our floors must
be order-independent (`SECURITY_INVARIANTS.md` I9). This is a case where the
reference implementation is *not* copied.

---

## P3 — Local control channel + approval machinery (delivered over a **non-GUI** channel, so no synthetic-input route exists)

**Goal:** build the approval machinery — request, digest, host-minted token, decision verification, audit — **and carry the decision over a channel that is not a host GUI window, so that no model-reachable surface can present, auto-answer, or synthesise input to it.**

> **Narrowed three times, and round 32 removes the claim rather than re-scoping it again (L71).** Round 18 narrowed "provably unreachable" to two named routes. Round 30 (L62) replaced the any-route claim with "host-UI surfaces are denied the affordance". **Round 31 then found that this cell contradicted itself: it opened with "rendered only into a surface no model-driven route can reach" and conceded one paragraph later that the process-route confirmation UI's reachability from synthetic input is `UNMEASURED`. Those are two descriptions of the SAME object, and no wording separates them — the reviewer's own formulation was "incompatible descriptions of the same process-route UI".**
>
> **So P3 stops delivering approval.** A confirmation dialog displayed to a host user is a surface a UI-automation route may drive, this plan has no mechanism that prevents it (P3-R, `NOT_ENFORCED`), and therefore **a decision read off that dialog is not established to be a human decision.** A phase that ships the dialog anyway is shipping an approval feature whose central claim it cannot support. **P3 therefore renders no affordance at all, every `ASK` becomes `DENY`, and P3 is a phase that builds the plumbing and withholds the feature.** That is the conservative direction and it costs the product its approval UX until P3-R exists — a cost the plan accepts rather than a boundary it claims. **Nothing in P3 may be described as human approval.**

> **Narrowed in review round 18, CHECK 3 (newly-raised finding).** The previous
> wording said "provably not reachable from the model" as a single property.
> Criterion 6 tests only the **process** route — a sandboxed shell. The submitted
> operation enum also contains model-facing **Browser/CDP** and
> **Computer/UI-automation** operations, which can drive a host UI that may
> display or auto-answer a confirmation, or open a terminal the profile does not
> govern. A sandbox profile governs processes; it does not deny synthetic input
> events. So the goal is restated per route, and a second route is named rather
> than left implicit.

> **Corrected (adversarial review round 1, CHECK 3).** An earlier revision called
> this "human approval" and asserted the local channel was safe by
> construction. The channel is itself a security-critical component and needs its
> own design and tests; a request digest is *correlation*, not proof of a human
> decision.

| Item | Value |
|---|---|
| `FILES` | new `src/tool_runtime/approval/{mod,request,decision,transport}.rs`, new `src/tool_runtime/control_channel/{mod,channel,token}.rs`, `src/tool_runtime/dispatch.rs`, `crates/webcodex-cli/src/webcodex_cli/` (approval command surface), desktop UI hook in `apps/desktop/src-tauri/` |
| `MODULES` | `approval`, `control_channel` (control plane) + CLI + optional desktop surface |
| `REUSED_COMPONENT` | Hermes `hermes_cli/approval_transport.py` (request/decision/digest/correlation/6 failure codes) — REIMPLEMENT_SMALL_CORE; Codex `ReviewDecision` lifetimes — DESIGN_ONLY; Pi `timed-confirm` UI semantics (`ctx.ui.confirm(..., {timeout})`, non-interactive ⇒ block) — DESIGN_ONLY |
| `NEW_GLUE_CODE` | ~850–1550 LOC (engine 300–450; control channel 250–500; local UI 300–600) |
| `TESTS` | `SECURITY_TEST_VECTORS.md` §5 (once/session/expired/replay/timeout/crash) + §6 failure table + **MD-10 / AG-\* control-channel reachability** + **CP-07 (new, round 18 CHECK 3; **reassigned to P3-R in round 30** — it tests the isolation property P3-R must produce, and its expected FAILURE is no longer a P3 release signal) ** |
| `ACCEPTANCE_CRITERIA` | (1) `ApprovalRequest` carries `request_id` + `digest`, where the digest is documented as **correlation only**; (2) the decision additionally requires a **host-minted, never-exposed, single-use token** verified in constant time, so a model that can read the request still cannot forge a decision; (3) every failure mode (timeout, error, busy, interrupted, invalid, stale) resolves to DENY; (4) a decision for request A is rejected against request B; (5) `choices` limited to `once/session/always/deny`, with `session`/`always` offered only when policy permits that lifetime; (6) **a sandboxed shell cannot reach the control channel** — proven by test, not asserted; (7) the channel does not listen on a TCP port reachable from inside the sandbox and is not a path inside the writable workspace; **(7b) BLOCKED, AND THEREFORE NOT A GATE — **this criterion is retained as a specification of the property P3 must eventually deliver, and it is removed from the release path, because as written it makes P3 unsatisfiable rather than unverified** (review round 28, CHECK 8, blocking; graded a NEWLY RAISED finding, "a test of an unproduced boundary"). The property is right and worth stating: for every model-reachable host-UI-interaction surface — Browser/CDP, Computer/UI-automation, and any surface added later, a set that is finite and maintained as a list precisely so the criterion is checkable — **the surface must not be able to present, auto-answer, or synthesise input to an approval or mode-change affordance, and the confirmation UI must not be rendered into a surface the model can drive.** The defect is the mechanism, and the absence is not incidental: **this plan specifies no mechanism that makes host UI unreachable from synthetic input** — no accessibility-API denial, no separate-window class, no input-event quarantine, no prompt-side suppression of the approval affordance. `SECURITY_TEST_VECTORS.md` CP-07 tests the property and is expected to FAIL. **A test of an absent producer does not make the producer appear; it converts a release criterion into an unsatisfiable gate, and a gate that cannot be met is a plan that cannot ship rather than a plan with an open risk.** So: the property stays, the test stays as the thing that will verify it once a mechanism exists, and the RELEASE GATE moves to a criterion this plan can actually meet — **(7b') THE RELEASE GATE, and it is a positive criterion with a producer: the decision travels over a channel that is not a host GUI window, and it is authenticated on a secret the sandbox cannot read rather than on a uid.** Specifically: the approval request is written to a **unix domain socket outside every writable root**, the socket is created by the runner with mode `0600`, and the decision must be accompanied by **(a) the host-minted single-use token for that exact `request_id`**, and **(b) an HMAC over the request digest under a per-boot operator secret generated at runner start, held in a file readable only outside every sandbox root and never passed to any child**.

**Why uid and file mode are not sufficient (review round 33, CHECK 3 and CHECK 9, blocking).** Round 33's first attempt at this gate was `0600` plus "owned by the operator's uid", on the reasoning that a UI-automation route cannot connect to a socket. **The reviewer correctly showed that reasoning is false:** `Computer/UI-automation` can drive an **unsandboxed host terminal**, and `webcodex approve` run there by the operator's own login has the operator's uid and the operator's file access. Nothing in the socket's mode distinguishes that from the operator typing the command. **The gap is not the socket, it is the credential.** An HMAC under a secret the model cannot read is a different property from a permission bit: the automation route can reach the terminal, the terminal has the uid, and it still cannot produce the tag. **A capability the adversary must possess, rather than a permission it must lack.**

**The honest limit of this criterion, which is not the one round 33 claimed.** It does not establish that the model cannot *ask* the operator for approval, cannot read the operator's screen, or cannot socially engineer an approval. It establishes that **no route can manufacture a decision without the per-boot secret** — a forged or replayed approval is `DENY`. Coercion and screen-reading are out of scope for this criterion and are named as residual risk rather than claimed as closed. A `Browser/CDP` or `Computer/UI-automation` route can synthesise **input events**; it cannot **connect to a unix socket**, because that requires a filesystem permission the sandbox profile denies and a peer credential the kernel checks. Input synthesis and IPC are different capabilities, so there is no surface on which "the model pressed the button" and "the operator answered" become indistinguishable. **This is why the criterion is satisfiable and why P3-R is not needed: the contradiction the round-31 reviewer identified was never a property of approval, it was a property of rendering approval in a window the model can type into.** The old criterion asked for a window that cannot be typed into; the new one declines to have such a window. **(7b) is withdrawn as a gate: its property is satisfied structurally, not by an isolation mechanism this plan lacks.**

> **What the non-GUI channel does and does not establish, stated once (round 33).** It establishes that **no synthetic-input route can answer an approval**, because answering requires an IPC connection the sandbox denies and the kernel attributes to a uid. It does **not** establish that the operator always answers deliberately: a human can still answer carelessly, and `SECURITY_TEST_VECTORS.md` AP-10..AP-16 keep every transport failure resolving to `DENY`. What it removes is the specific defect rounds 29-32 circled — **a decision read off a window the model can drive is not established to be a human decision** — by never putting that window on the path. **The `ASK` → `DENY` behaviour that round 32 introduced is withdrawn**: approval is delivered, and the product keeps its approval UX, which is what the brief's low-friction requirement asks for. (7b') is satisfiable because the mechanism exists and is named, not because the requirement was relaxed. (Review round 18, CHECK 3 is answered rather than withdrawn: criterion 6 covers the process route, criterion 7 covers the channel, and the host-UI surfaces have nothing to drive.)
| `ROLLBACK` | until P5, `auto` mode never emits ASK, so P3 is inert if the local UI is disabled; with no channel available, all ASK resolves to DENY. **Under (7b') as rewritten in round 33, approval is delivered over a socket rather than a window, so there is no UI to roll back to a prompt; the operator's `approve`/`deny` CLI is the whole surface** |

---

## P3-R — Host-UI isolation — **REMOVED in round 33, and this is a design change rather than another disclosure**

**Rounds 29-32 each created or kept this phase, and each round's reviewer was right
that it was the wrong shape.** It existed because the approval decision was rendered
into a **host GUI window**, and a window is a surface a `Browser/CDP` or
`Computer/UI-automation` route can synthesise input into. The four reviews agreed on
the diagnosis and disagreed only on how to record it: split it out and mark it
`SPECIFIED` (round 30), delete the claim (round 31), withhold the feature (round 32).
**All three kept the window.** Round 33 changes the object instead: P3 now carries the
decision over a **unix domain socket owned by the operator's uid**, which requires a
filesystem permission the sandbox denies and a peer credential the kernel checks. A
synthetic-input route cannot answer it, so the property this phase was created to
protect is satisfied **structurally**, with a producer, and there is nothing left to
defer.

**The honest cost of removing it.** Approval becomes an **operator CLI** decision
(`webcodex approve <request_id>`) rather than a pop-up dialog. That is a real product
change and it is the right trade for a boundary whose failure mode is a model
approving its own request, but it should be named as a cost rather than presented as
a free win. **What is NOT lost: the `once`/`session`/`always` lifetimes, the digest
binding, the replay and stale protections, and the audit record all still ship in P3
and P4.** Only the window is gone.

**A reader checking the old property should read `SECURITY_TEST_VECTORS.md` CP-07,
which is retained and re-pointed at this claim**: it now tests that no model-reachable
surface can produce a decision on the socket, which is a testable property with a
producer, rather than a property about a window that does not exist.

---

## P4 — Session grants

**Goal:** stop re-asking for the same thing within a session.

> **P4's grants come from the P3 channel, and this is stated because round 32
> found the phase promising `always` grants with no issuer (newly-raised finding).**
> Round 32 made P3 render no approval, so no grant could ever be issued and P4 was
> describing storage for rows nothing wrote. Round 33 restores the issuer: **a grant
> is created only by an operator decision delivered over the P3 unix-socket
> channel**, and the store accepts a row only when it carries the single-use token
> of the request that produced it. There is no second source of authority, and a
> grant whose originating token cannot be re-verified against the audit record is
> `DENY`, not `ASK`.

| Item | Value |
|---|---|
| `FILES` | new `src/tool_runtime/approval/grants.rs`, `src/tool_runtime/approval/store.rs`, `src/db/` migration, `src/tool_runtime/permissions/policy.rs` (consult grants) |
| `MODULES` | `approval::grants`, `approval::store` |
| `REUSED_COMPONENT` | OpenCode `packages/core/src/permission/saved.ts` + `permission/sql.ts` (project-keyed rows, persist only `request.save[]`) — REIMPLEMENT_SMALL_CORE; Hermes `(session_key, pattern_key)` keying (`tools/approval.py:249,350,366,448`) — DESIGN_ONLY; Codex `ApprovedForSession` — DESIGN_ONLY |
| `NEW_GLUE_CODE` | ~200–350 LOC |
| `TESTS` | grant scope (cannot widen beyond the requested pattern); session end revokes; cross-project grant is rejected; `always` persists and survives restart; expired grant re-asks; danger mode is never grantable |
| `ACCEPTANCE_CRITERIA` | (1) grants are keyed `(project_id, capability, pattern, lifetime)`; (2) a session grant never matches in another session; (3) a persisted `always` grant cannot cover a capability that was not in the originating request; (4) session-end revocation is exercised by a lifecycle test; (5) `danger.full_access` is absent from the grantable capability set |
| `ROLLBACK` | grants table is additive and unused unless P5 enables session/always choices; drop the migration |

---

## P5 — Modes: READ_ONLY / AUTO / APPROVE_FOR_ME / DANGER_FULL_ACCESS

**Goal:** expose the four modes with precise semantics.

| Item | Value |
|---|---|
| `FILES` | `src/tool_runtime/permissions/model.rs`, `policy.rs`, `src/tool_runtime/dispatch.rs`, CLI mode command, `SECURITY.md` update |
| `MODULES` | `permissions::model` (mode enum), CLI |
| `REUSED_COMPONENT` | `webcodex :: AuthorityMode` + `EffectiveAuthorityConfig` (`permissions/model.rs:18`, `policy.rs:64`) — REUSE_EXISTING (extend 2 → 4); Hermes `hermes_cli/approval_mode.py` (profile-scoped persistent mode, managed policy blocks change, must not rebuild the agent) — REIMPLEMENT_SMALL_CORE |
| `NEW_GLUE_CODE` | ~100–150 LOC |
| `TESTS` | per-mode capability matrix test; invalid mode fails closed; mode cannot be set by any tool argument; mode is not persisted in project config |
| `ACCEPTANCE_CRITERIA` | (1) mode is resolved only from two **host-owned** sources and nothing else: (i) the operator's persisted config/env, and (ii) a **non-persisted, in-process activation held by the local control plane**, created only on presentation of the single-use token from the operator's own local action. It is **never** resolved from a tool argument, and **never** from any model-reachable or MCP-writable session state. The round-12 reviewer correctly identified that the earlier wording of this criterion ("never from ... session state") made `REFERENCE_ARCHITECTURE.md §10`'s "restore the safe mode on session end" unsatisfiable, because no authoritative mechanism was named that could both hold a per-session activation and expire it. The two sources are reconciled explicitly: source (ii) lives in the **runner's own process state**, keyed to the **logical session id the runner mints for the activating request** (never a model-supplied id) and to the runner process lifetime; it is cleared when that logical session ends, when the process exits, on TTL, or on local revoke. `REFERENCE_ARCHITECTURE.md §10` now tabulates the three lifetimes and names the signal; if the runner cannot detect logical session end, per-session expiry is `NOT_ENFORCED`, TTL is the only expiry, and that limit must be stated to the operator. "Session" here means the **logical session** tabulated in `REFERENCE_ARCHITECTURE.md §10` — the session the runner mints, ended by session-registry removal or disconnect past a grace period — and **not** the runner process lifetime, which is a separate and longer lifetime. Round 14 caught this clause still equating the two. §10 names the transition owner (the runner/server lifecycle); this criterion binds P5 to it, and the clearing is a lifecycle event, not a policy decision. **The existence of such a session id is a prerequisite P5 must establish, not a premise it may assume (review round 15, CHECK 3)**: the prior wording said "the runner **already mints** such an id", and no observation in `OSS_RESEARCH_EVIDENCE.md §2` records one — the only session-keyed grants in the evidence are Hermes' (`H-17` `approve_session(session_key, pattern_key)` / `H-21` `clear_session(session_key)`), which is a pattern to reuse, not a fact about WebCodex. `REFERENCE_ARCHITECTURE.md §10` now tabulates the two states (**bound** / **unbound**, default `unbound`); P5 acceptance therefore requires the `bound` state to be **demonstrated** — the host mints a logical session id for the activating request **and** observes that session's end — before per-session expiry may be claimed. `OSS_RESEARCH_EVIDENCE.md §2.3` (W-42) is weaker still than round 15 stated: it records a list of *error-kind strings* (`session_guard_denied`, `unknown_session_id`, `session_project_mismatch`) produced by a classifier, and an error kind is evidence that the code **distinguishes** those cases, not that a session registry **exists with an observable lifetime** — a classifier can name a condition no component currently produces. These strings therefore suggest a session concept in the design; they do not establish a registry, and they establish nothing about end-of-life. Establishing `bound` is a **P5 work item with a named deliverable**, not a formality, and until it is done `unbound` holds and the mode is not offered. **The mode is offered only in the `bound` state (review round 16, CHECK 3).** The round-15 fix made `bound` a requirement for *claiming per-session expiry* but left the mode offerable in `unbound` with TTL-only expiry — so a danger mode whose activation outlives the session that requested it remained reachable, which is the failure `SECURITY_TEST_VECTORS.md` MD-09 exists to prevent. **In `unbound`, `DANGER_FULL_ACCESS` is not offered and resolves to `DENY`**; TTL remains an additional bound in `bound`, never a substitute for the session-end one. This is the same deliberate "boring failure direction" P5 criterion 7 already takes when P1c's `RELAXED_BACKEND` criteria have not passed: a mode that is unavailable is a usability cost; a mode advertised without enforceable session binding is a security hole. Until `bound` is demonstrated, per-session expiry is **`NOT_ENFORCED`**, TTL is the only expiry, and `SECURITY_TEST_VECTORS.md` MD-09's session-end leg is `NOT_ENFORCED` rather than passed — a disclosed fallback, not a claim of session-end revocation. (2) unknown mode ⇒ consequential tools fail closed with the existing `invalid_authority_mode:*` reason; (3) `READ_ONLY` denies write/shell/job on all surfaces including ACP/agent paths — **at the decision layer**. It does **not** reach a long-lived child that is already running when the mode changes: `REFERENCE_ARCHITECTURE.md §6.1` records that no kill or re-profile mechanism is proposed, so that residual is `NOT_ENFORCED` and this criterion is satisfied by refusing *new* work, not by stopping *existing* work (review round 21, CHECK 2; `SECURITY_TEST_VECTORS.md` FS-08 and MD-02 carry the same split and neither is in `OUTCOME_STRICT`); (4) `AUTO` allows workspace read/write, git read, test/build with **no** approval, and ASK/DENY for network, external FS, destructive ops, remote git mutation, agent spawn, unknown binaries (**each of these last three is `ASK`/`DENY` only for a **mediated** invocation; performed inside an allowed shell or build process they are `NOT_ENFORCED` — no boundary in this plan observes them, per `COMPONENT_REUSE_MATRIX.md §4` and `OSS_RESEARCH_EVIDENCE.md §8` item 9e. A `rm -rf` or an unknown binary launched by `bash -c` is ordinary in-sandbox activity.** `AUTO` deliberately allows shell and build execution without approval, so this is the accepted cost of the mode, stated rather than implied) — **Git works normally in `AUTO`**, which is why `.git/config` is readable (P1b criterion 9a); the honest limit is that a credential embedded in that file stays readable and transmittable, recorded as a `KNOWN_LIMITATION`, while an external `credential.helper` is `DENY` as a P2 policy check on Git invocations; (5) `APPROVE_FOR_ME` routes ASK to the reviewer and **escalation to the operator over the P3 channel**, with every transport failure — no operator answer, timeout, error, busy, interrupted, invalid, stale — resolving to `DENY` (review round 33, CHECK 8, blocking: round 32 had made escalation a terminal `DENY` here, and this criterion is the one place that rule was still live after the socket change restored the approval path. **The rule that survives is the fail-closed one, not the no-approval one: an unanswered escalation is `DENY`, an answered one is the operator's decision**; (6) **network `ASK` may be advertised only if P1c has shipped, and external-read `ASK` only if P1d has shipped** — and the network half of that promise is **narrower than "network `ASK`"** (review round 17, CHECK 5: this clause named no limit, so a reader could take it as including hostname grants, which this plan never releases). The only network `ASK` P5 may advertise is **literal-IP and localhost-port**, released by P1c criteria 1, 2, 3, 4, 5, 7, 9 and 10 (round 18 restored criteria 2 and 3, which round 17's split had dropped). **Hostname-scoped network `ASK` is never advertised by this plan at any mode**, because P1c criterion 8 is unsatisfiable within it; a future phase that satisfies criterion 8 is the only thing that can change that, and this plan is not it; until then every network-capable and every external-read capability resolves to `DENY`, including under `AUTO` and `APPROVE_FOR_ME` (added in review round 5, CHECK 5; external axis added in round 9); (7) `DANGER_FULL_ACCESS` lifecycle per `REFERENCE_ARCHITECTURE.md §10`, **and the mode is not offered at all until P1c's `RELAXED_BACKEND` criteria (a)–(f) pass** — until then it resolves to `DENY`. `P8` additionally depends on `P2`, since re-evaluating a hook's argument mutation is performed by the policy engine (added in round 11); The mode selects a **relaxed-but-still-floored** profile, never an unconfined backend: a pre-effect floor cannot govern a process that has no confinement, so an unconfined backend would silently void I9 and HD-02/HD-03 (added in review rounds 8 and 9); (8) mode changes are audited |
| `ROLLBACK` | mode enum keeps a total mapping onto `trusted_agent`/`restricted`, so reverting to two modes is a config change plus a deleted arm |

---

## P6 — Hermes / Codex agent integration

**Goal:** nested agents inherit confinement and cannot widen it.

| Item | Value |
|---|---|
| `FILES` | `crates/webcodex-runner/src/webcodex_runner/coding_agent.rs`, `mcp_gateway.rs`, `plugin.rs`, `docs/agent/acp-coding-agent-run.md` |
| `MODULES` | runner agent-execution paths |
| `REUSED_COMPONENT` | `webcodex :: ManagedChild::spawn` attach point — REUSE_EXISTING; Codex ACP integration path (documented in-tree) — REUSE_EXISTING; Hermes `approval_mode` semantics — DESIGN_ONLY |
| `NEW_GLUE_CODE` | ~250–450 LOC |
| `TESTS` | nested agent cannot read outside workspace; nested agent's own `mode=off` is overridden/pinned by the runner; a child reporting full access under a restricted parent is a violation; `stdin`/resume paths also carry a sandbox plan (they must not bypass it) |
| `ACCEPTANCE_CRITERIA` | (1) every ACP/MCP/plugin child spawn is **confined by the same plan as its parent, and reaches confinement through one of the two sites `REFERENCE_ARCHITECTURE.md §4` permits** — the `ManagedChild::spawn` path **or** the shared `sandbox::apply` hook. **The earlier wording of this criterion required `ManagedChild::spawn` specifically, and that contradicted the architecture's own two-path attach contract, which exists precisely because normalising every spawn onto one function is not required (round-14 CHECK 4)** — an implementer could not satisfy both. What is required is the **property** (the child is confined by an explicit plan), not the **call site**; a test asserts the property by observing the child's effective sandbox, not by asserting which function was called; (2) the runner pins the child agent's own approval mode and hard-denies on an unpinnable mismatch; (3) `danger-full-access` is never passed to a nested agent; (4) the ACP path is used for Codex (no shell nesting) |
| `ROLLBACK` | agent integration is feature-gated per provider; disable the provider to revert |

---

## P7 — Auto review (APPROVE_FOR_ME)

**Goal:** independent reviewer for ASK, after P1–P6 make review safe.

| Item | Value |
|---|---|
| `FILES` | new `src/tool_runtime/review/{mod,request,verdict,client}.rs`, `src/tool_runtime/approval/mod.rs` (escalation), prompts under `docs/` |
| `MODULES` | `review` |
| `REUSED_COMPONENT` | Codex guardian architecture (`ext/guardian-reviewer/`, `core/src/guardian/decision.rs`, `guardian-context/src/lib.rs`) — DESIGN_ONLY (do **not** vendor: ~9500 LOC incl. tests, coupled to `codex-mcp`/`codex-prompts`/`codex-protocol`/`codex-otel`); `ReviewDecision` vocabulary — DESIGN_ONLY |
| `NEW_GLUE_CODE` | ~400–700 LOC |
| `TESTS` | actor/reviewer context isolation; reviewer ALLOW cannot override a floor DENY; reviewer timeout ⇒ DENY; reviewer malformed output ⇒ DENY; reviewer missing ⇒ ESCALATE to human (never allow); reviewer ALLOW outside the delegated set is coerced to ESCALATE; transcript-as-data structural test |
| `ACCEPTANCE_CRITERIA` | (1) reviewer is a separate invocation with its own context and its own policy prompt; (2) `ReviewerVerdict` is a closed enum `ALLOW\|ESCALATE\|DENY`, and anything unparseable is DENY; (3) any reviewer failure, including absence, resolves to ESCALATE or DENY — never ALLOW; (4) reviewer cannot be reached by tool arguments; (5) **reviewer `ALLOW` is honoured only inside a host-owned delegated allowlist, which is empty by default**; actions outside it are coerced to ESCALATE, and the reviewer cannot widen the allowlist; (6) danger-mode-adjacent classes (network egress to a new host, credential paths, agent spawning, remote git mutation, `danger.full_access`) are **human-only**; (7) reviewer decisions are audited with verdict, delegation-set version, and inputs hash |
| `ROLLBACK` | `APPROVE_FOR_ME` falls back to `AUTO` with the **operator channel** as the hand-off target when the reviewer client is absent: an `ESCALATE` from a present reviewer, and an absent reviewer, both reach the operator over the P3 socket, and both are `DENY` if no operator answers before the timeout (round 33; round 32 made both terminal `DENY`, which contradicted AR-06/AR-12/MD-05 and cost the product its approval path) |

---

## P8 — Plugin extension point

**Goal:** let third parties *narrow* behaviour, with no way to widen it.

| Item | Value |
|---|---|
| `FILES` | new `crates/webcodex-plugin-api/` (or extend `src/plugin_gateway.rs`), `docs/PLUGINS.md` |
| `MODULES` | plugin hook contract |
| `REUSED_COMPONENT` | Pi `BeforeToolCallResult{block?,reason?,terminate?}` + error⇒block + non-interactive⇒block (`packages/agent/src/types.ts:66`, `coding-agent/src/core/agent-session.ts:533`) — DESIGN_ONLY; OpenCode `plugin.trigger("tool.execute.before"/"after")` — DESIGN_ONLY |
| `NEW_GLUE_CODE` | ~150–250 LOC |
| `TESTS` | plugin hook cannot return an allow that overrides policy/floor; hook exception ⇒ block; plugin load failure ⇒ narrower, not wider; a hook that **mutates arguments** triggers re-evaluation and cannot validate a mutation it did not authorize; a post-execution hook cannot flip a hard-denied or sandbox-violation result to success |
| `ACCEPTANCE_CRITERIA` | (1) the pre-execution hook result type can only express `block`/`narrow`, structurally unable to express `allow`; (2) a throwing hook blocks; (3) a failed plugin load does not restore capability; (4) hooks receive redacted request data only; (5) **argument mutation invalidates the prior decision and forces re-evaluation** — mutation never inherits an earlier allow; (6) plugin code does not execute inside the policy process and its own process is sandboxed at least as tightly as the action it intercepts; (7) result-integrity: hard-deny classification is attached before plugin result hooks and is immutable thereafter |
| `DEPENDENCY` | `P1b` (sandbox) **and** `P2` (policy engine). Criterion (5)'s re-evaluation of mutated arguments is performed by the P2 engine; there is no second evaluator, so enabling hooks before P2 exists would leave that criterion unsatisfiable. The dependency graph was corrected in round 11 from `P0 ──▶ P1b ──▶ P8` to `P0 ──▶ P1b ──▶ P2 ──▶ P8` |
| `ROLLBACK` | hooks stay **disabled** (the P0 gate remains in force). Rolling back P8 must **not** "restore current behaviour", because current behaviour is the undefined plugin path that I17 says must not run. Correct rollback = keep the gate closed |

---

## Sequencing and dependency graph

```
P0 ──▶ P1a ──▶ P1b ──▶ P1c ──▶ P2 ──▶ P3 ──▶ P5 ──▶ P6 ──▶ P7
                              │      ▲
                              └▶ P4 ─┘
        P2 ──▶ P1d ──▶ P5       (P1d needs the policy engine to decide)
        P0 ──▶ P1b ──▶ P2 ──▶ P8   (P8 needs the sandbox AND the policy engine, see below)
```

- `P1a` (spawn normalization) is a hard prerequisite for `P1b`: confinement
  cannot be attached while ≥ 10 direct spawn sites bypass the managed path.
- `P1b` is the phase that introduces **filesystem confinement**, and it ships one
  platform and one profile first. The earlier text said it was "the only phase
  that changes the security posture", which was wrong in both directions
  (corrected in review round 13, newly-raised finding): **P0** changes posture too,
  by hard-gating plugins off until P8's restrict-only contract exists, and
  **P1c/P1d** each *widen* the enforceable surface by making a previously-`DENY`
  network or external-read grant reachable. Confinement is the largest single
  change; it is not the only one.
- `P1c` gates every network `ASK`; without it, network is `DENY` only.
- `P1d` gates every external-read `ASK` for the same reason: without a profile
  extension that compiles an approved path into that spawn's profile, an approved
  external read would have no enforceable path. Until it ships,
  `fs.read.external` is `DENY`, not `ASK` (`COMPONENT_REUSE_MATRIX.md §4`).
- `P8` depends on **`P1b` and `P2`**, not on `P0` alone. Two separate reasons, and
  the second was missed until round 11:
  1. `P8` requires the plugin process to be sandboxed at least as tightly as the
     action it intercepts (`SECURITY_INVARIANTS.md` I17), which only exists after
     `P1b`. An earlier revision gave `P8` a `P0`-only dependency, which would have
     allowed plugin execution before the sandbox it depends on.
  2. `P8` `ACCEPTANCE_CRITERIA` (5) requires that **argument mutation invalidates the
     prior decision and forces re-evaluation**. Re-evaluation is performed *by the
     P2 policy engine* — there is no second policy evaluator. Enabling hooks before
     P2 exists would make that criterion unsatisfiable, and the hook would have no
     engine to re-validate a mutation against. The dependency graph previously read
     `P0 ──▶ P1b ──▶ P8`, permitting exactly that ordering.
- Everything else is friction or expressiveness.
- `P4` can start any time after `P2`; it is meaningless before `P3` ships a
  channel.
- `P8` depends on **`P1b` and `P2`** (it needs the sandbox to confine plugin processes, and the policy engine to re-evaluate mutated arguments) and
  can be sequenced after it. It is **not optional**: `SECURITY_INVARIANTS.md` I17
  records real gaps (plugin argument mutation, plugin execution authority, result
  laundering of hard denials) that `P8` is what closes. An earlier revision gave
  `P8` a `P0`-only dependency and called it "low-risk" and omissible — both
  withdrawn, because that would have permitted enabling plugin execution before
  the sandbox the plugin needs.

## Anti-goals (explicitly not planned)

- Fleet/multi-tenant management, RBAC, multi-person approval.
- A second scheduler or a parallel job system.
- Content/injection scanners as controls.
- PTY support.
- Any MCP tool that changes authority or answers approvals.
- Vendoring Codex's sandbox crate or its Guardian subsystem.
- Any selectable "run without sandbox" runtime mode.

## Definition of done for the whole plan

The baseline's empirically demonstrated escapes must fail. **Every `denied`
in the `Required` column below is a claim about the *catalogued, mediated*
surface, and carries the same qualification as the last row: a spawn that
bypasses the audited spawn API is `UNKNOWN`, not denied (review round 23,
CHECK 11 — round 22 added the `UNKNOWN` row but left this sentence claiming
P1 "does close" the rows above it, which is the stronger claim the row denies).
No row in this table asserts confinement of an uncatalogued process.**

| Baseline probe | Today | Required |
|---|---|---|
| `run_shell("cd /tmp && cat <outside>")` | allowed | denied (sandbox) |
| `run_shell("ls ~")` | allowed | denied (sandbox) |
| `run_shell("curl https://example.com")` | allowed, no approval | denied (no-network profile), or ASK once the network descriptor exists |
| nested `python3` / `node` / `bash` reading outside workspace | unmeasured, assumed allowed | denied identically to the direct child |
| `run_shell` with re-spelled path (`../`, symlink, absolute) | allowed | denied |
| **direct-spawn surfaces** (`remote_shell`, `ssh`, `detached_job`, `persistent-shell` on macOS) | bypass `ManagedChild` entirely | either routed through the managed path or carrying an explicit sandbox plan |
| **uncatalogued spawn entries** (raw `fork`/`posix_spawn`, a plugin-crate spawn, a re-exported alias, a plan widened past the anti-widening rule) | reachable and unmeasured | **`UNKNOWN` — not a row this plan can close.** P1a (1b) binds only callers that use the audited API; the runtime-bypass residual is phase **P1a-R** since round 30 — detective, `NOT_ENFORCED`, and off P1a's acceptance set; CP-04 surrenders universal reachability. **Round 31 removed the old sentence in this row that read "P1a does not pass on (1b) alone"; it was a live gate for a phase that had already stopped having one.** This row is the part of the threat model that stays open, and stating it here is what keeps the table above it honest (review round 21, CHECK 11) |
