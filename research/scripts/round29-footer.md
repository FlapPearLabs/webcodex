================================================================================
PART 3 — WHAT CHANGED SINCE ROUND 28
================================================================================

Round 28 blocked 2, 4, 5, 6, 7, 8, 9, 11, 12. Attack these.

A. THE STRUCTURAL FIX (CHECKS 2, 5, 6, 7, 9). Five of round 28's nine blocking
   findings were ONE defect: this review appends a correction next to a claim
   instead of replacing the claim, and the correction then lives inside the same
   operative cell. Round 28 said so itself about STV §4 and then found the same
   ordering in four other places.

   The round-29 rule is positional, not editorial: **the operative cell must OPEN
   with the weak claim, and the withdrawn text lives outside it.** Applied:

   - `COMPONENT_REUSE_MATRIX.md §4`'s `secret.read` Default cell now OPENS with
     `NOT_ENFORCED` (capability-wide) and names `DENY` as holding only by path
     and only on the structured surface. The strong label is no longer the first
     thing a reader takes away.
   - P1c(10c) now OPENS with "criterion (10) FAILS, the network axis is `DENY`,
     and `NET-09` is recorded as `NOT_ENFORCED`".
   - P1c's hostname gate now OPENS with "criteria 1, 2, 3, 4, 5, 7, 8, 9 and 10".
     The range "1–8" that opened it is withdrawn, with the reason: it named
     criterion (6), the sentence being governed, and omitted the floors.
   - `COMPONENT_REUSE_MATRIX.md §4`'s `net.outbound` release gate now reads
     "criteria 1, 2, 3, 4, 5, 7, **9 and 10**" — the set P1c and P5 use.
   - NET-06b1's post-exit column no longer says `DENY`.

B. CHECK 4 — P1a(1c)'s ES_LOG. The "observes every process creation on the
   machine regardless of which code path issued it" claim is WITHDRAWN, and the
   withdrawal names two specific reasons: "(i) `exec` is not process creation. A
   `fork()` with no following `exec` produces no `exec` event" and "(ii) An event
   is not an attribution" — nothing correlates the event with whether the spawn
   used the audited API, which "requires either a per-spawn capability token the
   audited path stamps and the observer reads back, or a host-side audit-token
   process tree. Neither is designed here." The criterion is `NOT_ENFORCED` by
   construction, not pending, because a subscriber observes and does not deny.

