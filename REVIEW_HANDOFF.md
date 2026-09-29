# REVIEW_HANDOFF.md

Entry point for independent review of the WebCodex secure-agent architecture
study. Read this file first; it describes what exists, what state it is in, and
what is unresolved.

---

## 1. Objective

Make WebCodex (the Coding MCP behind ChatGPT Web) into a **safe, low-friction,
Codex-style local coding runtime**.

```
ChatGPT Web
  -> WebCodex (MCP)
    -> local coding runtime on the operator's machine
```

Target capability set:

| Capability | Meaning |
|---|---|
| `READ_ONLY` | inspect the workspace, no mutation |
| workspace agent | read/write inside the project root, run builds and tests |
| human approval | an `ASK` requires a person to decide before the action takes effect |
| `APPROVE_FOR_ME` | a reviewer model handles routine `ASK`s, escalating the rest |
| `DANGER_FULL_ACCESS` | a deliberately relaxed, still-floored mode |

The brief asks for the **smallest** increment that makes this safe, reusing mature
open-source implementations rather than inventing new ones.

---

## 2. Current status

```
ARCHITECTURE_ONLY
NO_PRODUCTION_IMPLEMENTATION
```

Nothing in this branch is running code. There is no implementation of any phase,
no compiled artifact, and no executed profile. The branch contains documents,
raw reviewer output, and the scripts used to produce and check them.

**The security boundary has never been exercised.** No sandbox profile was
compiled, no network proxy was run, no approval channel was built. Every mechanism
described in these documents is a proposal.

---

## 3. Review state — read this part carefully

```
LAST_COMPLETED_CODEX_REVIEW = R33
LAST_CODEX_RESULT          = REJECTED  (8 blocking checks)
R34                         = PARTIAL design fixes applied, NOT REVIEWED
```

### Deviation from the handoff instruction, stated plainly

The handoff instruction I received said: `LAST_COMPLETED_CODEX_REVIEW = R32`,
`R33 = UNREVIEWED_DESIGN_REVISION`. **That was accurate when it was written and
it stopped being accurate during execution.** Round 33 ran and returned REJECTED
before the stop instruction arrived, and I then applied three of its fixes.

I am recording the real state rather than the instructed state. Writing R33 down
as unreviewed would be false, and a reviewer told "the last review was R32" would
not know that an R33 verdict exists and disagrees with the documents it is being
asked to approve.

| Round | Result | Blocking checks | What it found |
|---|---|---|---|
| R29 | REJECTED | 9 | Design-level contradiction, not wording: a criterion required an unbuilt detector, making its own phase unsatisfiable. |
| R30 | REJECTED | 6 | The fix had been applied *beside* the claim rather than in place of it — the stronger claim was still live upstream. |
| R31 | REJECTED | 5 | One cell made two incompatible statements about the same object (an approval dialog reachable and unreachable at once). |
| R32 | REJECTED | 7 | **The count went up, and that was the finding.** My round-32 fix had made P3 self-consistent by making four other documents wrong, and had removed the product's approval path. |
| R33 | REJECTED | 8 | The two design changes were real but had not been propagated into the criteria that enforce them. |

### What the last five rounds were actually about

Two distinct categories, and conflating them is how 33 rounds came to look like
33 security vulnerabilities:

**Mostly process, not distinct vulnerabilities:**
- evidence wording ("this is a comparative claim with no comparison behind it")
- verifier integrity (a check that passes without being able to fail)
- downstream synchronisation (fixing document A and forgetting that B, C and D
  quote it)

**Genuinely two real design problems:**
1. **Process-tree sandbox boundary** — can confinement cover the whole process
   tree, or only an enumerated set of spawn sites?
2. **Human-approval trust boundary** — when a decision is read off a host GUI
   window that a UI-automation route can drive, is it established to be a human
   decision?

Rounds 29-33 did not find 33 different security holes. They found two hard
problems and an unbounded number of ways to describe them inaccurately.

---

## 4. Key findings

### 4.1 The baseline has a real user-level execution-surface problem

