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
# ExecutionBroker`. That test's exit status is the P1-G gate. The broker's own
# lower-level fidelity test is run as a separate, informational invocation and
# is NOT accepted as evidence for the checkpoint path: it never touches the
# checkpoint layer, and its exit status is deliberately not an input to
# P1_NATIVE_ALL_PASS, so it can never mask a checkpoint failure.
#
# It does NOT establish that all execution is normalized. SSH, browser/CDP,
# plugin/MCP providers, LSP, the persistent interactive shell and the detached
# durable payload are outside P1 by scope.
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
#   * CHECKPOINT_RC == 0  (the webcodex-workspace checkpoint suite passed)
#   * FAIL_COUNT == 0     (every required marker was reported PASS)
#   * ENV_BLOCKED == 0    (no case reported ENV_BLOCKED)
#
# GIT_BROKER_FIDELITY_RC is deliberately NOT an input: that suite is
# informational and reports about `git_broker` alone, so it is not evidence for
# the P1-G gate and must never be able to vouch for it.
#
# The exit codes are captured from separate cargo invocations. They cannot be
# read out of a grouped subshell or a single pipeline. In
#
#     ( cargo test A ; cargo test B ) | tee "$LOG" ; RC=${PIPESTATUS[0]}
#
# PIPESTATUS[0] is the status of the whole subshell, which is the status of its
# *last* command. So a required suite that failed every case followed by an
# informational suite that passed would yield RC=0 and read as a clean run — the
# gate would be reporting the absence of a later failure rather than the presence
# of a pass.
all_pass() {
  [ "$1" -eq 0 ] && [ "$2" -eq 0 ] && [ "$3" -eq 0 ] && [ "$4" -eq 0 ]
}

