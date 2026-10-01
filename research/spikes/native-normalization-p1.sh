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
# P1-G is measured through `workspace_checkpoint::git_apply`, i.e. the whole
# production chain `workspace_checkpoint::git_apply → git_broker::run_git →
# ExecutionBroker`. The broker's own lower-level fidelity test is still run and
# reported, but it is NOT accepted as evidence for the checkpoint path: it never
# touches the checkpoint layer.
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
#   bash research/spikes/native-normalization-p1.sh              # full run
#   bash research/spikes/native-normalization-p1.sh --self-check # aggregation logic only
#
# EXIT CODES
#   0  every P1 native check reported PASS
#   1  at least one P1 native check reported FAIL
#   2  the script's own aggregation self-check failed
#   3  host cannot apply a restrictive profile (ENV_BLOCKED — not a pass)

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

say()  { printf '%s\n' "$*"; }
hr()   { printf '=%.0s' {1..72}; printf '\n'; }
fail() { say "FATAL: $*"; exit 2; }

# ---------------------------------------------------------------------------
# Aggregation rule, in one place
# ---------------------------------------------------------------------------
# P1_NATIVE_ALL_PASS is true only when ALL of the following hold:
#
#   * RUNNER_RC == 0      (the webcodex-runner normalization suite passed)
#   * WORKSPACE_RC == 0   (the webcodex-workspace checkpoint suite passed)
#   * FAIL_COUNT == 0     (every required marker was reported PASS)
#   * ENV_BLOCKED == 0    (no case reported ENV_BLOCKED)
#
# The two exit codes are captured from two *separate* cargo invocations. They
# cannot be read out of a single pipeline: in
#
#     ( cargo test RUNNER ; cargo test WORKSPACE ) | tee "$LOG" ; RC=${PIPESTATUS[0]}
#
# PIPESTATUS[0] is the status of the whole subshell, which is the status of its
# *last* command. A runner suite that failed every case followed by a
# workspace suite that passed would yield RC=0 and read as a clean run — so the
# gate would have been reporting the absence of a later failure rather than the
# presence of a pass.
all_pass() {
  [ "$1" -eq 0 ] && [ "$2" -eq 0 ] && [ "$3" -eq 0 ] && [ "$4" -eq 0 ]
}

# Deterministic proof that the aggregation rule above cannot be satisfied by a
# failing runner suite. No cargo, no sandbox, no host dependency.
self_check() {
  local bad=0

  # The exact defect this rule exists to catch: runner failed, workspace passed.
  if all_pass 1 0 0 0; then
    say "SELF-CHECK FAILED: runner rc=1 with workspace rc=0 was accepted as a pass"
    bad=1
  fi

  # The mirror image, and every other single-input failure.
  all_pass 0 1 0 0 && { say "SELF-CHECK FAILED: workspace rc=1 accepted"; bad=1; }
  all_pass 0 0 1 0 && { say "SELF-CHECK FAILED: a non-PASS marker accepted"; bad=1; }
  all_pass 0 0 0 1 && { say "SELF-CHECK FAILED: ENV_BLOCKED accepted"; bad=1; }
  all_pass 1 1 0 0 && { say "SELF-CHECK FAILED: both suites failing accepted"; bad=1; }

  # And the one combination that must be accepted, so the rule cannot be
  # trivially "always false" either.
  if ! all_pass 0 0 0 0; then
    say "SELF-CHECK FAILED: a genuinely clean run was rejected"
    bad=1
  fi

  if [ "$bad" -eq 0 ]; then
    say "SELF-CHECK PASSED: ALL_PASS requires RUNNER_RC=0 AND WORKSPACE_RC=0 AND no failures AND no ENV_BLOCKED"
  fi
  return "$bad"
}

if [ "${1:-}" = "--self-check" ]; then
  self_check
  exit $?
fi

hr
say "WebCodex P1 execution-normalization native smoke"
hr
say "repo: $REPO_ROOT"
say "host: $(uname -srm)"
say "date: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
say ""

# The aggregation rule is what makes the summary trustworthy, so it is checked
# before it is used, and a failure here aborts rather than reporting a verdict.
if ! self_check; then
  say ""
  say "P1_NATIVE_ALL_PASS=false (aggregation self-check failed; no measurement was trusted)"
  exit 2
