# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)

# ---- 1. P3 title: approval IS delivered, over a channel that is not a GUI window ----
old = "## P3 — Local control channel + approval machinery (**renders no model-reachable approval affordance; `ASK` resolves to `DENY` in this phase**)"
new = "## P3 — Local control channel + approval machinery (delivered over a **non-GUI** channel, so no synthetic-input route exists)"
assert t.count(old) == 1, "P3 title anchor=%d" % t.count(old)
t = t.replace(old, new)

# ---- 2. P3 goal ----
old = """**Goal:** build the approval machinery — request, digest, host-minted token, decision verification, audit — **and render no approval affordance the model can reach at all, so that `ASK` resolves to `DENY` for the life of this phase.**"""
new = """**Goal:** build the approval machinery — request, digest, host-minted token, decision verification, audit — **and carry the decision over a channel that is not a host GUI window, so that no model-reachable surface can present, auto-answer, or synthesise input to it.**"""
assert t.count(old) == 1, "P3 goal anchor=%d" % t.count(old)
t = t.replace(old, new)

save(P, t, "## P3 — Local control channel + approval machinery (delivered over a **non-GUI** channel")
print("OK f33b step1")