WebCodex's shell, job, and agent surfaces can execute outside the workspace with
no OS-level boundary. The authority decision is made once, server-side, before
mutation, and is trustworthy as far as it goes — but there is no sandbox beneath
it. `REFERENCE_ARCHITECTURE.md §1` and `SECURITY_INVARIANTS.md` carry the detail,
and `OSS_RESEARCH_EVIDENCE.md §2.1a` carries the spawn-site inventory that shows
`ManagedChild::spawn` is not the only path.

### 4.2 The design now attaches confinement at the runner, not at each spawn site

Rounds 29-32 attacked this from the text side for four rounds without success:
universal process-tree confinement was disclosed, footnoted, moved into a phase
that blocked nothing, and re-disclosed. The reviewer kept calling it an honestly
disclosed failure to meet a hard requirement, and it was.

Round 33 changed the design instead. A Seatbelt / Landlock / bwrap profile is
established **before `exec`** and is **inherited by every descendant, where it
cannot be widened**. Confining one process — `webcodex-runner`, the process every
sandboxed child descends from — therefore constrains the whole tree below it,
including children the plan never catalogued. Enumeration becomes a testing
convenience rather than the security property.

**This is the "minimal increment" the brief asked for and the earlier shape was
not**: one mechanism at one attach point, instead of N mechanisms plus a permanent
inventory that must be re-verified whenever it drifts.

R33 accepted the direction and blocked on its consequences, which are real:

- A fixed parent profile cannot later grant a child access the parent is denied.
  Round 33's first attempt was incoherent and the reviewer said so. The fix
  applied in R34: the runner is confined to the **union** of every profile any
  spawn could need, and each child gets a **narrower** profile at `exec`, so
  narrowing is monotone and inheritance cannot subtract.
- The cost is named rather than hidden: the union is broader than any single
  job's needs, so **a compromise of the runner process itself is outside what
  per-child narrowing can contain.** This plan confines *what children can reach*.
  Hardening the runner binary is a different task and is not claimed here.
- Two residuals remain open and are named: a `setuid` `exec`, and a raw spawn
  inside a pre-existing library that runs before the runner's own attach.

### 4.3 The human-approval design being explored — NOT APPROVED

The trust problem is real: a decision read off a host GUI window is not
established to be a human decision, because `Browser/CDP` and
`Computer/UI-automation` routes can synthesise input into a window.

Three rounds attacked this by changing what the plan *claims*:

- R30: split the property into a phase that blocks nothing
- R31: delete the claim
- R32: stop delivering approval entirely

All three kept the window. R32's version was the worst of them: it satisfied "P3
is self-consistent" by making four other documents wrong and by removing the
approval path the brief explicitly asks for. That direction is **withdrawn**.

The current design (R33, unapproved) keeps the feature and changes the object:

```
approval request  -> unix domain socket outside every writable root, mode 0600
operator decision -> single-use token for that exact request_id
                  + HMAC over the request digest under a per-boot secret
                     that the sandbox cannot read
```

The reasoning: a UI-automation route can synthesise **input events**, and it can
drive an **unsandboxed host terminal** — R33's reviewer correctly showed that
socket file mode and a same-uid peer credential do **not** distinguish that from
the operator typing the command. So the credential, not the permission bit, is
what does the work: a capability the adversary must *possess* rather than a
permission it must lack.

**What this does not establish, stated in the document itself:** it does not stop
the model asking the operator for approval, reading the operator's screen, or
socially engineering a decision. It establishes that no route can *manufacture* a
decision without the per-boot secret. Coercion and screen-reading are named as
residual risk, not claimed as closed.

**The product cost is real:** approval becomes a terminal command, not a pop-up
dialog. That is a genuine change to the user experience.

### 4.4 An approach that was tried and abandoned

Round 32's "P3 stops delivering approval" is retained as a
`FAILED_ATTEMPT` in `EVIDENCE_MANIFEST.md`, not deleted. It is the most expensive
single mistake in this study and removing it would make the surviving design look
cheaper than it was.

---

## 5. Open questions

These are genuinely open. None of them is resolved by the documents in this
branch.

