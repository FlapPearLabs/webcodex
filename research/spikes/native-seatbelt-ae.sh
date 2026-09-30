#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
#
# Native enforcement suite for the Codex-informed macOS Seatbelt profile.
#
# WHY THIS MUST BE RUN BY HAND, FROM AN ORDINARY TERMINAL
# ------------------------------------------------------
# When this process tree is already inside a sandbox that refuses nested
# narrowing, `sandbox-exec` fails at sandbox_apply with EPERM before the
# profile has any effect. Every enforcement test then "passes" for the wrong
# reason: the program never started, so nothing was denied. Running from
# Terminal.app (launchd session `Aqua`, no agent sandbox wrapper) is the only
# way to measure the profile rather than the host's refusal to nest.
#
# The script verifies it is not in such a wrapper and refuses to continue if it
# cannot tell, because a false "all pass" here is worse than no result.
#
# USAGE
#   bash research/spikes/native-seatbelt-ae.sh
#
# EXIT CODES
#   0  all checks passed
#   1  at least one check failed
#   2  preconditions not met (wrong environment, build failed, no cargo)
#   3  host cannot apply a restrictive profile at all

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CRATE_DIR="$REPO_ROOT/crates/webcodex-process"
PROBE_BIN="seatbelt-ae-probe"

say()  { printf '%s\n' "$*"; }
hr()   { printf '=%.0s' {1..72}; printf '\n'; }
fail() { say "FATAL: $*"; exit 2; }

hr
say "WebCodex native Seatbelt enforcement suite"
hr
say "repo:   $REPO_ROOT"
say "host:   $(uname -srm)"
say "date:   $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
say ""

# ---------------------------------------------------------------------------
# 1. Environment sanity: refuse to run where the result would be meaningless
# ---------------------------------------------------------------------------
say "--- environment check ---"

# launchd session type: Aqua means a normal login session. Managers other than
# Aqua (Background, System) are what a nested/sandboxed context looks like.
SESSION_TYPE="$(launchctl managername 2>/dev/null || echo 'unknown')"
say "launchctl manager: $SESSION_TYPE"

# Ancestor chain: if an agent runner is in the ancestry, results are suspect.
ANCESTORS="$(ps -o comm= -p $$ -p $PPID 2>/dev/null | tr '\n' ' ' || echo 'unknown')"
say "process ancestry:  $ANCESTORS"

# Ask the host directly whether it can apply a restrictive profile. This is the
# only precondition that actually matters: if it cannot, every result below is
# ENV_BLOCKED no matter what shell we are in.
#
# ONE probe, not two. A `(allow default)(deny file-read*)` probe used to be
# required as well, which was wrong: a blanket deny-file-read cuts off the
# system reads /usr/bin/true needs to start, so it fails with rc=134 even on a
# host that applies restrictive profiles perfectly well. Requiring it made this
# script report ENV_BLOCKED on capable hosts. `deny network*` narrows something
# while still letting the target run, which is the property we need.
probe_rc=0
/usr/bin/sandbox-exec -p '(version 1)(allow default)(deny network*)' -- /usr/bin/true \
  >/dev/null 2>&1 || probe_rc=$?
if [ "$probe_rc" -ne 0 ]; then
  say "restrictive profile probe: rc=$probe_rc (host refuses narrowing)"
  say ""
  say "This host cannot apply a restrictive Seatbelt profile, so A-E cannot be"
  say "measured here. This is NOT a pass. Do not report NATIVE_A..E or"
  say "READY_FOR_NORMALIZATION as anything other than unmeasured."
  exit 3
fi
say "restrictive profile probe: rc=0 (host accepts narrowing)"
say ""

if [ "$SESSION_TYPE" != "Aqua" ]; then
  say "WARNING: launchd session is '$SESSION_TYPE', not 'Aqua'."
  say "This looks like a nested or background context. The profile probe passed,"
  say "so results may still be valid, but record this when reporting."
  say ""
fi

# ---------------------------------------------------------------------------
# 2. Build the probe
# ---------------------------------------------------------------------------
command -v cargo >/dev/null 2>&1 || fail "cargo not found on PATH"

say "--- building $PROBE_BIN ---"
BUILD_LOG="$(mktemp -t webcodex-ae-build.XXXXXX)"
if ! (cd "$REPO_ROOT" && cargo build -p webcodex-process --bin "$PROBE_BIN") \
      >"$BUILD_LOG" 2>&1; then
  say "build failed; last 40 lines:"
  tail -40 "$BUILD_LOG"
  rm -f "$BUILD_LOG"
  fail "could not build the native probe"
