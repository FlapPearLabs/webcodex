#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
#
# Native smoke for P1 execution normalization.
#
# WHY THIS MUST BE RUN BY HAND, FROM AN ORDINARY TERMINAL
# ------------------------------------------------------
# Same reason as native-seatbelt-ae.sh: a nested sandbox refuses `sandbox_apply`
# with EPERM before any profile takes effect. When that happens every
# enforcement check "passes" for the wrong reason — the program never started.
# This script detects that and reports ENV_BLOCKED, never PASS.
#
# WHAT THIS MEASURES (and what it does not)
# -----------------------------------------
# It exercises the P1 *production* path — the real runner chokepoint
# (`spawn_local_action` → `ExecutionBroker` → `sandbox-exec`) — rather than the
# research probe binary. The distinction matters: `native-seatbelt-ae.sh`
# measures the *profile*; this measures that production execution actually
# reaches it.
#
# It does NOT establish that all execution is normalized. SSH, browser/CDP,
# plugin/MCP providers, LSP, the persistent interactive shell, the detached
# durable payload and interpreter-based validation are outside P1 by scope.
#
# DO NOT RUN THIS INSIDE WORKBUDDY / A NESTED SANDBOX. This host (and any
# nested sandbox session) refuses `sandbox_apply` with EPERM, so every
# enforcement check would "pass" for the wrong reason — the program never
# started. The script detects that and reports ENV_BLOCKED, never PASS. Run it
# from an ordinary Terminal.app session (launchd session Aqua) and report the
# result from there. Treating ENV_BLOCKED as a pass is explicitly forbidden by
# the closure spec (§13).
#
# USAGE
#   bash research/spikes/native-normalization-p1.sh
#
# EXIT CODES
#   0  every P1 native check reported PASS
#   1  at least one P1 native check reported FAIL
#   3  host cannot apply a restrictive profile (ENV_BLOCKED — not a pass)

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

say()  { printf '%s\n' "$*"; }
hr()   { printf '=%.0s' {1..72}; printf '\n'; }
fail() { say "FATAL: $*"; exit 2; }

hr
say "WebCodex P1 execution-normalization native smoke"
hr
say "repo: $REPO_ROOT"
say "host: $(uname -srm)"
say "date: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
say ""

# ---------------------------------------------------------------------------
# 1. Environment: refuse to report a result this host cannot produce
# ---------------------------------------------------------------------------
say "--- environment check ---"

probe_rc=0
/usr/bin/sandbox-exec -p '(version 1)(allow default)(deny network*)' -- /usr/bin/true \
  >/dev/null 2>&1 || probe_rc=$?
if [ "$probe_rc" -ne 0 ]; then
  say "restrictive profile probe: rc=$probe_rc (host refuses narrowing)"
  say ""
  say "P1_NATIVE_ALL_PASS=ENV_BLOCKED"
  say ""
  say "This host cannot apply a restrictive Seatbelt profile, so the P1 native"
  say "checks cannot be measured here. This is NOT a pass. Run this script from"
  say "Terminal.app (launchd session Aqua) and report the result from there."
  exit 3
fi
say "restrictive profile probe: rc=0 (host accepts narrowing)"
say ""

SESSION_TYPE="$(launchctl managername 2>/dev/null || echo 'unknown')"
say "launchctl manager: $SESSION_TYPE"
if [ "$SESSION_TYPE" != "Aqua" ]; then
  say "WARNING: launchd session is '$SESSION_TYPE', not 'Aqua'. Record this when"
  say "reporting; the profile probe passed, so results may still be valid."
fi
say ""

# ---------------------------------------------------------------------------
# 2. Run the P1 native cases
# ---------------------------------------------------------------------------
command -v cargo >/dev/null 2>&1 || fail "cargo not found on PATH"

say "--- running P1 native cases ---"
say ""

RUN_LOG="$(mktemp -t webcodex-p1-native.XXXXXX)"
# The cases assert their own confinement and print machine-readable verdicts.
# `--nocapture` is required: the verdicts go to stderr, not to the test harness.
# The runner suite (P1-A..P1-M) enters the REAL production run_shell boundary and
# emits P1_NATIVE_RUN_SHELL / P1_NATIVE_EXTERNAL_DENY / P1_NATIVE_NETWORK_DENY /
# P1_NATIVE_DESCENDANT. P1-G (git apply) lives in webcodex-workspace's git_broker
# tests and emits P1_NATIVE_GIT_APPLY. All verdicts land in the same log the
# summary reads.
( cd "$REPO_ROOT" && \
  cargo test -p webcodex-runner --features workspace-checkpoints \
    --bin webcodex-runner normalization_p1 \
    -- --nocapture --test-threads=1 ; \
  cargo test -p webcodex-workspace --features workspace-checkpoints \
    git_broker::tests::g_git_apply_still_applies_a_real_patch_through_the_broker \
    -- --nocapture --test-threads=1 ) 2>&1 | tee "$RUN_LOG"
TEST_RC="${PIPESTATUS[0]}"
say ""

# The launcher-refusal signature. A run that produced it proves nothing about
# confinement, whatever the harness exit code says. ENV_BLOCKED is NOT a pass.
ENV_BLOCKED=0
if grep -q 'sandbox_apply: Operation not permitted' "$RUN_LOG"; then
  ENV_BLOCKED=1
fi

# ---------------------------------------------------------------------------
# 3. Machine-readable summary
# ---------------------------------------------------------------------------
hr
say "SUMMARY"
hr

emit() { grep -E "^$1=" "$RUN_LOG" | tail -1 || true; }

fail_count=0
# Each case prints exactly one of PASS/FAIL/ENV_BLOCKED. A missing line is
# NOT_REPORTED and counts as a failure: silence must never read as a pass. A
# per-case ENV_BLOCKED is the same event as the launcher-refusal signature: the
# host cannot measure this path, so the whole run cannot be a pass. P1_NATIVE_ALL_PASS
# is computed last (below) from the per-case results plus the launcher-refusal state.
for key in P1_NATIVE_RUN_SHELL P1_NATIVE_GIT_APPLY P1_NATIVE_EXTERNAL_DENY \
           P1_NATIVE_NETWORK_DENY P1_NATIVE_DESCENDANT; do
  line="$(emit "$key")"
  if [ -z "$line" ]; then
    say "$key=NOT_REPORTED"
    fail_count=$((fail_count + 1))
  else
    say "$line"
    case "$line" in
      *"=PASS") ;;
      *"=ENV_BLOCKED")
        ENV_BLOCKED=1
        ;;
      *)
        fail_count=$((fail_count + 1))
        ;;
    esac
  fi
done

if [ "$ENV_BLOCKED" -eq 1 ]; then
  say ""
  say "P1_NATIVE_ALL_PASS=ENV_BLOCKED"
  say ""
  say "The sandbox launcher refused the profile during at least one case, so the"
  say "P1 production path could not be measured on this host. This is NOT a pass."
  rm -f "$RUN_LOG"
  exit 3
fi

rm -f "$RUN_LOG"

if [ "$fail_count" -eq 0 ] && [ "$TEST_RC" -eq 0 ]; then
  say ""
  say "P1_NATIVE_ALL_PASS=true"
  exit 0
fi

say ""
say "P1_NATIVE_ALL_PASS=false ($fail_count check(s) not PASS, harness rc=$TEST_RC)"
exit 1
