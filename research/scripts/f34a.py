# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)

# ---- CHECK 8: P5(5) still carries the round-32 terminal-DENY rule ----
old = """(5) `APPROVE_FOR_ME` routes ASK to the reviewer and **unresolved cases to `DENY`** — round 32 makes this exact, because P3 no longer offers any human-approval surface, so there is nothing for an escalation to reach. **A reviewer `ESCALATE` is a terminal `DENY` in this plan, not a hand-off to a person**"""
new = """(5) `APPROVE_FOR_ME` routes ASK to the reviewer and **escalation to the operator over the P3 channel**, with every transport failure — no operator answer, timeout, error, busy, interrupted, invalid, stale — resolving to `DENY` (review round 33, CHECK 8, blocking: round 32 had made escalation a terminal `DENY` here, and this criterion is the one place that rule was still live after the socket change restored the approval path. **The rule that survives is the fail-closed one, not the no-approval one: an unanswered escalation is `DENY`, an answered one is the operator's decision**"""
assert t.count(old) == 1, "P5(5)=%d" % t.count(old)
t = t.replace(old, new)

save(P, t, "The rule that survives is the fail-closed one, not the no-approval one")
print("OK f34a P5(5)")
