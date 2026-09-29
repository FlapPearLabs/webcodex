# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/review/codex-architecture-review.log'
t = load(P)

entry = """
================================================================================
ROUND 33 -- ADVERSARIAL ARCHITECTURE REVIEW
================================================================================

RUN
  codex exec --sandbox read-only --ephemeral -C <repo> - < prompt
  prompt: 10 parts, 898170 bytes of document text (assembled by assemble33.py)
  model: codex-cli 0.136.0, fresh context, no prior round's text

VERDICT RECEIVED: REJECTED -- blocking checks 1, 3, 4, 6, 8, 9, 12

WHAT ROUND 32 GOT WRONG, AND WHY THAT MATTERS MORE THAN THE BLOCKING COUNT
  Round 31 left 5 blocking checks. Round 32 returned 7. The count rose, and the
  rise is the finding: comparing the two verdicts item by item, checks 3/8/9/12
  did not merely persist -- their CONTENT changed.

    R31 CHECK 3 : P3(7b') contradicts ITSELF about the same process-route UI.
    R32 CHECK 3 : RA 2/7/9/10 still describes a working approval UI. Two
                  incompatible DELIVERY DESCRIPTIONS across documents.

    R31 CHECK 8 : HD-02/HD-03 do not distinguish pathname from content.
    R32 CHECK 8 : AR-06/AR-12 require escalation to a human; P3 offers no
                  human surface; AR-03 says ESCALATE -> DENY.

    R31 CHECK 9 : P3 calls its process-route confirmation "human approval"
                  while admitting UI automation may drive it.
    R32 CHECK 9 : MD-05 says "reviewer first, then human"; AP-01..AP-05,
                  AP-17 require successful human approvals and grants that
                  no phase can now produce.

    R32 CHECK 12: verify.py reports 0 failures while all of the above is live.

  So the round-32 edit moved the contradiction OUT of P3 and INTO four
  downstream documents, while removing the product's approval feature. It
  satisfied "P3 is self-consistent" by making the other documents wrong.
  That is L72 (reducing scope obliges you to update every downstream consumer)
  violated one round after being written down, and it is the single most
  expensive mistake in this log.

  L74. A change that makes one document self-consistent by making another
  document wrong has not been made. Count the documents, not the sentences.

  L75. "Delete the feature" and "keep the feature" are not two severities of
  the same fix. They are different products. The brief requires low friction;
  a plan that answers a security objection by removing the approval path has
  answered a different question than the one asked, and the review rounds that
  followed would have kept blocking it forever for exactly that reason.

DECISION FOR ROUND 33: STOP EDITING WORDING. CHANGE TWO DESIGNS.

  Thirty-two rounds of wording edits produced a document that is honest,
  self-disclosing, internally annotated, and still does not meet the brief.
  That is the signal that the defect is not in the sentences. Both remaining
  hard blockers had the same shape -- a property with no producer, moved into
  a phase that produces nothing and blocks nothing (P1a-R, P3-R):

    P1a-R : universal process-tree confinement is UNKNOWN because the plan
            enumerates spawn sites, and the enumeration cannot be complete.
    P3-R  : a decision read off a host GUI window is not established to be a
            human decision, and no mechanism prevents a UI-automation route
            from driving that window.

  In both cases the unproducer-ness is a property of the OBJECT the plan
  chose, not of the plan's effort. So round 33 changes the objects.

DESIGN CHANGE A -- confine the runner, not the spawn catalogue (CHECK 4)
  A Seatbelt / Landlock / bwrap profile is established BEFORE exec and is
  INHERITED by every descendant, where it cannot be widened. Therefore
  confining ONE process -- webcodex-runner, the process every sandboxed child
  descends from -- constrains the entire tree below it, including children
  this plan never catalogued.

  This inverts the plan's shape. Enumeration was the load-bearing assumption
  and it is unsound; inheritance makes it unnecessary. What remains open is
  narrower and is named: (i) a setuid exec, (ii) a raw spawn inside a
  pre-existing library that runs before the runner's own attach.

  It is also, at last, the "minimal increment" the brief asks for: ONE
  mechanism at one attach point, rather than N mechanisms plus a permanent
  inventory that must be re-verified whenever it drifts. P1a is demoted from
  prerequisite to consequence.

DESIGN CHANGE B -- carry the decision over a socket, not a window (CHECK 3/8/9)
  The approval decision travels over a unix domain socket created by the
  runner OUTSIDE every writable root, mode 0600, owned by the OPERATOR'S UID;
  the operator answers with `webcodex approve|deny <request_id>` plus the
  host-minted single-use token.

  Why this resolves the contradiction rather than restating it: a
  Browser/CDP or Computer/UI-automation route can synthesise INPUT EVENTS. It
  cannot CONNECT TO A UNIX SOCKET, because that needs a filesystem permission
  the sandbox profile denies and a peer credential the kernel verifies.
  Input synthesis and IPC are different capabilities, so there is no surface
  on which "the model pressed the button" and "the operator answered" become
  indistinguishable. The old criterion asked for a window the model cannot
  type into; the new design declines to have such a window.

  P3-R is REMOVED (its property is now structural), CP-07 is re-pointed from
  "a window is never presented" to "no decision is produced on the socket"
  (a decidable test with a producer), and the ASK -> DENY behaviour introduced
  in round 32 is WITHDRAWN -- approval ships, which is what low friction asks.

  The cost is named rather than hidden: approval is a terminal action, not a
  pop-up dialog. That is a real product change.

DOWNSTREAM SYNCHRONISATION (the round-32 lesson, applied this time)
  AR-06, AR-12, MD-05 -> operator over the P3 channel; STV vocabulary line ->
  operator decision; RA section 7 transport -> operator CLI over socket;
  RA section 8 flow -> ESCALATE reaches the operator; P4 -> grants are issued
  only by an operator decision, with the originating token re-verified;
  P5 ROLLBACK -> escalation has a hand-off target that times out to DENY;
  delivery statement -> rewritten to match the new phase bodies.

OTHER BLOCKING FIXES
  CHECK 1  The matrix called the adapter "the cheapest *assessed* option". The
           qualifier did not help: the document records no cost figures for
           any option, so the ranking had no basis in the document asserting
           it. The comparative claim is now removed entirely and the
           recommendation rests on the reuse-type reasoning alone.
  CHECK 6  P1b's goal sentence said the escapes "fail on paths" while PE-06a
           records that the same outside inode stays readable through a
           hardlink created after admission. The goal now carries the
           limitation inline: PE-06a is a permanent macOS limitation of this
           plan, not a pending verification.
  CHECK 12 verify.py gained a section 5b, "APPROVAL LIFECYCLE CONSISTENCY",
           asserting that every escalation hand-off names the P3 channel, that
           the round-32 phrasings are not live anywhere, that P3 names a
           channel with a peer credential, and that the delivery statement
           agrees with the phase bodies. This is the check whose absence let
           round 32 ship four inconsistent documents with RESULT: 0.

NEGATIVE CONTROL (L57 -- a check that has never failed proves nothing)
  Three mutations, each reinjecting exactly the defect class round 32 shipped:
    AR-06 reverts to a human hand-off        -> DETECTED
    AR-12 reverts to a human hand-off        -> DETECTED
    MD-05 reverts to a human hand-off        -> DETECTED
  3/3 detected, document restored, verifier back to 0 failures. Without this
  the new section would be a third decorative gate.

FILES CHANGED IN ROUND 33 (sizes pasted from the verifier run, not predicted)
  SECURITY_INVARIANTS.md           557 lines    32146 bytes   (unchanged)
  COMPONENT_REUSE_MATRIX.md        447 lines    44146 bytes
  REFERENCE_ARCHITECTURE.md       1083 lines    71093 bytes
  IMPLEMENTATION_PLAN.md           681 lines   125339 bytes
  SECURITY_TEST_VECTORS.md        1178 lines   101170 bytes
  OSS_RESEARCH_EVIDENCE.md         682 lines    70033 bytes   (unchanged)
  research/verification/verify.py  -- gained section 5b; RESULT: 0 FAILURE(S)

NEW LESSONS
  L74. A change that makes one document self-consistent by making another
       document wrong has not been made. Count the documents, not sentences.
  L75. "Delete the feature" and "keep the feature" are not severities of one
       fix; they are different products. Check the brief before choosing.
  L76. When a property has had no producer for several rounds, the productive
       question is not "how do we disclose this?" but "which object did we
       choose that makes it unproducible?" Four rounds of disclosure failed
       because the answer to the second question was never asked.
  L77. A verifier section is not evidence until a mutation has made it fail.
  L78. Inheriting a property from the platform is strictly stronger than
       enumerating the cases it applies to, and it is the difference between
       one mechanism and N.
  L79. A window and a socket are not two implementations of one interface. One
       is reachable by anything that can synthesise input; the other is
       reachable only by a process the kernel attributes to a uid. Choosing
       between them is choosing what an adversary must be able to do.

STATE AFTER ROUND 33
  Two design changes made, one phase removed (P3-R), one deferred phase
  re-scoped (P1a-R), four downstream documents re-synchronised, one new
  verifier section with a passing negative control. Round 33's own verdict is
  not yet known; this entry records the changes, not a result.
"""

save(P, t + entry, "L79. A window and a socket are not two implementations of one interface.")
print("OK r33log")
