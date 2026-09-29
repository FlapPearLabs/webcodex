#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Mechanical verification sweep for this submission (corrected; rounds 24-27).

WHAT THIS SCRIPT IS, AND WHAT IT IS NOT -- read before trusting a PASS
  This script checks TEXT and POSITION only: that a criterion number is
  referenced inside its own phase, that a retired string survives nowhere
  except in a sentence that quotes it in order to withdraw it, that a
  phase-scoped criterion actually lives under that phase's heading, and
  that the documents stay within their size budget.

  It CANNOT detect a semantic defect.  Specifically, a RESULT of
  0 FAILURE(S) does NOT mean the documents are correct.  Every defect the
  independent review found in rounds 20-26 -- a present-state claim with no
  observation behind it, a criterion whose two restatements disagree, a
  table corrected without re-deriving its summary sentence, a mechanism
  offered as a disjunction where one branch cannot produce the report --
  passed this script cleanly.  They are contradictions between a claim and
  its evidence, and no amount of string matching sees a contradiction.
  Round 26 is the sharpest case: the round-26 fix script printed success for
  a REFERENCE_ARCHITECTURE.md edit it computed and never wrote to disk, and
  this sweep reported 0 FAILURE(S) on the unedited file.  A positional PASS
  was mistaken for semantic validation, which is the failure mode this
  header exists to prevent.

  PROVENANCE.  Round 27 blocked a claim about this script on the grounds
  that no execution output was submitted, and then round 27's own prompt
  asserted the output was included when the assembly step had not actually
  appended it. Both were assembly faults, not script faults, and the honest
  form of the fix is to say what this script can and cannot establish about
  its own invocation: **it reports on the files in the directory containing
  it, and it has no way to verify that those files are the same bytes as any
  text pasted elsewhere.** When its output is submitted as evidence, the
  submitter must state the file list and byte counts it ran against, and a
  reader must treat the output as describing THAT filesystem, not as a
  property of any transcript.

  Use it as a floor, never as a ceiling: 0 FAILURE(S) means no KNOWN
  mechanical defect remains, and says nothing about the ones nobody has
  thought to encode here.

ORIGIN AND FALSE POSITIVES

The first run of this script reported 8 FAILUREs; all 8 were the script's own
false positives, and that is itself the defect worth fixing -- a verifier with
known false positives trains you to ignore it, which is how a real one gets
missed.  Each correction below names the false positive it removes:

  FP1  phase-heading detection: P1a/P1b are bold run-in paragraphs, and the
       (1b''x) sub-bullets are indented continuation lines, so a line-index
       comparison against a `##` heading misjudges containment.
  FP2  retired-string sweep: a hit inside a sentence that *quotes the old
       wording in order to withdraw it* is not a live claim.  The first pass
       only recognised a withdrawal marker on the same LINE, and both
       surviving hits are split across lines.
  FP3  `criterion (N)` reference check: criterion numbers are PHASE-LOCAL.
       `criterion (6)` in the matrix means P0's criterion 6, not P1b's.  The
       first pass pooled every number in a document. **The first fix for
       this was to DOCUMENT the scoping rather than to implement it, which
       is why the same false positive survived four rounds; the reviewer's
       reading of the code in round 28 is what finally forced the
       implementation attempted in round 29 — **AND THE ATTEMPT DID NOT WORK.**
       A negative control was run: a real reference (`criterion (9)`) was
       replaced with `criterion (78)`, a number defined nowhere in the
       submission, and the verifier still reported 0 FAILURE(S). The
       section-scoped implementation is dead code for these documents,
       because they are almost entirely pipe tables and `sections()` yields
       too few boundaries to separate a reference from a definition.
       Resolving this correctly needs table-structure awareness — which phase
       a table row belongs to — which is real work rather than a
       regex, and shipping the regex version as a fix would be the
       overclaim this review exists to prevent.
       **Disposition: the criterion-reference check is NOT a gate.** The
       check below is retained only because it is harmless and may become
       useful once it is table-aware. The false positive it was meant to
       remove is instead prevented by construction: no document contains a
       `criterion (N)` reference to an undefined number, which is checkable
       by eye and is stated here rather than asserted by a script that does
       not check it. See L56 and L57.**
  FP4  OUTCOME_STRICT: the `FS-08` hit is the sentence that RECORDS its removal.
