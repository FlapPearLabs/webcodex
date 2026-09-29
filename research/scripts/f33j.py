# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)

# ---- P4: name the source of authority for grants ----
old = """**Goal:** stop re-asking for the same thing within a session."""
new = """**Goal:** stop re-asking for the same thing within a session.

> **P4's grants come from the P3 channel, and this is stated because round 32
> found the phase promising `always` grants with no issuer (newly-raised finding).**
> Round 32 made P3 render no approval, so no grant could ever be issued and P4 was
> describing storage for rows nothing wrote. Round 33 restores the issuer: **a grant
> is created only by an operator decision delivered over the P3 unix-socket
> channel**, and the store accepts a row only when it carries the single-use token
> of the request that produced it. There is no second source of authority, and a
> grant whose originating token cannot be re-verified against the audit record is
> `DENY`, not `ASK`."""
assert t.count(old) == 1, "P4 goal=%d" % t.count(old)
t = t.replace(old, new)
save(P, t, "a grant\n> is created only by an operator decision delivered over the P3 unix-socket")
print("OK f33j P4")