fi
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
say "launchd session manager: $SESSION_TYPE"
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

# Suite 1 — the real production run_shell boundary. Emits P1_NATIVE_RUN_SHELL /
# P1_NATIVE_EXTERNAL_DENY / P1_NATIVE_NETWORK_DENY / P1_NATIVE_DESCENDANT.
# Run on its own so its exit code is this command's exit code.
( cd "$REPO_ROOT" && \
  cargo test -p webcodex-runner --features workspace-checkpoints \
    --bin webcodex-runner normalization_p1 \
    -- --nocapture --test-threads=1 ) >>"$RUN_LOG" 2>&1
RUNNER_RC=$?
say "runner suite finished: rc=$RUNNER_RC"

# Suite 2 — the checkpoint wrapper, i.e. the git path production actually takes.
# `workspace_checkpoint::git_apply` is private, so this can only be driven by an
# in-module test; that is what makes it evidence about the checkpoint layer and
# not just about the broker underneath it. Its own lower-level fidelity test runs
# alongside it for information only.
( cd "$REPO_ROOT" && \
  cargo test -p webcodex-workspace --features workspace-checkpoints \
    checkpoint_git_apply_applies_a_real_patch_through_the_broker \
    -- --nocapture --test-threads=1 ; \
  cargo test -p webcodex-workspace --features workspace-checkpoints \
    git_broker::tests::g_git_apply_still_applies_a_real_patch_through_the_broker \
    -- --nocapture --test-threads=1 ) >>"$RUN_LOG" 2>&1
WORKSPACE_RC=$?
say "workspace suite finished: rc=$WORKSPACE_RC"
say ""

cat "$RUN_LOG"

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

FAIL_COUNT=0
# Each case prints exactly one of PASS/FAIL/ENV_BLOCKED. A missing line is
# NOT_REPORTED and counts as a failure: silence must never read as a pass. A
# per-case ENV_BLOCKED is the same event as the launcher-refusal signature: the
# host cannot measure this path, so the whole run cannot be a pass. P1_NATIVE_ALL_PASS
# is computed last (below) from the per-case results plus both exit codes plus
# the launcher-refusal state.
for key in P1_NATIVE_RUN_SHELL P1_NATIVE_GIT_APPLY P1_NATIVE_EXTERNAL_DENY \
           P1_NATIVE_NETWORK_DENY P1_NATIVE_DESCENDANT; do
  line="$(emit "$key")"
  if [ -z "$line" ]; then
    say "$key=NOT_REPORTED"
    FAIL_COUNT=$((FAIL_COUNT + 1))
  else
    say "$line"
    case "$line" in
      *"=PASS") ;;
      *"=ENV_BLOCKED")
        ENV_BLOCKED=1
        ;;
      *)
        FAIL_COUNT=$((FAIL_COUNT + 1))
        ;;
    esac
  fi
done

# Reported, never required: this marker is the broker's own fidelity and says
# nothing about the checkpoint layer.
fidelity="$(emit P1_NATIVE_GIT_BROKER_FIDELITY)"
say "${fidelity:-P1_NATIVE_GIT_BROKER_FIDELITY=NOT_REPORTED} (informational, not the P1-G gate)"

# The two exit codes are reported as their own fields so a reader can see which
# suite failed without re-deriving it from the log.
say "P1_RUNNER_TEST_RC=$RUNNER_RC"
say "P1_WORKSPACE_TEST_RC=$WORKSPACE_RC"
say "P1_ENV_BLOCKED_COUNT=$ENV_BLOCKED"
say "P1_MARKER_FAILURE_COUNT=$FAIL_COUNT"

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

if all_pass "$RUNNER_RC" "$WORKSPACE_RC" "$FAIL_COUNT" "$ENV_BLOCKED"; then
  say ""
  say "P1_NATIVE_ALL_PASS=true"
  exit 0
fi

say ""
say "P1_NATIVE_ALL_PASS=false (runner_rc=$RUNNER_RC workspace_rc=$WORKSPACE_RC failures=$FAIL_COUNT env_blocked=$ENV_BLOCKED)"
exit 1