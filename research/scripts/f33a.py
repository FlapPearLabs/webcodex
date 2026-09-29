# -*- coding: utf-8 -*-
import io, os, sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)

# ---- 1. P1 goal: state the runner-self-confinement mechanism up front ----
old_goal = """**Goal:** close the **named baseline escapes** on the **pathname boundary**. This is the phase that changes the security posture. It has **two steps**, and the first was missing from an earlier revision."""
new_goal = """**Goal:** close the **named baseline escapes** on the **pathname boundary**. This is the phase that changes the security posture. It has **two steps**, and the first was missing from an earlier revision.

> **The whole-tree property comes from confining the runner, not from enumerating spawn sites (review round 33; blocking CHECK 4 in rounds 29-32, unchanged across four rounds).** Rounds 16-32 all attacked the same gap from the text side: `UNKNOWN` reachability was disclosed, footnoted, moved into a phase that blocks nothing, and re-disclosed. The gap is **not** a disclosure defect. It is an **attachment-point** defect, and it has a producer that 33 rounds never designed.
>
> **The mechanism:** a Seatbelt/Landlock/bwrap profile is established **before `exec`** and is **inherited by every descendant**, where it **cannot be widened** (a child cannot obtain a profile weaker than its parent's; `REFERENCE_ARCHITECTURE.md §3` records inheritance as the platform facility). Therefore confining **one** process — `webcodex-runner`, the single process from which every sandboxed child is created — constrains the **entire process tree** below it, *including* children created by paths this plan has not catalogued. Enumeration is then a **convenience for testing and for the pre-`exec` attach**, not the security property itself.
>
> **What this changes.** P1a-R's premise is that an uncatalogued spawn is unmeasured and therefore outside the boundary. With runner-level confinement the residual is reclassified: an uncatalogued spawn is **still confined**, because it inherits a profile it never had to ask for. What remains genuinely open is narrower and is stated as such: (i) a child that `exec`s a **setuid** binary may regain privilege the profile cannot express, and (ii) a **raw `fork`/`posix_spawn` in a pre-existing library** that runs *before* the runner's own attach is outside it. Neither is closed here, and neither is what CHECK 4 has been asking about.
>
> **This is the "minimal increment" the brief asks for, and the earlier shape was not.** Confinement at the single attach point is **one** mechanism. Normalising every spawn site (P1a) is **N** mechanisms plus a permanent inventory that must be re-verified whenever it drifts. Round 33 therefore **demotes P1a from prerequisite to consequence**: attaching at the runner does not require the spawn catalogue to be complete, because nothing about the boundary depends on the catalogue's completeness."""
assert t.count(old_goal) == 1, "goal anchor count=%d" % t.count(old_goal)
t = t.replace(old_goal, new_goal)

# ---- 2. P1a-R: reclassify disposition honestly ----
old_r = """| `DISPOSITION` | **`SPECIFIED`.** A criterion exists, no producer is designed, nothing observes it, and **no phase waits on it.** The three-value vocabulary is used deliberately: it is not `DELIVERED` (nothing delivers it) and not merely `PARTIAL` (nothing is half-built), and calling it `UNVERIFIED` would imply an observation was made and found inconclusive |"""
new_r = """| `DISPOSITION` | **`SPECIFIED`.** A criterion exists, no producer is designed, nothing observes it, and **no phase waits on it.** The three-value vocabulary is used deliberately: it is not `DELIVERED` (nothing delivers it) and not merely `PARTIAL` (nothing is half-built), and calling it `UNVERIFIED` would imply an observation was made and found inconclusive. **Round 33 re-scoped this phase after P1 moved the attach point to the runner: the question it was created to answer is now largely answered by the attach itself, and only the two residuals in P1's round-33 note are left. It is retained, smaller, rather than deleted, because a named owner for "did a child get an unconfined profile" is still worth having** |"""
assert t.count(old_r) == 1, "P1a-R disposition anchor count=%d" % t.count(old_r)
t = t.replace(old_r, new_r)

save(P, t, "The whole-tree property comes from confining the runner, not from enumerating spawn sites")
print("OK f33a")
