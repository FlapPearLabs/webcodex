# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)

# ---- CHECK 3/9: a 0600 same-uid socket does not stop Computer automation
#      operating an unsandboxed host terminal running `webcodex approve`.
#      Fix the mechanism, not the wording: authenticate on a secret the sandbox
#      cannot read, not on uid. ----
old = "(7b') THE RELEASE GATE, and it is a positive criterion with a producer: the decision travels over a channel that is not a host GUI window.** Specifically: the approval request is written to a **unix domain socket outside every writable root**, the socket is created by the runner with mode `0600` owned by the **operator's uid**, and the decision returns over that same socket carrying the host-minted single-use token."
new = "(7b') THE RELEASE GATE, and it is a positive criterion with a producer: the decision travels over a channel that is not a host GUI window, and it is authenticated on a secret the sandbox cannot read rather than on a uid.** Specifically: the approval request is written to a **unix domain socket outside every writable root**, the socket is created by the runner with mode `0600`, and the decision must be accompanied by **(a) the host-minted single-use token for that exact `request_id`**, and **(b) an HMAC over the request digest under a per-boot operator secret generated at runner start, held in a file readable only outside every sandbox root and never passed to any child**.\n\n**Why uid and file mode are not sufficient (review round 33, CHECK 3 and CHECK 9, blocking).** Round 33's first attempt at this gate was `0600` plus \"owned by the operator's uid\", on the reasoning that a UI-automation route cannot connect to a socket. **The reviewer correctly showed that reasoning is false:** `Computer/UI-automation` can drive an **unsandboxed host terminal**, and `webcodex approve` run there by the operator's own login has the operator's uid and the operator's file access. Nothing in the socket's mode distinguishes that from the operator typing the command. **The gap is not the socket, it is the credential.** An HMAC under a secret the model cannot read is a different property from a permission bit: the automation route can reach the terminal, the terminal has the uid, and it still cannot produce the tag. **A capability the adversary must possess, rather than a permission it must lack.**\n\n**The honest limit of this criterion, which is not the one round 33 claimed.** It does not establish that the model cannot *ask* the operator for approval, cannot read the operator's screen, or cannot socially engineer an approval. It establishes that **no route can manufacture a decision without the per-boot secret** — a forged or replayed approval is `DENY`. Coercion and screen-reading are out of scope for this criterion and are named as residual risk rather than claimed as closed."
assert t.count(old) == 1, "7b' cred=%d" % t.count(old)
t = t.replace(old, new)

save(P, t, "it is authenticated on a secret the sandbox cannot read rather than on a uid")
print("OK f34b P3 credential")
