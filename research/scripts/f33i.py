# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

# ---- CHECK 1: drop the cost ranking entirely ----
P = '/Users/songshiyao/Desktop/Projects/webcodex/COMPONENT_REUSE_MATRIX.md'
t = load(P)
old = 'The honest characterisation is therefore **"recommended as the cheapest *assessed* option, and provisional pending two open assessments that could overturn it"** — not "selected", and not, as rounds 13-29 had it, "defensible". **Defensible was a comparative claim about two options nobody had assessed, and it is withdrawn in round 30.**'
new = 'The honest characterisation is therefore **"recommended, provisional pending two open assessments that could overturn it"** — not "selected", and not, as rounds 13-29 had it, "defensible". **Defensible was a comparative claim about two options nobody had assessed, and it is withdrawn in round 30.** **Round 33 removes the remaining comparative word as well: "cheapest *assessed*" was still a cost ranking, and this matrix records no cost figures for any option, so the ranking had no basis in the document that asserts it (review round 32, CHECK 1, blocking). The recommendation now rests on the reuse-type reasoning above and nothing else.**'
assert t.count(old) == 1, "matrix cheapest=%d" % t.count(old)
t = t.replace(old, new)
save(P, t, "Round 33 removes the remaining comparative word as well")
print("OK f33i matrix")

# ---- CHECK 6: P1b goal must not stand in for the qualified test outcome ----
P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)
old = 'So the accurate claim is **"the demonstrated escapes fail on paths, and one alias-based residual is disclosed and remains"**'
new = 'So the accurate claim is **"the demonstrated escapes fail on paths, and one alias-based residual is disclosed and remains"** — and, per review round 32 CHECK 6, the goal sentence must not be read as covering what PE-06a records: a test of the direct pathname passes while the same outside inode remains readable through a hardlink the child creates after admission. **PE-06a is a permanent macOS limitation of this plan, not a pending verification**'
assert t.count(old) == 1, "P1 goal claim=%d" % t.count(old)
t = t.replace(old, new)
save(P, t, "PE-06a is a permanent macOS limitation of this plan, not a pending verification")
print("OK f33i plan P1 goal")
