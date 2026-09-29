# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

# ================= STV =================
P = '/Users/songshiyao/Desktop/Projects/webcodex/SECURITY_TEST_VECTORS.md'
t = load(P)

# AR-03
old = "| AR-03 | reviewer returns `ESCALATE` | **`DENY` — there is no human-approval surface to fall through to, because P3 renders none (round 32)** |"
new = "| AR-03 | reviewer returns `ESCALATE` | the request is presented to the **operator over the P3 unix-socket channel** and resolves by that decision; a reviewer `ESCALATE` never decides the outcome itself (round 33) |"
assert t.count(old) == 1, "AR-03=%d" % t.count(old)
t = t.replace(old, new)

# AR-06
old = "| AR-06 | reviewer client unavailable | `ESCALATE` to human — never `ALLOW` |"
new = "| AR-06 | reviewer client unavailable | `ESCALATE` to the operator over the P3 channel — never `ALLOW` |"
assert t.count(old) == 1, "AR-06=%d" % t.count(old)
t = t.replace(old, new)

# AR-12
old = "| AR-12 | `APPROVE_FOR_ME` with an **empty** delegated set (the default) | every ASK escalates to a human; the reviewer is advisory only |"
new = "| AR-12 | `APPROVE_FOR_ME` with an **empty** delegated set (the default) | every ASK escalates to the operator; the reviewer is advisory only |"
assert t.count(old) == 1, "AR-12=%d" % t.count(old)
t = t.replace(old, new)

# MD-05
old = "| MD-05 | `APPROVE_FOR_ME` | ASK-class action | reviewer first, then human if escalated |"
new = "| MD-05 | `APPROVE_FOR_ME` | ASK-class action | reviewer first, then the operator if escalated |"
assert t.count(old) == 1, "MD-05=%d" % t.count(old)
t = t.replace(old, new)

# §1 vocabulary line
old = "  action must not take effect; `ASK` means no effect without a human decision;"
new = "  action must not take effect; `ASK` means no effect without an operator\n  decision on the P3 channel;"
assert t.count(old) == 1, "vocab=%d" % t.count(old)
t = t.replace(old, new)

# CP-07 -- re-point at the socket property (it now has a producer)
i = t.find("## 7." )  # not needed; do targeted replace below
old = "CP-07"
# handled in a separate pass to avoid ambiguity

save(P, t, "| AR-03 | reviewer returns `ESCALATE` | the request is presented to the **operator over the P3 unix-socket channel**")
print("OK f33e STV part 1")
