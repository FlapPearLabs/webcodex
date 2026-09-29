# -*- coding: utf-8 -*-
import sys
sys.path.insert(0, '/tmp/codex-review')
from save import load, save

P = '/Users/songshiyao/Desktop/Projects/webcodex/IMPLEMENTATION_PLAN.md'
t = load(P)

# ---- CHECK 4/5: a fixed runner profile cannot later grant a child what it
#      itself is denied.  The resolution is that the runner is NOT confined
#      less than its most privileged child: the profile is composed per-spawn
#      and applied to the CHILD, and the runner is confined to the UNION.
#      Inheritance then cannot subtract. ----
old = """> **This is the "minimal increment" the brief asks for, and the earlier shape was not.** Confinement at the single attach point is **one** mechanism. Normalising every spawn site (P1a) is **N** mechanisms plus a permanent inventory that must be re-verified whenever it drifts. Round 33 therefore **demotes P1a from prerequisite to consequence**: attaching at the runner does not require the spawn catalogue to be complete, because nothing about the boundary depends on the catalogue's completeness."""
new = """> **This is the "minimal increment" the brief asks for, and the earlier shape was not.** Confinement at the single attach point is **one** mechanism. Normalising every spawn site (P1a) is **N** mechanisms plus a permanent inventory that must be re-verified whenever it drifts. Round 33 therefore **demotes P1a from prerequisite to consequence**: attaching at the runner does not require the spawn catalogue to be complete, because nothing about the boundary depends on the catalogue's completeness.
>
> **The profile is a UNION, and this is the mechanism round 33 had to add (review round 33, CHECK 4 and CHECK 5, blocking).** Round 33's first attempt confined the runner to *one* profile and let P1c/P1d compile approved grants into *child* profiles. **That is incoherent, and the reviewer is right that no criterion in the plan noticed:** a child cannot be granted access its own parent is denied, so every network grant, every external read, and every relaxed mode would either fail or require the runner to be less confined than its most privileged child — which is the shape the whole design exists to prevent.
>
> **The resolution, stated as a criterion so it is checkable.** The runner is confined to the **union of every profile any spawn could legitimately need**, computed from the policy engine's capability vocabulary rather than from a hand-written list, and **each child receives its own narrower profile at `exec`**. Three properties follow, and each is a test:
> 1. **Monotone narrowing.** A child's profile is always a **subset** of the runner's. A grant can therefore never widen past the parent, and inheritance cannot subtract from the runner's own reach.
> 2. **The union is finite and derived.** It is computed from the mode/capability table P5 already defines, so a new mode changes the union by construction rather than by an audit someone must remember to perform.
> 3. **A profile that would exceed the union is refused at construction**, in the same place and with the same failure mode as (1b)'s no-plan refusal — so an over-broad request is a build error, not a runtime surprise.
>
> **What this costs, stated plainly.** The union is broader than any single child's profile, so the runner itself can do more than any one job needs — a shell spawned by the runner inherits the union unless (1) narrows it at `exec`. **The security consequence is bounded but real: a compromise of the runner process itself is outside what the per-child narrowing can contain.** This plan confines *what children can reach*, and that is the property the brief asks for; hardening the runner binary against its own compromise is a different task and is not claimed here. `REFERENCE_ARCHITECTURE.md §3` already records the two facilities (per-`exec` attach, inherited by descendants) and this row names which one carries the boundary."""
assert t.count(old) == 1, "union anchor=%d" % t.count(old)
t = t.replace(old, new)

save(P, t, "The profile is a UNION, and this is the mechanism round 33 had to add")
print("OK f34c union")
