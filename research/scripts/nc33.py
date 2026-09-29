# -*- coding: utf-8 -*-
"""Negative control for the round-33 approval-lifecycle checks (L57).

A new check that has never failed proves nothing.  Each mutation below injects
exactly the defect class that round 32 shipped, and every check must turn FAIL.
"""
import io, os, shutil, subprocess, sys, tempfile

REPO = '/Users/songshiyao/Desktop/Projects/webcodex'
PY = '/Users/songshiyao/.workbuddy/binaries/python/versions/3.13.12/bin/python3'
STV = os.path.join(REPO, 'SECURITY_TEST_VECTORS.md')

MUTATIONS = [
    ("AR-06 reverts to a human hand-off",
     "| AR-06 | reviewer client unavailable | `ESCALATE` to the operator over the P3 channel — never `ALLOW` |",
     "| AR-06 | reviewer client unavailable | `ESCALATE` to human — never `ALLOW` |"),
    ("AR-12 reverts to a human hand-off",
     "every ASK escalates to the operator; the reviewer is advisory only",
     "every ASK escalates to a human; the reviewer is advisory only"),
    ("MD-05 reverts to a human hand-off",
     "reviewer first, then the operator if escalated",
     "reviewer first, then human if escalated"),
]

def run():
    r = subprocess.run([PY, os.path.join(REPO, 'research/verification/verify.py')],
                       capture_output=True, text=True, cwd=REPO)
    return r.stdout

base = run()
assert "RESULT: 0 FAILURE(S)" in base, "baseline is not clean; fix docs first"
print("baseline: RESULT: 0 FAILURE(S)  (as expected)")
print()

orig = io.open(STV, encoding='utf-8').read()
backup = STV + ".nc-backup"
shutil.copy2(STV, backup)
try:
    for name, old, new in MUTATIONS:
        assert orig.count(old) == 1, "mutation anchor missing/bad: %s" % name
        io.open(STV, 'w', encoding='utf-8').write(orig.replace(old, new))
        out = run()
        failed = "RESULT: 0 FAILURE(S)" not in out
        n = out.count("FAIL  ")
        print("%-42s -> %s (%d FAIL line(s))" % (
            name, "DETECTED" if failed else "*** MISSED ***", n))
        assert failed, "negative control MISSED: %s" % name
        shutil.copy2(backup, STV)
finally:
    shutil.copy2(backup, STV)
    os.unlink(backup)

print()
print("all 3 negative controls DETECTED -- the checks discriminate.")
final = run()
assert "RESULT: 0 FAILURE(S)" in final, "document not restored"
print("restored: RESULT: 0 FAILURE(S)")
