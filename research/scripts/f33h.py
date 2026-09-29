# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

# ---- RA §8 flow ----
P = '/Users/songshiyao/Desktop/Projects/webcodex/REFERENCE_ARCHITECTURE.md'
t = load(P)
old = "        ├── ESCALATE → **DENY** (round 32: P3 renders no human-approval surface, so escalation has no hand-off target — §7 describes the machinery, which exists, and the affordance, which this plan withholds)"
new = "        ├── ESCALATE → presented to the **operator over the P3 unix-socket channel** (round 33; round 32 made this `DENY` by withholding the affordance, which removed the product's approval UX and left four downstream documents describing an approval this plan no longer delivered — the socket restores it)"
assert t.count(old) == 1, "RA flow=%d" % t.count(old)
t = t.replace(old, new)
save(P, t, "├── ESCALATE → presented to the **operator over the P3 unix-socket channel**")
print("OK f33h RA flow")

# ---- P5 ROLLBACK ----
P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)
old = "| `ROLLBACK` | `APPROVE_FOR_ME` falls back to `AUTO` with **no approval surface at all** when the reviewer client is absent — an `ESCALATE` from a present reviewer, and an absent reviewer, both resolve to `DENY` (round 32) |"
new = "| `ROLLBACK` | `APPROVE_FOR_ME` falls back to `AUTO` with the **operator channel** as the hand-off target when the reviewer client is absent: an `ESCALATE` from a present reviewer, and an absent reviewer, both reach the operator over the P3 socket, and both are `DENY` if no operator answers before the timeout (round 33; round 32 made both terminal `DENY`, which contradicted AR-06/AR-12/MD-05 and cost the product its approval path) |"
assert t.count(old) == 1, "P5 ROLLBACK=%d" % t.count(old)
t = t.replace(old, new)

# ---- delivery statement lead sentence ----
old = "**It delivers a bounded milestone, not the whole-tree property, and round 32 puts that at the top because five rounds of review found it stated only in footnotes.** Specifically, after every phase in this plan ships:"
new = "**What this plan delivers is stated here in full, because rounds 29-32 established that a limitation stated only in a footnote is not stated at all. Round 33 changed two designs, so two of the limitations those rounds recorded are gone: confinement now covers the whole process tree by attaching at the runner, and approval is delivered over a non-GUI channel.** Specifically, after every phase in this plan ships:"
assert t.count(old) == 1, "delivery lead=%d" % t.count(old)
t = t.replace(old, new)
save(P, t, "confinement now covers the whole process tree by attaching at the runner, and approval is delivered")
print("OK f33h plan")