"""
import io, os, re

R = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DOCS = ["SECURITY_INVARIANTS.md", "COMPONENT_REUSE_MATRIX.md",
        "REFERENCE_ARCHITECTURE.md", "IMPLEMENTATION_PLAN.md",
        "SECURITY_TEST_VECTORS.md", "OSS_RESEARCH_EVIDENCE.md"]
T = {d: io.open(os.path.join(R, d), encoding="utf-8").read() for d in DOCS}
L = {d: T[d].split("\n") for d in DOCS}
fails = []


def ck(cond, label, detail=""):
    print(("PASS  " if cond else "FAIL  ") + label + (("  -- " + detail) if detail and not cond else ""))
    if not cond:
        fails.append(label)


# ---- FP1 -------------------------------------------------------------------
# A phase "region" runs from its own heading to the next heading at the same or
# higher markdown level.  Bold run-in headings count as headings.
HEAD_RE = re.compile(r"^(?:#{1,4} )|\*\*P\d[a-z]? \u2014 ")


def region(prefixes):
    start = None
    for i, l in enumerate(L["IMPLEMENTATION_PLAN.md"]):
        if any(l.startswith(p) for p in prefixes):
            start = i
            break
    if start is None:
        return None, None
    for j in range(start + 1, len(L["IMPLEMENTATION_PLAN.md"])):
        if HEAD_RE.match(L["IMPLEMENTATION_PLAN.md"][j]):
            return start, j
    return start, len(L["IMPLEMENTATION_PLAN.md"])


print("=" * 72)
print("1. CONTAINER AUDIT -- phase-scoped criteria inside their own region")
print("=" * 72)
OWNED = [
    ("TOOLCHAIN_READ_ALLOWLIST", ["**P1b \u2014 ", "## P1b "]),
    ("(12a) the list is enumerated", ["**P1b \u2014 ", "## P1b "]),
    ("(13a) a workspace-entry preflight", ["**P1b \u2014 ", "## P1b "]),
    ("(13b) a resolver-time `st_nlink` check", ["**P1b \u2014 ", "## P1b "]),
    ("(1b''a) the list is a build-time artefact", ["**P1a \u2014 ", "## P1a "]),
    ("(1b''b) an entry whose verdict", ["**P1a \u2014 ", "## P1a "]),
    ("(1b''c) an entry", ["**P1a \u2014 ", "## P1a "]),
    ("(1b''d) the list", ["**P1a \u2014 ", "## P1a "]),
    ("(9) the compiled profile carries an explicit", ["## P1c "]),
    ("(10) the compiled profile denies a listener", ["## P1c "]),
    ("the rule model expresses the mediated Git", ["## P2 "]),
    ("(13) **the hardlink defence is the pair of named steps", ["**P1b \u2014 ", "## P1b "]),
    ("(9) the compiled profile carries an explicit", ["## P1c "]),
    ("every ACP/MCP/plugin child spawn is **confined", ["## P6 "]),
]
planL = L["IMPLEMENTATION_PLAN.md"]
for needle, prefixes in OWNED:
    s, e = region(prefixes)
    owner = next(p for p in prefixes)
    hits = [i for i, l in enumerate(planL) if needle in l]
    inside = [i for i in hits if s is not None and s < i < e]
    ck(bool(hits) and len(inside) == len(hits),
       "%-40s in %-12s" % ('"' + needle[:38] + '"', owner.strip("*# ")),
       "hits=%s region=%s..%s" % ([h + 1 for h in hits],
                                  (s + 1) if s else None, (e + 1) if e else None))

print()
print("=" * 72)
print("2. STALE REFERENCE SWEEP -- a moved criterion's old number must not live")
print("=" * 72)
# FP2: a sentence that quotes old wording in order to withdraw it is not live.
QUOTES_OLD = re.compile(
    r"(withdrew|wrote (?:this|it) as|put it in|earlier revision|previous wording"
    r"|round \d+ (?:wrote|placed|deleted|re-inserted)|is not its own"
    r"|collided with)", re.I)
for s, why in [("criterion (6) below", "allow-list gate was (6) in round 21"),
               ("Criterion (10) below", "allow-list gate was (10) in round 22"),
               ("criterion (10) of this phase", "allow-list gate was (10)"),
               ("criteria (13) and (14)", "self-caught dangling (14)"),
               ("criteria 1, 2, 3, 4, 5 and 7 pass", "network gate widened in r25 to include 9 and 10"),
               ("criteria 1, 2, 3, 4, 5 and 7**", "network gate widened in r25")]:
    live = 0
    ctx = []
    for i, l in enumerate(planL):
        if s in l:
            window = " ".join(planL[max(0, i - 2):i + 3])
            if not QUOTES_OLD.search(window):
                live += 1
                ctx.append(i + 1)
    ck(live == 0, "no live %r  (%s)" % (s, why), "live at lines %s" % ctx)
ck("criterion (12) below" in T["IMPLEMENTATION_PLAN.md"],
   "allow-list gate cited as criterion (12)")
ck(re.search(r"\(12\) \*\*the `TOOLCHAIN_READ_ALLOWLIST`", T["IMPLEMENTATION_PLAN.md"]),
   "allow-list gate IS criterion (12)")

print()
print("=" * 72)
print("3. OUTCOME_STRICT -- every membership list, and every removed vector")
print("=" * 72)
# ROUND 31 (CHECK 12): this used to be one line per declaration --
#   re.findall(r"^`OUTCOME_STRICT`: (.+?)\.?$", ..., re.M)
# -- which stops at the newline, so a member on a CONTINUATION line was
# invisible to the check.  Several declarations in this file wrap.  A check
# that previews one line and is read as a membership audit is the same defect
# as the disabled criterion-reference check, so it is fixed here rather than
# labelled: the declaration is collected WITH its continuation lines, and the
# removed-vector assertions below run against the whole declaration.
#
# A continuation is a following line that is indented and does not itself start
# a new declaration, table row, heading, or bullet.
_stv = T["SECURITY_TEST_VECTORS.md"]
_stv_lines = _stv.split("\n")
lists = []
_i = 0
while _i < len(_stv_lines):
    _m = re.match(r"^`OUTCOME_STRICT`:\s*(.*)$", _stv_lines[_i])
    if not _m:
        _i += 1
        continue
    _parts = [_m.group(1).rstrip(".")]
    _j = _i + 1
    while _j < len(_stv_lines):
        _nxt = _stv_lines[_j]
        if (not _nxt.strip() or _nxt.startswith(("`OUTCOME_STRICT`", "|", "#", "-", "*"))
                or not _nxt.startswith((" ", "\t"))):
            break
        _parts.append(_nxt.strip())
        _j += 1
    lists.append(" ".join(_parts))
    _i = _j
for x in lists:
    print("  " + x[:96])
# The previews above are truncated for display ONLY.  Membership is asserted
# against `lists`, which holds the complete declaration.
print("  [%d OUTCOME_STRICT declaration(s); membership asserted on the "
      "complete text, not on the truncated preview]" % len(lists))
for v, why in [("FS-08", "removed r22"), ("MD-02", "removed r23"),
               ("EX-13", "removed r23"), ("EX-14", "removed r23"),
               ("NET-08", "removed r23"), ("NET-09", "removed r23")]:
    bad = [x[:50] for x in lists if re.search(r"(?<![-\w])%s(?![-\w])" % v, x)
           and "sole member" not in x and "removed" not in x.lower()
           and "previously read" not in x]
    ck(not bad, "%-7s in no membership list  (%s)" % (v, why), str(bad))
ck(not re.search(r"NET-03\(A/B/B0\)", " ".join(lists)),
   "NET-03(B) removed from the network list (A/B0 remain)")

print()
print("=" * 72)
print("4. CRITERION REFERENCE INTEGRITY -- DISABLED, NOT A GATE. See FP3 "
      "in the header: the negative control fails, i.e. this check "
      "does not detect a reference to an undefined criterion. It is "
      "DISABLED in round 30 rather than merely labelled, because "
      "round 29 found it still printing PASS lines for all six "
      "documents below -- and a PASS line is what a reader scans "
      "for. No line in this section's output is evidence. The "
      "diagnostic still runs, and its findings are printed as "
      "UNVERIFIED-DIAGNOSTIC, never as PASS or FAIL.")
print("=" * 72)
# FP3 (really fixed in round 29, see the header): criterion numbers are
# PHASE-LOCAL, so a document-wide pool of `(N)` definitions is wrong -- it
# lets `criterion (6)` in the matrix resolve against any `(6)` defined
# anywhere in that file, which is precisely the false positive FP3 names.
#
# Definitions are therefore collected PER SECTION. A section boundary is a
# markdown heading, or a bold run-in phase label such as `## P1b -- ...`.
# A reference resolves only against definitions in its own section; a
# reference that names a phase (`P1b (12)`) resolves against that phase's
# section, and an unqualified one against its own.
SECT = re.compile(r"^(?:#{1,6} |\*\*#{0,2} )", re.M)


def sections(text):
    """Split a document at heading / bold-phase-label boundaries."""
    marks = [m.start() for m in SECT.finditer(text)]
    if not marks:
        return [("", text)]
    bounds = marks + [len(text)]
    out = []
    for a, b in zip(bounds, bounds[1:]):
        chunk = text[a:b]
        head = chunk.split("\n", 1)[0][:120]
        out.append((head, chunk))
    return out


PHASE_OF = re.compile(r"^#{0,3}\s*\**\s*(P\d[a-z]?)\b")

for d in DOCS:
    secs = sections(T[d])
    # phase label -> the concatenated text of that phase's section(s)
    by_phase = {}
    for head, chunk in secs:
        m = PHASE_OF.match(head)
        if m:
            by_phase.setdefault(m.group(1), []).append(chunk)
    phase_defs = dict((p, set(re.findall(r"\((\d+)\)", "".join(cs))))
                      for p, cs in by_phase.items())
    missing = []
    for head, chunk in secs:
        own = set(re.findall(r"\((\d+)\)", chunk))
        # a reference qualified by an explicit phase resolves in that phase
        for m in re.finditer(
                r"criteri(?:on|a) \((\d+)\)", chunk):
            window = chunk[max(0, m.start() - 260):m.start()]
            ph = None
            for cand in re.findall(r"\b(P\d[a-z]?)\b", window):
                ph = cand
            if ph and ph in phase_defs:
                if m.group(1) not in phase_defs[ph]:
                    missing.append("%s in %s: (%s not in %s's defs)"
                                   % (head[:24], ph, m.group(1), ph))
            elif m.group(1) not in own:
                missing.append("%s unqualified (%s)"
                               % (head[:24], m.group(1)))
    missing = sorted(set(missing))
    # ROUND 30 (CHECK 12): this used to call ck(), which prints PASS or FAIL.
    # The check is disabled because its negative control fails, so a PASS
    # here asserts nothing -- but it was still a PASS *line*, and that is what
    # a reader takes away. It now prints a diagnostic with no verdict word in
    # it, so it cannot be quoted as a result.
    if missing:
        print("   [UNVERIFIED-DIAGNOSTIC, NOT A GATE] %-28s %d unresolved ref(s):"
              " %s" % (d, len(missing), str(missing[:6])))
    else:
        print("   [UNVERIFIED-DIAGNOSTIC, NOT A GATE] %-28s no unresolved ref "
              "found -- this is NOT a pass and NOT a gate" % d)

print()
print("=" * 72)
print("5. RETIRED STRINGS (rounds 12-24) -- FP2: look at the surrounding window")
print("=" * 72)
for s in ["this closes the risk", "binds uncatalogued callers too",
          "criteria 1, 4, 5 and 7 only",
          "Every other boundary is enforced by host code or the OS",
          "changing policy cannot be done by the agent",
          "HOLDS (I2)", "Invariant I9 holds", "content returned verbatim",
          "Network `ASK`"]:
    live, ctx = 0, []
    for d in DOCS:
        dl = L[d]
        for i, l in enumerate(dl):
            if s in l:
                window = " ".join(dl[max(0, i - 3):i + 4])
                if not re.search(r"withdrew|withdrawn|~~|previous wording|"
                                 r"earlier revision|was rewritten|sole member|"
                                 r"is not established|not a live|no longer|"
                                 r"previously read|demoted in round|"
                                 r"corrected in review", window, re.I):
                    live += 1
                    ctx.append("%s:%d" % (d[:12], i + 1))
    ck(live == 0, "no live %r" % s, "live at %s" % ctx)

print()
print("=" * 72)
print("5b. APPROVAL LIFECYCLE CONSISTENCY (added round 33)")
print("=" * 72)
# Round 32 removed P3's approval affordance and left four downstream documents
# describing a human hand-off, and the verifier did not catch it -- CHECK 12.
# These assertions are the gate that class of defect needed.

# Every document that describes an escalation hand-off must name the P3 channel.
for d, needle, why in [
        ("SECURITY_TEST_VECTORS.md", "operator over the P3 channel",
         "AR-06 escalation target"),
        ("SECURITY_TEST_VECTORS.md", "the operator; the reviewer is advisory only",
         "AR-12 escalation target"),
        ("SECURITY_TEST_VECTORS.md", "then the operator if escalated",
         "MD-05 escalation target")]:
    ck(needle in T[d], "%s carries %s" % (d, why),
       "missing %r" % (needle[:40],))

# No document may still claim escalation is terminal, nor that a human decides.
BAD_APPROVAL = [
    "ESCALATE to human", "escalates to a human", "then human if escalated",
    "no human-approval surface", "renders no human-approval",
    "withholds the affordance", "no approval surface at all",
    "the affordance, which this plan withholds",
]
for s in BAD_APPROVAL:
    live, ctx = 0, []
    for d in DOCS:
        dl = L[d]
        for i, l in enumerate(dl):
            if s in l:
                window = " ".join(dl[max(0, i - 3):i + 4])
                if not re.search(r"round 3[23]|withdrew|withdrawn|~~|"
                                 r"earlier revision|was rewritten|restores|"
                                 r"round 32 made|previous", window, re.I):
                    live += 1
                    ctx.append("%s:%d" % (d[:12], i + 1))
    ck(live == 0, "no live %r" % s, "live at %s" % ctx)

# P3 must ship a producer for the approval decision.
PLAN = T["IMPLEMENTATION_PLAN.md"]
ck("unix domain socket" in PLAN and "operator's uid" in PLAN,
   "P3 names the socket channel and its owner",
   "P3 does not name a channel with a peer credential")

# P3-R must not exist as a live deferral any more.
ck("## P3-R — Host-UI isolation — **REMOVED" in PLAN,
   "P3-R is recorded as removed, not as a live phase",
   "P3-R heading is not marked removed")
ck(PLAN.count("## P3-R") == 1,
   "P3-R appears exactly once (as a removal record)",
   "P3-R appears %d times" % PLAN.count("## P3-R"))

# The delivery statement must match the phase bodies.
ck("**Confinement covers the whole process tree, by attaching at the runner.**" in PLAN,
   "delivery statement claims whole-tree coverage",
   "delivery statement does not claim it")
ck("- **Approval is delivered, over a non-GUI channel.**" in PLAN,
   "delivery statement claims approval is delivered",
   "delivery statement does not claim it")

print()
print("=" * 72)
print("6. SIZE")
print("=" * 72)
for d in DOCS:
    print("  %-30s %5d lines  %7d bytes" % (d, len(L[d]), len(T[d].encode("utf-8"))))

print()
print("=" * 72)
print("RESULT: %d FAILURE(S)" % len(fails))
for f in fails:
    print("  - " + f)
