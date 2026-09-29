# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/research/verification/verify.py'
t = load(P)

anchor = 'print()\nprint("=" * 72)\nprint("6. SIZE")'
assert t.count(anchor) == 1, "size anchor=%d" % t.count(anchor)

new_section = '''print()
print("=" * 72)
print("5b. APPROVAL LIFECYCLE CONSISTENCY (added round 33)")
print("=" * 72)
# Round 32 removed P3's approval affordance and left four downstream documents
# describing a human hand-off, and the verifier did not catch it -- CHECK 12.
# These assertions are the gate that class of defect needed.

# Every document that describes an escalation hand-off must name the P3 channel.
for d, needle, why in [
        ("SECURITY_TEST_VECTORS.md", "operator over the P3 channel",
         "AR-06 escalation target"),
        ("SECURITY_TEST_VECTORS.md", "the operator; the reviewer is advisory only",
         "AR-12 escalation target"),
        ("SECURITY_TEST_VECTORS.md", "then the operator if escalated",
         "MD-05 escalation target")]:
    ck(needle in T[d], "%s carries %s" % (d, why),
       "missing %r" % (needle[:40],))

# No document may still claim escalation is terminal, nor that a human decides.
BAD_APPROVAL = [
    "ESCALATE to human", "escalates to a human", "then human if escalated",
    "no human-approval surface", "renders no human-approval",
    "withholds the affordance", "no approval surface at all",
    "the affordance, which this plan withholds",
]
for s in BAD_APPROVAL:
    live, ctx = 0, []
    for d in DOCS:
        dl = L[d]
        for i, l in enumerate(dl):
            if s in l:
                window = " ".join(dl[max(0, i - 3):i + 4])
                if not re.search(r"round 3[23]|withdrew|withdrawn|~~|"
                                 r"earlier revision|was rewritten|restores|"
                                 r"round 32 made|previous", window, re.I):
                    live += 1
                    ctx.append("%s:%d" % (d[:12], i + 1))
    ck(live == 0, "no live %r" % s, "live at %s" % ctx)

# P3 must ship a producer for the approval decision.
PLAN = T["IMPLEMENTATION_PLAN.md"]
ck("unix domain socket" in PLAN and "operator's uid" in PLAN,
   "P3 names the socket channel and its owner",
   "P3 does not name a channel with a peer credential")

# P3-R must not exist as a live deferral any more.
ck("## P3-R — Host-UI isolation — **REMOVED" in PLAN,
   "P3-R is recorded as removed, not as a live phase",
   "P3-R heading is not marked removed")
ck(PLAN.count("## P3-R") == 1,
   "P3-R appears exactly once (as a removal record)",
   "P3-R appears %d times" % PLAN.count("## P3-R"))

# The delivery statement must match the phase bodies.
ck("**Confinement covers the whole process tree, by attaching at the runner.**" in PLAN,
   "delivery statement claims whole-tree coverage",
   "delivery statement does not claim it")
ck("- **Approval is delivered, over a non-GUI channel.**" in PLAN,
   "delivery statement claims approval is delivered",
   "delivery statement does not claim it")

'''

t = t.replace(anchor, new_section + anchor)
save(P, t, "5b. APPROVAL LIFECYCLE CONSISTENCY (added round 33)")
print("OK f33k verifier")