| # | Question | Why it matters |
|---|---|---|
| 1 | Codex sandbox: `DIRECT_DEPENDENCY`, `VENDOR_SUBSET`, or `SUBPROCESS_ADAPTER`? | The matrix recommends the adapter provisionally; two alternatives are unassessed and could overturn it. No cost comparison exists. |
| 2 | Does runner-level confinement actually cover every real execution path? | R33 accepted the mechanism and blocked because the plan never made it executable. R34 added the union rule; **it has not been reviewed.** |
| 3 | Should the unix-socket approval channel be built in-house? | Hermes ships an approval transport with six fail-closed codes. Relying on it may be strictly better than reimplementing. |
| 4 | Can Hermes / Codex / Pi approval transports be reused directly? | The plan currently treats them as `DESIGN_ONLY`. If one is directly reusable, the socket design is unnecessary new code. |
| 5 | Is MCP elicitation sufficient? | The baseline advertises only the `tools` capability — no elicitation, no prompts. Adding it may change the whole transport design. |
| 6 | Is a ChatGPT-Web-side approval transport available? | If the human can approve from the browser, the local socket question changes shape entirely. |
| 7 | How is the danger-mode control plane implemented? | Needs a single-use token from a local operator action; the mechanism is named but not designed. |
| 8 | How does network sandboxing attach under a runner-level profile? | The P1c proxy design predates the round-33 attach change and has not been re-derived against it. |
| 9 | Linux / macOS backend divergence. | P1b is macOS-only. Whether bwrap changes the attach model is unexamined. |
| 10 | Is the whole plan over-engineered for the stated goal? | A fair question given 33 rounds. See §7. |

---

## 6. What I would like reviewed

1. **Is this architecture over-engineered?** 33 rounds and six documents for a
   security wrapper may be more machinery than the problem needs. If a
   substantially smaller design closes the same invariants, that is the most
   valuable finding available right now.
2. **Does runner-level confinement hold up?** The claim is that inheritance
   substitutes for enumeration. Is that sound on macOS Seatbelt, and does it
   survive the union-plus-narrowing rule R34 added?
3. **Is the unix-socket approval channel necessary**, or does a mature existing
   transport remove the need to build one?
4. **Which mature components should be reused rather than reimplemented?** The
   matrix flags this as unassessed in two places.
5. **Which of the R32/R33 objections are genuinely blocking**, and which are
   over-formal review process? I have attempted to separate these myself and may
   have got it wrong.
6. **Is this ready for implementation, and what is the minimum first slice?**

---

## 7. Honest self-assessment

The strongest argument against this work: **33 rounds of review produced no
accepted design, and the rounds were mostly about how the documents were worded
rather than about what they proposed.** The two real design problems were
identified early and were not solved until round 33, and the round-33 solution has
itself not been reviewed.

A second honest note: the documents are heavily annotated with round numbers,
withdrawn claims, and correction records. That was deliberate — it makes the
reasoning auditable and it is what surfaced several defects. It also makes the
documents roughly three times longer than the underlying design, which is a real
cost for anyone trying to read them.

---

## 8. Where to look

| If you want to... | Read |
|---|---|
| Understand the target architecture | `REFERENCE_ARCHITECTURE.md` |
| Understand what phases would be built | `IMPLEMENTATION_PLAN.md` — read §"WHAT THIS PLAN DELIVERS" first |
| Understand the security claims | `SECURITY_INVARIANTS.md` |
| Check whether claims are tested | `SECURITY_TEST_VECTORS.md` |
| Check what was reused vs. built | `COMPONENT_REUSE_MATRIX.md` |
| Check upstream evidence | `OSS_RESEARCH_EVIDENCE.md` |
| See what reviewers actually said | `review/rounds/*.log`, then `review/codex-architecture-review.log` |
| See what was tried and abandoned | `EVIDENCE_MANIFEST.md` §"Retained failures and superseded material" |
| See the full decision history | LESSON LEDGER L1-L79 in `review/codex-architecture-review.log` |

**`verify.py` returning `RESULT: 0 FAILURE(S)` is not evidence that this design
is correct.** It checks selected lexical properties across six documents. Rounds
29-33 each found defects it did not catch, and its criterion-reference section is
disabled because its own negative control failed.
