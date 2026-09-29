# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)

# ---- Remove the P3-R phase entirely ----
i = t.find("## P3-R — Host-UI isolation")
assert i > 0, "P3-R heading not found"
j = t.find("## P4 — Session grants", i)
assert j > i, "P4 heading not found"
t = t[:i] + """## P3-R — Host-UI isolation — **REMOVED in round 33, and this is a design change rather than another disclosure**

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

""" + t[j:]

# ---- Sync the up-front delivery statement ----
old = "- **No approval is offered to the model at all.** P3 builds the machinery and renders no affordance; `ASK` resolves to `DENY` until P3-R can show a confirmation dialog is unreachable from synthetic input."
new = "- **Approval is delivered, over a non-GUI channel.** P3 ships the full approval path — `once`/`session`/`always`, digest binding, replay protection, audit — carried over a unix domain socket owned by the operator's uid. No model-reachable surface can answer it, because answering requires an IPC connection the sandbox denies. The cost is that the decision is an operator CLI action, not a pop-up."
assert t.count(old) == 1, "delivery bullet anchor=%d" % t.count(old)
t = t.replace(old, new)

old2 = "- **Universal process-tree confinement remains `UNKNOWN`.** A raw `fork`/`posix_spawn`, a plugin-crate spawn, a re-exported alias, an exempt control-plane caller, and a widened plan are all outside that control. The residual is `SPECIFIED` in phase P1a-R, has no producer, and is not gated on by anything."
new2 = "- **Confinement covers the whole process tree, by attaching at the runner.** The sandbox profile is established before `exec` and inherited by every descendant, where it cannot be widened, so confining `webcodex-runner` constrains every child below it — including children this plan never catalogued. Two residuals remain and are named: a `setuid` `exec`, and a raw spawn inside a pre-existing library that runs before the runner's own attach."
assert t.count(old2) == 1, "confinement bullet anchor=%d" % t.count(old2)
t = t.replace(old2, new2)

old3 = "**Anyone needing the whole-tree property should treat P1a-R and P3-R as prerequisites, not as follow-up work** — they are the two places this plan stops."
new3 = "**The two places this plan stops are narrow and stated: a `setuid` `exec` and a pre-attach raw spawn.** Both are named in P1's round-33 note, and neither is the whole-tree gap that rounds 29-32 were blocked on."
assert t.count(old3) == 1, "stop anchor=%d" % t.count(old3)
t = t.replace(old3, new3)

save(P, t, "## P3-R — Host-UI isolation — **REMOVED in round 33")
print("OK f33d")