# Deterministic proof that the aggregation rule above cannot be satisfied by a
# failing required suite. No cargo, no sandbox, no host dependency.
self_check() {
  local bad=0

  # runner failed, checkpoint passed.
  if all_pass 1 0 0 0; then
    say "SELF-CHECK FAILED: runner rc=1 with checkpoint rc=0 was accepted as a pass"
    bad=1
  fi

  # checkpoint failed while the runner passed: this is the masking path the
  # broker-fidelity suite used to hide behind, since it is informational and
  # runs last. A gate that accepted this would let a broken checkpoint wrapper
  # report PASS.
  if all_pass 0 1 0 0; then
    say "SELF-CHECK FAILED: checkpoint rc=1 with runner rc=0 was accepted as a pass"
    bad=1
  fi

  # The mirror image, and every other single-input failure.
  all_pass 0 0 1 0 && { say "SELF-CHECK FAILED: a non-PASS marker accepted"; bad=1; }
  all_pass 0 0 0 1 && { say "SELF-CHECK FAILED: ENV_BLOCKED accepted"; bad=1; }
  all_pass 1 1 0 0 && { say "SELF-CHECK FAILED: both required suites failing accepted"; bad=1; }

  # And the one combination that must be accepted, so the rule cannot be
  # trivially "always false" either.
  if ! all_pass 0 0 0 0; then
    say "SELF-CHECK FAILED: a genuinely clean run was rejected"
    bad=1
  fi

  if [ "$bad" -eq 0 ]; then
    say "SELF-CHECK PASSED: ALL_PASS requires RUNNER_RC=0 AND CHECKPOINT_RC=0 AND no failures AND no ENV_BLOCKED (broker fidelity is informational and never an input)"
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
# Toolchain determinism. This repository's tests call `AtomicU32::try_update`,
# which needs a newer rustc than the Homebrew build that comes first on PATH on
# some hosts (`/opt/homebrew/bin/rustc` was 1.94.0 and rejected it with E0658).
# A harness whose result depends on PATH ordering is not reproducible, so the
# rustup toolchain is preferred when present. This selects one binary for this
# process only; the user's PATH is not modified.
if [ -x "$HOME/.cargo/bin/cargo" ]; then
  CARGO_BIN="$HOME/.cargo/bin/cargo"
else
  CARGO_BIN="$(command -v cargo || true)"
fi
[ -n "$CARGO_BIN" ] || fail "cargo not found (neither ~/.cargo/bin/cargo nor PATH)"

P1_RUSTC_VERSION="$("$CARGO_BIN" --version 2>/dev/null || echo 'unknown')"
command -v cargo >/dev/null 2>&1 || fail "cargo not found on PATH"

say "--- running P1 native cases ---"
say "cargo bin: $CARGO_BIN"
say "cargo version: $P1_RUSTC_VERSION"
say ""

RUN_LOG="$(mktemp -t webcodex-p1-native.XXXXXX)"
# The cases assert their own confinement and print machine-readable verdicts.
# `--nocapture` is required: the verdicts go to stderr, not to the test harness.

# Suite 1 — the real production run_shell boundary. Emits P1_NATIVE_RUN_SHELL /
# P1_NATIVE_EXTERNAL_DENY / P1_NATIVE_NETWORK_DENY / P1_NATIVE_DESCENDANT.
# Run on its own so its exit code is this command's exit code.
( cd "$REPO_ROOT" && \
  "$CARGO_BIN" test -p webcodex-runner --features workspace-checkpoints \
    --bin webcodex-runner normalization_p1 \
    -- --nocapture --test-threads=1 ) >>"$RUN_LOG" 2>&1
RUNNER_RC=$?
say "runner suite finished: rc=$RUNNER_RC"

# Suite 2 — the checkpoint wrapper, i.e. the git path production actually takes.
# `workspace_checkpoint::git_apply` is private, so this can only be driven by an
# in-module test; that is what makes it evidence about the checkpoint layer and
# not just about the broker underneath it.
#
# The checkpoint test and the broker's fidelity test are run as two separate
# cargo invocations and their exit statuses captured separately. Grouping them
# into one subshell is what previously allowed a false pass: a subshell's status
# is the status of its LAST command, so a checkpoint test that failed every
# assertion followed by a passing fidelity test yielded rc=0, while the log
# already carried P1_NATIVE_GIT_APPLY=PASS from the marker printed before the
# assertion. Fidelity is informational, so it must not share an exit status with
# the gate it is not part of.
( cd "$REPO_ROOT" && \
  "$CARGO_BIN" test -p webcodex-workspace --features workspace-checkpoints \
    checkpoint_git_apply_applies_a_real_patch_through_the_broker \
    -- --nocapture --test-threads=1 ) >>"$RUN_LOG" 2>&1
CHECKPOINT_RC=$?
say "checkpoint suite finished: rc=$CHECKPOINT_RC"

# Suite 3 — informational only. Reported, never required by the gate.
( cd "$REPO_ROOT" && \
  "$CARGO_BIN" test -p webcodex-workspace --features workspace-checkpoints \
    git_broker::tests::g_git_apply_still_applies_a_real_patch_through_the_broker \
    -- --nocapture --test-threads=1 ) >>"$RUN_LOG" 2>&1
GIT_BROKER_FIDELITY_RC=$?
say "broker fidelity suite finished: rc=$GIT_BROKER_FIDELITY_RC (informational)"
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

# The exit codes are reported as their own fields so a reader can see which
# suite failed without re-deriving it from the log.
say "P1_CARGO_BIN=$CARGO_BIN"
say "P1_RUSTC_VERSION=$P1_RUSTC_VERSION"
say "P1_RUNNER_TEST_RC=$RUNNER_RC"
say "P1_CHECKPOINT_TEST_RC=$CHECKPOINT_RC"
say "P1_GIT_BROKER_FIDELITY_TEST_RC=$GIT_BROKER_FIDELITY_RC"
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

if all_pass "$RUNNER_RC" "$CHECKPOINT_RC" "$FAIL_COUNT" "$ENV_BLOCKED"; then
  say ""
  say "P1_NATIVE_ALL_PASS=true"
  exit 0
fi

say ""
say "P1_NATIVE_ALL_PASS=false (runner_rc=$RUNNER_RC checkpoint_rc=$CHECKPOINT_RC failures=$FAIL_COUNT env_blocked=$ENV_BLOCKED)"
exit 1