# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/SECURITY_TEST_VECTORS.md'
t = load(P)

i = t.find("| CP-07 |")
assert i > 0
j = t.find("\n", i)            # end of the table row
assert j > i
# the two withdrawal paragraphs R30 appended run until the next blank-line block that is not one of them
k = t.find("\n## ", j)
if k < 0:
    k = len(t)
seg = t[j:k]
# keep anything in seg that is not a withdrawal paragraph
paras = [p for p in seg.split("\n\n") if p.strip()]
kept = [p for p in paras if "superseded readings" not in p and "withdrawn in round 30" not in p and p.strip().startswith("|")]
tail = ("\n\n" + "\n\n".join(kept) + "\n\n") if kept else "\n\n"

new = """| CP-07 | **Host-UI route to the approval channel** (review round 33; the property is unchanged, the mechanism is new): for each model-reachable host-UI-interaction surface — Browser/CDP, Computer/UI-automation, and any added later — (a) a test drives the surface and asserts that **no approval decision is produced on the P3 unix domain socket**; (b) a negative test asserts the socket is unreachable from inside the sandbox profile, and that a connection attempt is refused **by the profile** rather than by the model's cooperation. **This row now has a producer, and the test is decidable.** Rounds 27-32 recorded it as `SPECIFIED` and expected to FAIL, because the plan rendered approval into a **host window** and had no mechanism preventing a UI-automation route from driving that window. Round 33 removes the window: the decision travels over a socket owned by the operator's uid, so a route that can synthesise input events still cannot answer it. **The "expected to FAIL" disposition is withdrawn, and the row was rewritten rather than relabelled** — a test of an absent producer cannot be re-pointed at a present one by editing its verdict. The two superseded readings recorded here in round 30 ("P3 does not pass while CP-07 is `NOT_ENFORCED`" and "P3 does not pass without this property") are struck and are **not** part of this row's requirement: P3's gate is (7b'), which the socket mechanism now satisfies. |"""

t = t[:i] + new + tail + t[k:]
save(P, t, "This row now has a producer, and the test is decidable")
print("OK f33f CP-07")
