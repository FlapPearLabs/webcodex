# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)

# Replace the whole (7b') gate text with a positive criterion that has a producer.
old_start = "(7b') THE RELEASE GATE, and it is one condition, not a choice between two.**"
i = t.find(old_start)
assert i > 0, "7b' start not found"
j = t.find("\n\n> **What withholding does NOT establish", i)
assert j > i, "7b' end not found"
k = t.find("| `ROLLBACK` | until P5", j)
assert k > j, "ROLLBACK anchor not found"

new = """(7b') THE RELEASE GATE, and it is a positive criterion with a producer: the decision travels over a channel that is not a host GUI window.** Specifically: the approval request is written to a **unix domain socket outside every writable root**, the socket is created by the runner with mode `0600` owned by the **operator's uid**, and the decision returns over that same socket carrying the host-minted single-use token. A `Browser/CDP` or `Computer/UI-automation` route can synthesise **input events**; it cannot **connect to a unix socket**, because that requires a filesystem permission the sandbox profile denies and a peer credential the kernel checks. Input synthesis and IPC are different capabilities, so there is no surface on which "the model pressed the button" and "the operator answered" become indistinguishable. **This is why the criterion is satisfiable and why P3-R is not needed: the contradiction the round-31 reviewer identified was never a property of approval, it was a property of rendering approval in a window the model can type into.** The old criterion asked for a window that cannot be typed into; the new one declines to have such a window. **(7b) is withdrawn as a gate: its property is satisfied structurally, not by an isolation mechanism this plan lacks.**"""

t = t[:i] + new + t[j:]

# Replace the withdrawal prose that followed it.
old2_start = "> **What withholding does NOT establish, stated here because round 30's executable read of this criterion was correct and the plan had not said it.**"
i2 = t.find(old2_start)
assert i2 > 0, "withholding prose not found"
j2 = t.find("\n| `ROLLBACK` | until P5", i2)
assert j2 > i2, "ROLLBACK anchor 2 not found"

new2 = """> **What the non-GUI channel does and does not establish, stated once (round 33).** It establishes that **no synthetic-input route can answer an approval**, because answering requires an IPC connection the sandbox denies and the kernel attributes to a uid. It does **not** establish that the operator always answers deliberately: a human can still answer carelessly, and `SECURITY_TEST_VECTORS.md` AP-10..AP-16 keep every transport failure resolving to `DENY`. What it removes is the specific defect rounds 29-32 circled — **a decision read off a window the model can drive is not established to be a human decision** — by never putting that window on the path. **The `ASK` → `DENY` behaviour that round 32 introduced is withdrawn**: approval is delivered, and the product keeps its approval UX, which is what the brief's low-friction requirement asks for. (7b') is satisfiable because the mechanism exists and is named, not because the requirement was relaxed. (Review round 18, CHECK 3 is answered rather than withdrawn: criterion 6 covers the process route, criterion 7 covers the channel, and the host-UI surfaces have nothing to drive.)"""

t = t[:i2] + new2 + t[j2:]

# ROLLBACK row
old3 = """| `ROLLBACK` | until P5, `auto` mode never emits ASK, so P3 is inert if the local UI is disabled; with no channel available, all ASK resolves to DENY. **Under (7b') as rewritten in round 30, host-UI-mediated approval is permanently `DENY` in this phase — the Browser/Computer surfaces lose interactive approval for as long as P3-R is unbuilt, which is a product cost the plan accepts rather than a boundary it claims** |"""
new3 = """| `ROLLBACK` | until P5, `auto` mode never emits ASK, so P3 is inert if the local UI is disabled; with no channel available, all ASK resolves to DENY. **Under (7b') as rewritten in round 33, approval is delivered over a socket rather than a window, so there is no UI to roll back to a prompt; the operator's `approve`/`deny` CLI is the whole surface** |"""
assert t.count(old3) == 1, "ROLLBACK anchor=%d" % t.count(old3)
t = t.replace(old3, new3)

save(P, t, "(7b') THE RELEASE GATE, and it is a positive criterion with a producer")
print("OK f33c")