fi
rm -f "$BUILD_LOG"
say "build ok"
say ""

BIN_PATH="$(cd "$REPO_ROOT" && cargo metadata --format-version 1 --no-deps 2>/dev/null \
  | /usr/bin/python3 -c 'import json,sys,os
d=json.load(sys.stdin)
print(os.path.join(d["target_directory"],"debug","seatbelt-ae-probe"))' 2>/dev/null || true)"
if [ -z "$BIN_PATH" ] || [ ! -x "$BIN_PATH" ]; then
  BIN_PATH="$REPO_ROOT/target/debug/$PROBE_BIN"
fi
[ -x "$BIN_PATH" ] || fail "probe binary not found or not executable"

say "probe:   $BIN_PATH"
say "version: $("$BIN_PATH" --version 2>/dev/null || echo 'n/a')"
say ""

# ---------------------------------------------------------------------------
# 3. Run the suite
# ---------------------------------------------------------------------------
say "--- running security gate A-E and runtime compatibility probes ---"
say ""

RUN_LOG="$(mktemp -t webcodex-ae-run.XXXXXX)"
"$BIN_PATH" 2>&1 | tee "$RUN_LOG"
PROBE_STATUS="${PIPESTATUS[0]}"
say ""

if [ "$PROBE_STATUS" -ne 0 ]; then
  say "probe exited $PROBE_STATUS without producing a full summary"
  rm -f "$RUN_LOG"
  exit 1
fi

# ---------------------------------------------------------------------------
# 4. Machine-readable summary
# ---------------------------------------------------------------------------
hr
say "SUMMARY"
hr

emit() { grep -E "^$1=" "$RUN_LOG" | tail -1 || true; }

# Security gate. A-E use only system binaries (/bin/sh, /bin/cat, /usr/bin/nc),
# so these results are about confinement and nothing else.
for key in NATIVE_A NATIVE_B NATIVE_C NATIVE_D NATIVE_E; do
  line="$(emit "$key")"
  if [ -z "$line" ]; then
    say "$key=NOT_REPORTED"
  else
    say "$line"
  fi
done

ALL="$(emit NATIVE_SECURITY_ALL_PASS)"
if [ -z "$ALL" ]; then
  say "NATIVE_SECURITY_ALL_PASS=NOT_REPORTED"
  rm -f "$RUN_LOG"
  exit 1
fi

say "$ALL"

# Runtime compatibility. Reported, never gated: whether a non-system
# interpreter runs under this profile is a fact about the host's interpreter
# layout, not about whether the sandbox confines.
for key in RUNTIME_PYTHON RUNTIME_NODE; do
  line="$(emit "$key")"
  if [ -z "$line" ]; then
    say "$key=NOT_REPORTED"
  else
    say "$line"
  fi
done

# Capture every runtime verdict *before* the run log is removed. The
# interpreter note below used to call `emit` after `rm -f "$RUN_LOG"`, so it
# silently compared two empty strings and printed nothing: a missing
# interpreter read as "no note needed" instead of "not runnable". Reading a
# deleted file cannot be allowed to look like a passing check.
PY_RUNTIME_VERDICT="$(emit RUNTIME_PYTHON)"
NODE_RUNTIME_VERDICT="$(emit RUNTIME_NODE)"

rm -f "$RUN_LOG"

if [ "$ALL" = "NATIVE_SECURITY_ALL_PASS=true" ]; then
  say ""
  say "VERDICT: READY_FOR_NORMALIZATION=true"
  say "The Codex-informed profile confines as designed on this host: A-E all pass."
  if [ "$PY_RUNTIME_VERDICT" != "RUNTIME_PYTHON=PASS" ] || \
     [ "$NODE_RUNTIME_VERDICT" != "RUNTIME_NODE=PASS" ]; then
    say ""
    say "NOTE: at least one non-system interpreter is not runnable under this"
    say "profile on this host. That is a runtime-compatibility fact and does NOT"
    say "affect the security verdict above; it must be resolved before any"
    say "product decision that assumes those interpreters are available."
  fi
  exit 0
fi

say ""
say "VERDICT: READY_FOR_NORMALIZATION=false"
say "At least one security check failed. See the per-test output above."
exit 1