C. CHECK 8 — P3(7b) is marked "BLOCKED, AND THEREFORE NOT A GATE" and removed
   from the release path. The property is retained as a specification, and the
   release gate moves to (7b'): "no approval affordance is rendered into a
   model-reachable surface, AND the plan records which of the four isolation
   mechanisms above it has adopted, or states that the approval route is
   unavailable." (7b') is satisfiable by withholding host-UI approvals entirely,
   and the trade is named: right for a security boundary, wrong for a feature.

D. NEWLY RAISED 1 — the localhost-only fallback. It is withdrawn: "if (7) fails,
   the network axis is `DENY` — no localhost-only reduction, no partial
   release", because (7) IS the arbitrary-destination demonstration, so a profile
   that cannot express the literal-IP rule has not passed it.

E. CHECK 11 — the assembly. `assemble28.py` now reports BYTES
   (`os.path.getsize`) and CHARACTERS separately, asserts they agree with the
   encoded form of what it read, and publishes a **sha256 prefix per part**. See
   PART 0b. The byte counts now equal what `wc -c` reports for these files.

F. CHECK 12 — the verifier. This is the item most likely to draw fire, so read it
   carefully. Round 28 found that the criterion-reference check "resolves numbers
   anywhere in a document, not within the referenced phase" — the false positive
   the header has claimed as corrected since 2025.

   Round 29 attempted the fix. **It did not work, and the verifier now says so.**
   A negative control was run: a real reference (`criterion (9)`) was replaced
   with `criterion (78)`, defined nowhere, and the verifier still reported
   0 FAILURE(S). The section-scoped implementation is dead code for these
   documents, which are almost entirely pipe tables. So the check is now labelled
   **NOT A GATE** in its own section header, the failing control is recorded in
   the file's header, and the false positive is instead prevented by
   construction: no document contains a reference to an undefined criterion,
   which is checkable by eye.

   **Attack this.** The claim "checkable by eye" is itself a claim. Check it —
   grep the six documents for `criteri(on|a) \(N\)` and confirm every N is
   defined. If it is not true, that is a new finding, and it is the same class as
   every other finding in this review.

G. L48's OVERSTATEMENT — corrected in the log. It read "The dominant failure mode
   is the undelivered promise". Round 28's LOG-TRUTH AUDIT rejected that as too
   categorical, and it was right: three instances establish that undelivered
   promises RECUR here, not that they outnumber wrong claims. Rounds 20–25 are
   dominated by overclaims, and round 28's own five-cell finding is an overclaim
   rather than a missing deliverable. L48 now says the review has "two roughly
   equal failure modes — a claim stronger than its evidence, and a promise never
   delivered — and the second is the cheaper one to fix".

H. L56 and L57 are new. L56: a known false positive documented but not fixed is
   worse than one that is neither, because the documentation reads as a fix —
   and FP3 survived two rounds of exactly that. L57: a checker that fails its
   negative control must be disabled rather than reported, because tuning a
   regex until one injected control passes teaches it to accept that input and
   nothing else.

================================================================================
PART 4 — REQUIRED OUTPUT FORMAT (Round 29)
================================================================================

Reply in exactly this shape, in English. Be concise. Do not restate the
submission.

CHECK 1: ACCEPT | ISSUE
  <document + section. If ISSUE, the exact offending claim and why it is still
   wrong.>

CHECK 2: ACCEPT | ISSUE
  ...
(repeat through CHECK 12)

NEW DEFECTS INTRODUCED BY THE ROUND-29 REVISION:
  - <or "none">

NEWLY RAISED FINDINGS (not previously an issue in any round):
  - <or "none">

ROUND-28 FIXES A-H VERIFICATION:
  <state for each of A-H: APPLIED AS CLAIMED / PARTIAL / NOT APPLIED / NOT
   FOUND, with the document + section you checked. For each, add one line: what
   new surface did this fix expose?>

LOG-TRUTH AUDIT:
  1. Check the mechanical-sweep figures in the ROUND 28 log entry against the
     six documents as pasted. Report ANY mismatch.
  2. Check L48's revised wording and L52–L57. Does any of them overstate what
     this submission's history supports?
  3. Check PART 0b's byte counts against the pasted text. Do the sha256 prefixes
     give you anything a count did not?

CELL-OPENS-WITH-THE-WEAK-CLAIM AUDIT (new in round 29, and it is the direct
descendant of round 28's five-cell finding):
  Round 28 found five blocking issues that were one defect: a strong claim at the
  front of an operative cell with a correction appended behind it. Round 29
  applied the positional rule. Test whether it was applied everywhere or only
  where it was noticed. **For every operative cell in the six documents — every
  Default cell in the matrix, every gate statement, every required-outcome
  column, every status label — read the FIRST clause and ask whether it is the
  strongest or the weakest claim the cell goes on to make.** Name every cell
  where the front is stronger than the truth. This is the single most productive
  check available this round; expect findings.

NEGATIVE-CONTROL AUDIT (new in round 29):
  The verifier has one disabled check and one documented-by-eye claim. Test both:
  1. Grep the six documents for every `criteri(on|a) (N)` reference and confirm
     each N is defined. The file claims this is "checkable by eye" — check it.
  2. For each remaining ACTIVE check in the verifier, name a mutation to a
     document that would make it FAIL. If you cannot name one for any check,
     that check is decorative and should be reported as such. **The verifier
     passes on documents that have been wrong in nine consecutive rounds; treat
     every PASS as a statement about the check, not about the documents.**

PRODUCER SWEEP:
  <for each property asserted beyond denial/non-enforcement: name the producer
   and classify it as (i) observed baseline property, (ii) design proposal, or
   (iii) test. Apply the round-28 question to each: does the named producer
   actually produce what the claim needs, and is the claim narrower than the
   producer? P1a(1c)'s ES_LOG claim was narrowed in round 29; CP-07/B6 has no
   producer at all and that is now disclosed rather than gated.>

SINGLE-DEFINITION AUDIT (retained):
  <for every gate, find every place it is RESTATED, and check each restatement
   is derivable from the first and names the same set. Round 28 found the proxy
   gate retyped with (6) included and the matrix omitting (9)/(10). Confirm
   those and look for the same class of error elsewhere — including in the log,
   which restates the gates in every entry.>

EXECUTABILITY AUDIT (retained):
  <for every gate, criterion, and disposition: could a reader satisfy the text
   without satisfying the property it claims? Quote the sentence, yes or no.>

DESIGN-COLUMN-AS-STATUS SWEEP (retained):
  <find any remaining place a proposed mechanism is read as present enforcement,
   or an assumption as an observation.>

OVER-HEDGING AUDIT (retained):
  <name anything now so qualified that it delivers no usable guarantee, and say
   whether the trade is right. Round 28 found P1a's unshipped mandatory
   extension makes the macOS-first path non-deliverable as written, and said
   "calling the plan minimal and executable without resolving it is not [right]".
   Has that been resolved?>

UNDELIVERED-PROMISE SWEEP (retained):
  <for every noun in all six documents that reads as a deliverable, name the
   thing that would produce it. Anything with no producer is in scope.>

REMAINING WEAKNESSES (accepted, non-blocking, worth recording):
  - <or "none">

STRONGEST REMAINING OBJECTION:
  <2-4 sentences>

FINAL VERDICT: ACCEPTED | REJECTED
  <If REJECTED, list blocking checks. ACCEPTED requires that no check carries an
   unresolved issue that would mislead a reader about a security property, and
   that the documents are internally consistent about the security boundary.
   Accepting does NOT require you to endorse the design.>

REVIEWER DECLARATION:
  State that your judgement applies only to the material submitted and that you
  did not independently verify the underlying source observations.
