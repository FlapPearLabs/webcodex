# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

# ============ RA: transport section ============
P = '/Users/songshiyao/Desktop/Projects/webcodex/REFERENCE_ARCHITECTURE.md'
t = load(P)

old = """1. **Local approval UI** (CLI/desktop) — primary. Mirrors Pi's timed confirm:
   auto-cancel on timeout, non-interactive defaults to deny
   (`pi @ 11894012 :: examples/extensions/timed-confirm.ts`,
   `:: examples/extensions/dirty-repo-guard.ts`)."""
new = """1. **Operator CLI over a unix domain socket** — primary, and the only approval
   channel (round 33). The runner creates the socket outside every writable root,
   mode `0600`, owned by the operator's uid; the operator answers with
   `webcodex approve|deny <request_id>` carrying the single-use token.
   Auto-cancel on timeout, non-interactive defaults to deny
   (`pi @ 11894012 :: examples/extensions/timed-confirm.ts`,
   `:: examples/extensions/dirty-repo-guard.ts` — the *timeout semantics* are
   reused; its **window** is not, and the reason is the next sentence).
   **Why not a desktop dialog:** a dialog is a surface a `Browser/CDP` or
   `Computer/UI-automation` route can synthesise input into, so a decision read
   off it is not established to be a human decision. A unix socket requires a
   filesystem permission the sandbox denies and a peer credential the kernel
   checks — a **different capability**, one that input synthesis does not confer.
   The cost is real and is named: approval is a terminal action, not a pop-up."""
assert t.count(old) == 1, "RA transport=%d" % t.count(old)
t = t.replace(old, new)

# RA §2 flow: ESCALATE -> DENY  (round 32 change) -> restore
old = "ESCALATE"
# handled by explicit targeted replaces below

save(P, t, "1. **Operator CLI over a unix domain socket** — primary, and the only approval")
print("OK f33g RA transport")
