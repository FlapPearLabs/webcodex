#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
#
# native-seatbelt-probe.sh — determine whether restrictive Seatbelt profiles
# fail because of THIS repository's execution environment, or on this Mac
# regardless of who runs them.
#
# WHY THIS IS A SEPARATE SCRIPT
#
# Round 2 measured, inside the WorkBuddy agent environment:
#
#   P0  (version 1)(allow default)                         -> rc 0   applied
#   P1  (version 1)(allow default)(deny network*)          -> rc 71  EPERM
#   P2  (version 1)(allow default)(deny file-read*)        -> rc 71  EPERM
#
# `(allow default)` applies; every narrowing rule is refused at `sandbox_apply`.
# Two explanations fit that observation equally well:
#
#   (a) the host kernel refuses nested restrictive Seatbelt profiles, or
#   (b) this process already runs inside a sandbox, and macOS will not let a
#       sandboxed process impose further restrictions on a child.
#
# These have opposite consequences for the spike: (a) means per-action sandboxing
# cannot be verified on any Mac and the backend choice must change; (b) means it
# is only unverifiable from inside this tool, and a normal Terminal would show it
# working. Nothing in round 2 can tell them apart.
#
# This script settles it. RUN IT FROM A NORMAL TERMINAL WINDOW — one you opened
# yourself, not a shell spawned by the agent.
#
# WHAT TO DO
#
#   1. Open Terminal.app (or iTerm) directly.
#   2. Run:  bash /path/to/native-seatbelt-probe.sh
#   3. Paste the whole output back into the agent conversation.
#
# The script is read-only: it runs /usr/bin/true under three profiles and prints
# exit codes. It changes no system state.

set -u

echo "=============================================================="
echo " Native Seatbelt probe"
echo " host : $(uname -srm)"
echo " date : $(date)"
echo "=============================================================="
echo

# Identify the caller so we can tell a normal login session from a nested one.
echo "--- session context ---"
echo "uid            : $(id -u) ($(id -un))"
echo "pid            : $$"
echo "ppid           : $PPID"
if command -v launchctl >/dev/null 2>&1; then
  echo "launchctl mgr  : $(launchctl managername 2>/dev/null || echo 'unavailable')"
fi
if [ -n "${SSH_CONNECTION:-}" ]; then
  echo "ssh connection : yes (${SSH_CONNECTION})"
else
  echo "ssh connection : no (local session)"
fi
echo

# Walk the ancestor chain. If any ancestor is itself a sandbox wrapper, the
# nesting hypothesis gains support.
echo "--- ancestor chain (looking for a sandbox wrapper) ---"
p=$$
chain=""
while [ "$p" -gt 1 ] 2>/dev/null; do
  line=$(ps -o pid=,ppid=,comm= -p "$p" 2>/dev/null)
  [ -z "$line" ] && break
  chain="$chain
  $line"
  p=$(echo "$line" | awk '{print $2}')
done
echo "$chain"
echo
if echo "$chain" | grep -qiE 'sandbox-exec|sandboxd|seatbelt|codex|workbuddy'; then
  echo "NESTING_HINT = a sandbox wrapper appears in the ancestor chain"
else
  echo "NESTING_HINT = no obvious sandbox wrapper in the ancestor chain"
fi
echo

run_probe() {
  local name="$1" profile="$2"
  local out rc
  out=$(/usr/bin/sandbox-exec -p "$profile" /usr/bin/true 2>&1)
  rc=$?
  printf '%-4s rc=%-4s %s\n' "$name" "$rc" "$(echo "$out" | tr '\n' ' ')"
}

echo "--- the three profiles ---"
run_probe P0 '(version 1)(allow default)'
run_probe P1 '(version 1)(allow default)(deny network*)'
run_probe P2 '(version 1)(allow default)(deny file-read*)'
echo

# P3-P5 rule out alternative explanations for the EPERM.
echo "--- alternative explanations ---"
run_probe P3 '(version 1)(allow default)(deny file-write*)'
run_probe P4 '(version 1)(allow file-read*)'
run_probe P5 '(version 2)(allow default)(deny file-read*)'
echo

echo "--- inheritance under a permissive profile (should work either way) ---"
/usr/bin/sandbox-exec -p '(version 1)(allow default)' \
  /bin/sh -c 'echo CHILD_RAN' 2>&1 | sed 's/^/  /'
echo

echo "=============================================================="
echo " HOW TO READ THIS"
echo "=============================================================="
cat <<'EOF'
If P1 and P2 return rc=0 here, but rc=71 inside the agent:
    the host is fine; the agent's environment is what refuses them.
    -> NESTED_SANDBOX_HYPOTHESIS = SUPPORTED
    -> per-action sandboxing is verifiable on this Mac, just not from in here.

If P1 and P2 also return rc=71 here:
    the host itself refuses narrowing profiles.
    -> NESTED_SANDBOX_HYPOTHESIS = REJECTED
    -> macOS Seatbelt cannot be verified on this machine at all, and the
       backend decision must change (or move to Linux/bwrap, which is
       scriptable in CI).

Either way, paste the full output back. Do not summarise it.
EOF
echo
echo "If P1-P5 differ from each other, that is itself a finding — report it as-is."
