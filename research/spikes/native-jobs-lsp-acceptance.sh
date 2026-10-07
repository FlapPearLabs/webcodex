#!/bin/bash
# SPDX-License-Identifier: Apache-2.0
#
# Native acceptance suite for the WebCodex ChatGPT-safe managed Jobs and the
# read-only LSP facade.
#
# WHY THIS MUST BE RUN BY HAND, FROM AN ORDINARY TERMINAL
# ------------------------------------------------------
# When this process tree is already inside a sandbox that refuses nested
# narrowing, `sandbox-exec` fails at sandbox_apply with EPERM before the
# profile has any effect. Every enforcement check then "passes" for the wrong
# reason: the child never started, so nothing was actually denied. Running
# from Terminal.app (launchd session `Aqua`, no agent sandbox wrapper) is the
# only way to measure the broker rather than the host's refusal to nest.
#
# This script therefore REFUSES to run where its result would be meaningless.
# A false "all pass" is worse than no result at all.
#
# It tests the exact current candidate bytes: it builds from the working tree
# and pins the source SHA-256 of every runtime file that defines the code this
# suite exercises, so a result can never be attributed to different code than
# the one that ran. The binding is audited against the real tree (the broker is
# a directory module, not a flat file) and is itself self-tested: a missing
# bound path, a mutated bound source, and the clean current candidate are each
# verified to behave correctly before any build or product check runs.
#
# USAGE
#   bash research/spikes/native-jobs-lsp-acceptance.sh
#   bash research/spikes/native-jobs-lsp-acceptance.sh --identity-only
#
# EXIT CODES
#   0  all required checks passed
#   1  at least one required check FAILED
#   2  preconditions not met (wrong environment, build failed, no cargo)
#   3  host cannot apply a restrictive Seatbelt profile (ENV_BLOCKED, not a pass)
#
# `--identity-only` runs the candidate-source binding and its self-test, then
# exits 0. It runs NO product check and applies NO sandbox, so it is safe on any
# host. It exists because the binding is otherwise only reachable after the
# Seatbelt precondition passes — which would leave the machinery that attributes
# a result to specific bytes unverifiable on exactly the hosts that cannot
# produce a result at all. It can never report a product PASS.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CRATE="webcodex-chatgpt-safe"

IDENTITY_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --identity-only) IDENTITY_ONLY=1 ;;
    *)
      printf 'FATAL: unknown option: %s\n' "$arg" >&2
      printf 'usage: %s [--identity-only]\n' "$0" >&2
      exit 2
      ;;
  esac
done

say()  { printf '%s\n' "$*"; }
hr()   { printf '=%.0s' {1..72}; printf '\n'; }
fail() { say "FATAL: $*"; exit 2; }

PASS_COUNT=0
FAIL_COUNT=0
declare -a FAILED_CHECKS=()

# check <name> <expected> <actual>
check() {
  local name="$1" expected="$2" actual="$3"
  if [ "$expected" = "$actual" ]; then
    PASS_COUNT=$((PASS_COUNT + 1))
    say "  PASS  $name"
  else
    FAIL_COUNT=$((FAIL_COUNT + 1))
    FAILED_CHECKS+=("$name (expected=$expected actual=$actual)")
    say "  FAIL  $name"
    say "        expected: $expected"
    say "        actual:   $actual"
  fi
}

hr
say "WebCodex native Jobs + LSP acceptance suite"
hr
say "repo:   $REPO_ROOT"
say "host:   $(uname -srm)"
say "date:   $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
say ""

# ---------------------------------------------------------------------------
# 1. Environment sanity: refuse to run where the result would be meaningless
# ---------------------------------------------------------------------------
# `--identity-only` deliberately skips this gate: it performs no product check,
# so the Seatbelt precondition is irrelevant to it.
if [ "$IDENTITY_ONLY" -eq 1 ]; then
  say "--- environment check (skipped: --identity-only) ---"
  say "no product check will run; no sandbox is applied"
  say ""
else
say "--- environment check ---"

SESSION_TYPE="$(launchctl managername 2>/dev/null || echo 'unknown')"
say "launchctl manager: $SESSION_TYPE"

if [ "$SESSION_TYPE" != "Aqua" ]; then
  say "WARNING: launchd session is '$SESSION_TYPE', not 'Aqua'."
  say "This looks like a nested or background context. The probe below is"
  say "authoritative, but record this when reporting the result."
  say ""
fi

# Ask the host directly whether it can apply a restrictive profile. This is the
# only precondition that actually matters: if it cannot, every result below is
# ENV_BLOCKED no matter what shell we are in.
#
# ONE probe. A blanket `deny file-read*` would also cut off the system reads
# /usr/bin/true needs in order to start, so it fails even on a capable host.
# `deny network*` narrows something while still letting the target run, which is
# exactly the property we need to confirm.
probe_rc=0
/usr/bin/sandbox-exec -p '(version 1)(allow default)(deny network*)' -- /usr/bin/true \
  >/dev/null 2>&1 || probe_rc=$?
if [ "$probe_rc" -ne 0 ]; then
  say "restrictive profile probe: rc=$probe_rc (host refuses narrowing)"
  say ""
  say "This host cannot apply a restrictive Seatbelt profile, so the Jobs and"
  say "LSP enforcement checks below CANNOT be measured here. This is NOT a pass."
  say "Run this script from an ordinary Terminal.app session instead."
  say "Do not report JOBS_NATIVE_SECURITY or LSP_NATIVE_SECURITY as anything"
  say "other than ENV_BLOCKED on this host."
  exit 3
fi
say "restrictive profile probe: rc=0 (host accepts narrowing)"
say ""
fi

if [ "$IDENTITY_ONLY" -ne 1 ]; then
  command -v cargo >/dev/null 2>&1 || fail "cargo not found on PATH"
fi

# ---------------------------------------------------------------------------
# 2. Pin the exact candidate bytes under test
# ---------------------------------------------------------------------------
say "--- candidate source identity ---"

# The candidate is the `webcodex-chatgpt-safe` binary. These are the runtime
# source files that define the code it actually executes, grouped by the role
# each one plays in the Jobs/LSP path under test.
#
# AUDITED against the real tree. The previous list assumed a flat
# `execution_broker.rs`, which does not exist: the broker is a DIRECTORY
# module. `ExecutionBroker`, `SpawnSpec`, `SandboxPlan`, `EnvPolicy`,
# `NetworkPolicy`, `StreamPolicy` and `BrokerError` live in `mod.rs`;
# `TrustedToolchainRoot`/`CompiledProfile` in `compiler.rs`; `WorkspaceAuthority`
# in `workspace_authority.rs`.
#
# The two `.sbpl` files are bound because they are NOT data files: `mod.rs`
# pulls them in with `include_str!` and `compiler.rs` concatenates them into
# every compiled profile. Editing either changes enforcement without changing
# any `.rs` byte, so leaving them unbound would defeat the entire purpose of
# pinning.
#
# `unix.rs` is bound because it implements `ManagedChild` — `terminate_tree`,
# `wait_tree_exit` and `try_wait` are what make a job cancellation observable
# as CANCELLED/TIMED_OUT/OUTCOME_UNKNOWN rather than an unverifiable guess.
#
# `supervisor.rs` and `protocol.rs` are bound because the LSP facade reaches the
# broker through `LspSupervisor::spawn` in `supervisor.rs`, which is the second
# broker client under test.
#
# EXCLUDED, deliberately:
#   - `execution_broker/fidelity_tests.rs` — `#[cfg(test)]`-only (declared at
#     execution_broker/mod.rs:739-741), so it does not exist in a release build
#     and cannot affect any measured behaviour.
#   - `windows.rs` — not compiled on this macOS host.
#   - `program.rs`, `src/bin/*` — program resolution helpers and separate
#     binaries, not on the Jobs/LSP spawn path.
#
# A file is bound if editing it could change what this suite observes. That set
# is exactly the list below; the self-test proves it is enforced, not decorative.
CANDIDATE_SOURCES=(
  # --- the candidate surface under test -------------------------------------
  "crates/webcodex-chatgpt-safe/src/main.rs"
  "crates/webcodex-chatgpt-safe/src/service.rs"
  # --- read-only LSP facade + the brokered supervisor behind it -------------
  "crates/webcodex-lsp/src/navigation.rs"
  "crates/webcodex-lsp/src/supervisor.rs"
  "crates/webcodex-lsp/src/protocol.rs"
  # --- the broker itself: policy types, compilation, authority --------------
  "crates/webcodex-process/src/execution_broker/mod.rs"
  "crates/webcodex-process/src/execution_broker/compiler.rs"
  "crates/webcodex-process/src/execution_broker/workspace_authority.rs"
  "crates/webcodex-process/src/lib.rs"
  # --- SBPL policies compiled into every profile via include_str! -----------
  "crates/webcodex-process/src/execution_broker/sbpl/codex_base_policy.sbpl"
  "crates/webcodex-process/src/execution_broker/sbpl/codex_read_only_platform_defaults.sbpl"
  # --- process-tree supervision that makes cancel/timeout verifiable -------
  "crates/webcodex-process/src/unix.rs"
)

# Record one `sha256  path` line per bound file. A missing path is FATAL before
# anything is built or run: an identity check that silently skips what it cannot
# find would let a result be attributed to code that never ran.
pin_source() {
  local rel="$1"
  local digest
  if [ ! -f "$REPO_ROOT/$rel" ]; then
    fail "candidate file missing: $rel"
  fi
  digest="$(shasum -a 256 "$REPO_ROOT/$rel" | cut -d' ' -f1)"
  say "  $digest  $rel"
  printf '%s  %s\n' "$digest" "$rel" >>"$PINS"
}

PINS="$(mktemp -t webcodex-native-pins.XXXXXX)"
for rel in "${CANDIDATE_SOURCES[@]}"; do
  pin_source "$rel"
done
say ""
say "  candidate fingerprint: $(shasum -a 256 "$PINS" | cut -d' ' -f1)"
say "  bound files: ${#CANDIDATE_SOURCES[@]}"
say ""

# ---------------------------------------------------------------------------
# 2b. Identity self-test: prove the binding above is actually enforced
# ---------------------------------------------------------------------------
# A binding that is never exercised is a comment. These three controls are run
# against throwaway copies, never against the real tree, and each must behave
# exactly as it would during a real run:
#
#   a) a nonexistent bound path fails BEFORE any build or execution
#   b) mutating one bound runtime source produces an identity mismatch
#   c) the unmodified current candidate validates clean
#
# (b) and (c) share one comparison function, which is the same one a real rerun
# would use against a recorded fingerprint, so a passing self-test cannot come
# from a comparison that is not actually performed.
say "--- identity self-test ---"
IDENTITY_SELFTEST="PASS"
identity_of() {
  # $1 = repo root. Prints "sha256  path" lines for every bound source.
  local root="$1" rel digest
  for rel in "${CANDIDATE_SOURCES[@]}"; do
    if [ ! -f "$root/$rel" ]; then
      echo "MISSING $rel"
      return 1
    fi
    digest="$(shasum -a 256 "$root/$rel" | cut -d' ' -f1)"
    printf '%s  %s\n' "$digest" "$rel"
  done
}

SELFTEST_DIR="$(mktemp -d -t webcodex-native-identity.XXXXXX)"
# WORK and BUILD_LOG do not exist yet at this point, so the cleanup function
# tests for them rather than expanding unset names under `set -u`.
# shellcheck disable=SC2064
trap 'cleanup_paths' EXIT
cleanup_paths() {
  [ -n "${WORK:-}" ] && rm -rf "$WORK"
  [ -n "${PINS:-}" ] && rm -f "$PINS"
  [ -n "${SELFTEST_DIR:-}" ] && rm -rf "$SELFTEST_DIR"
  [ -n "${BUILD_LOG:-}" ] && rm -f "$BUILD_LOG"
  return 0
}

# (a) a bound path that does not exist must be rejected by the same check the
#     real run uses, before anything is built.
if identity_of "$SELFTEST_DIR" >/dev/null 2>&1; then
  say "  FAIL  a nonexistent bound path was NOT rejected"
  IDENTITY_SELFTEST="FAIL"
else
  say "  PASS  a nonexistent bound path is rejected before execution"
fi

# (b) mutating one bound runtime source must change the fingerprint.
#     mod.rs is chosen because it defines ExecutionBroker, SpawnSpec,
#     SandboxPlan and the policy enums — editing it cannot fail to matter.
#     Only the bound paths are staged, so this stays cheap regardless of how
#     large the repository or its build directory is.
MUTANT_ROOT="$SELFTEST_DIR/mutant"
mkdir -p "$MUTANT_ROOT"
STAGE_OK=1
for rel in "${CANDIDATE_SOURCES[@]}"; do
  target="$MUTANT_ROOT/$rel"
  if [ -e "$target" ]; then
    # A previous iteration already staged this path.
    continue
  fi
  if ! mkdir -p "$(dirname "$target")" 2>/dev/null; then
    STAGE_OK=0
    break
  fi
  if ! cp "$REPO_ROOT/$rel" "$target" 2>/dev/null; then
    STAGE_OK=0
    break
  fi
done
if [ "$STAGE_OK" -eq 1 ]; then
  BASE_ID="$(identity_of "$MUTANT_ROOT")"
  printf '\n// identity self-test mutation\n' \
    >>"$MUTANT_ROOT/crates/webcodex-process/src/execution_broker/mod.rs"
  MUTANT_ID="$(identity_of "$MUTANT_ROOT")"
  if [ "$BASE_ID" != "$MUTANT_ID" ]; then
    say "  PASS  mutating a bound runtime source changes the identity"
  else
    say "  FAIL  mutating execution_broker/mod.rs did NOT change the identity"
    IDENTITY_SELFTEST="FAIL"
  fi

  # (c) the unmodified candidate must validate clean against itself.
  if [ "$(identity_of "$REPO_ROOT")" = "$BASE_ID" ]; then
    say "  PASS  the current candidate validates clean"
  else
    say "  FAIL  the current candidate did NOT match its own recorded identity"
    IDENTITY_SELFTEST="FAIL"
  fi
else
  say "  FAIL  could not stage the bound sources for the mutation control"
  IDENTITY_SELFTEST="FAIL"
fi
rm -rf "$MUTANT_ROOT"
say "  identity self-test: $IDENTITY_SELFTEST"
say ""

if [ "$IDENTITY_SELFTEST" != "PASS" ]; then
  fail "candidate identity self-test failed; the binding is not trustworthy"
fi

# `--identity-only` stops here on purpose: identity is proven, and no product
# check has run, so there is nothing further this mode may claim.
if [ "$IDENTITY_ONLY" -eq 1 ]; then
  hr
  say "IDENTITY_ONLY: binding verified, NO product check was run."
  say "This is NOT a Jobs or LSP acceptance result."
  hr
  exit 0
fi

# ---------------------------------------------------------------------------
# 3. Build the candidate binary
# ---------------------------------------------------------------------------
say "--- building $CRATE ---"
BUILD_LOG="$(mktemp -t webcodex-native-build.XXXXXX)"
if ! (cd "$REPO_ROOT" && cargo build --offline --locked -p "$CRATE") \
      >"$BUILD_LOG" 2>&1; then
  say "build failed; last 40 lines:"
  tail -40 "$BUILD_LOG"
  rm -f "$BUILD_LOG"
  fail "could not build the candidate binary"
fi
rm -f "$BUILD_LOG"
say "build ok"
say ""

BIN_PATH="$(cd "$REPO_ROOT" && cargo metadata --format-version 1 --no-deps 2>/dev/null \
  | /usr/bin/python3 -c 'import json,sys,os
d=json.load(sys.stdin)
print(os.path.join(d["target_directory"],"debug","webcodex-chatgpt-safe"))' 2>/dev/null || true)"
if [ -z "$BIN_PATH" ] || [ ! -x "$BIN_PATH" ]; then
  BIN_PATH="$REPO_ROOT/target/debug/webcodex-chatgpt-safe"
fi
[ -x "$BIN_PATH" ] || fail "candidate binary not found or not executable"
say "binary: $BIN_PATH"
say ""

# ---------------------------------------------------------------------------
# 4. Disposable registered project fixture
# ---------------------------------------------------------------------------
say "--- building disposable project fixture ---"
WORK="$(mktemp -d -t webcodex-native-accept.XXXXXX)"

PROJECT="$WORK/project"
REGISTRY="$WORK/registry.json"
mkdir -p "$PROJECT/src"

# A real Rust crate, so rust-analyzer has genuine symbols to resolve.
cat >"$PROJECT/Cargo.toml" <<'CRATE'
[package]
name = "native-accept-fixture"
version = "0.1.0"
edition = "2021"
CRATE

cat >"$PROJECT/src/lib.rs" <<'RS'
//! Fixture with cross-file references for LSP navigation.

pub fn helper(value: i32) -> i32 {
    value * 2
}

pub fn caller() -> i32 {
    helper(21)
}

pub fn unused_symbol() -> &'static str {
    "unreferenced"
}
RS

# Files used for the authority-denial and secret-leak checks.
echo "TOP-SECRET-CANARY-VALUE-9f3a2b" >"$WORK/host-secret.txt"
echo "canary" >"$PROJECT/.env"
mkdir -p "$PROJECT/.git"
echo "git-internals-canary" >"$PROJECT/.git/config"

# A recording listener for the network-deny check.
#
# It appends the exact bytes of every accepted connection to a log, so the
# verdict is behavioural: a token appearing in the log is the ONLY evidence
# that a connection completed. Substring counting over a job's captured output
# cannot distinguish a successful connection from a traceback echoing the
# source line that contained the marker.
#
# `timeout` bounds the listener so it cannot outlive the suite.
PY_PORT_FILE="$WORK/.listener-port"
PY_TOKEN_LOG="$WORK/.listener-tokens"
rm -f "$PY_PORT_FILE" "$PY_TOKEN_LOG"
python3 - "$PY_PORT_FILE" "$PY_TOKEN_LOG" <<'PY' &
import socket, pathlib, sys
port_file, token_log = sys.argv[1], sys.argv[2]
srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", 0))
srv.listen(8)
srv.settimeout(300)
pathlib.Path(port_file).write_text(str(srv.getsockname()[1]))
try:
    while True:
        try:
            conn, _ = srv.accept()
        except socket.timeout:
            break
        except Exception:
            break
        try:
            conn.settimeout(5)
            data = b""
            try:
                data = conn.recv(256)
            except Exception:
                data = b""
            with open(token_log, "a") as fh:
                fh.write(data.decode("utf-8", "replace").strip() + "\n")
                fh.flush()
            try:
                conn.close()
            except Exception:
                pass
        except Exception:
            pass
finally:
    try:
        srv.close()
    except Exception:
        pass
PY
LISTENER_PID=$!
rm -f "$PY_TOKEN_LOG"

for _ in $(seq 1 100); do
  [ -s "$PY_PORT_FILE" ] && break
  sleep 0.1
done
LISTENER_PORT="$(cat "$PY_PORT_FILE" 2>/dev/null || echo '')"
if [ -z "$LISTENER_PORT" ]; then
  say "WARNING: positive-control listener did not start; network checks will be skipped"
  say ""
fi

# --- network control tokens -------------------------------------------------
# Two distinct tokens. HOST_CONTROL is sent by this UNSANDBOXED shell and MUST
# be recorded; JOB_CONTROL is handed to the brokered job and MUST NOT be. They
# are generated per run and never appear in the job's command source, so a
# traceback echoing that source cannot fabricate a match.
NET_TOKEN_FILE="$WORK/.net-token"
NET_HOST_TOKEN="HOST_CONTROL_$$_$(date +%s)"
NET_JOB_TOKEN="JOB_CONTROL_$$_$(date +%s)"
printf '%s' "$NET_JOB_TOKEN" >"$NET_TOKEN_FILE"

# Host positive control: prove the listener is reachable at all. Without this, a
# brokered-job failure to connect proves nothing — it could equally mean the
# listener was already gone.
if [ -n "$LISTENER_PORT" ]; then
  if /usr/bin/python3 -c "
import socket, sys
s = socket.create_connection(('127.0.0.1', int(sys.argv[1])), 5)
s.sendall(sys.argv[2].encode())
s.close()
" "$LISTENER_PORT" "$NET_HOST_TOKEN" 2>/dev/null; then
    say "network host positive control: connected and sent its token"
  else
    say "network host positive control: FAILED to connect to the listener"
    say ""
  fi
  # Let the listener record before anyone asserts on the log.
  sleep 1
fi

# --- external filesystem canaries ------------------------------------------
# EXTERNAL: outside the registered project, marker absent from the command.
# LOCAL: inside the project, read by the same command shape as a control.
EXT_MARKER="EXT-CANARY-$$-$(date +%s)-a7f3"
LOC_MARKER="LOC-CANARY-$$-$(date +%s)-b2e9"
EXTERNAL_CANARY="$WORK/external-absolute-canary.txt"
LOCAL_CANARY="$PROJECT/src/local-canary.txt"
printf '%s\n' "$EXT_MARKER" >"$EXTERNAL_CANARY"
printf '%s\n' "$LOC_MARKER" >"$LOCAL_CANARY"
# Exported so the verdict layer counts occurrences of these exact markers. They
# are generated here and never embedded in any command string.
export EXT_MARKER LOC_MARKER
say "fixture project: $PROJECT"
say "external canary: $EXTERNAL_CANARY (outside the project)"
say "local canary:    $LOCAL_CANARY (inside the project)"
say "listener port:   ${LISTENER_PORT:-none}"
say ""

# Registry for the chatgpt-safe profile.
#
# CONTRACT (crates/webcodex-chatgpt-safe/src/main.rs:68-73,166-167):
#   #[serde(deny_unknown_fields)]
#   struct Registry { id: String, name: String, root: PathBuf }
#
# It is a FLAT object, not a list of projects, and the field is `root`, not
# `path`. `deny_unknown_fields` means both a missing and an extra key are hard
# errors. An earlier version of this harness wrote
# {"projects":[{"id","name","path"}]}, which is the shape used by a different
# tool in this repo; App::load then failed before serve() ever ran, the child
# exited, and the driver reported the result as a JSONDecodeError on empty
# stdout instead of the actual startup error.
cat >"$REGISTRY" <<JSON
{
  "id": "native-accept",
  "name": "native-accept",
  "root": "$PROJECT"
}
JSON
say "registry: $REGISTRY"

# Preflight: prove the candidate can actually load THIS registry file, using the
# same accepted `status` subcommand and the same path the MCP driver will use.
# Without this, a registry rejection only shows up as an unreadable MCP stream.
PREFLIGHT_OUT="$(mktemp -t webcodex-native-preflight.XXXXXX)"
PREFLIGHT_ERR="$(mktemp -t webcodex-native-preflight-err.XXXXXX)"
if "$BIN_PATH" status --registry "$REGISTRY" >"$PREFLIGHT_OUT" 2>"$PREFLIGHT_ERR"; then
  say "registry preflight: accepted"
  say "  $(head -c 200 "$PREFLIGHT_OUT")"
else
  preflight_rc=$?
  say "registry preflight: REJECTED (rc=$preflight_rc)"
  say "  the candidate refused the generated registry:"
  # Bounded tail only. This binary prints diagnostics, never credentials, but a
  # harness must not assume that about arbitrary future stderr.
  tail -20 "$PREFLIGHT_ERR" 2>/dev/null | head -20
  rm -f "$PREFLIGHT_OUT" "$PREFLIGHT_ERR"
  fail "candidate could not load the harness registry (MCP_CHILD_STARTUP_FAILURE precondition)"
fi
rm -f "$PREFLIGHT_OUT" "$PREFLIGHT_ERR"
say ""

# ---------------------------------------------------------------------------
# 5. Drive the real MCP surface over stdio
# ---------------------------------------------------------------------------
# A tiny JSON-RPC driver speaks the real protocol to the real binary, so these
# checks exercise the same code path the ChatGPT tunnel uses. No test-only
# bypass is used anywhere.
say "--- MCP acceptance driver ---"

# --- Stage 1: everything except the polling loop ---------------------------
cat >"$WORK/stage1.py" <<'PY'
import json, subprocess, sys, os

binary, registry, project_id, work, listener_port, out_path = sys.argv[1:7]
external_canary = sys.argv[7]
local_canary = sys.argv[8]
net_token_file = sys.argv[9]
host_control_token = sys.argv[10]
job_control_token = sys.argv[11]
W = {}
def rec(k, v): W[k] = v

proc = subprocess.Popen(
    [binary, "serve", "--profile", "chatgpt-safe", "--registry", registry],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True, bufsize=1, env={**os.environ, "HOME": work},
)
_rid = [0]

def _child_startup_failure(reason):
    """Classify an early child exit instead of degrading into a decode error.

    `readline()` returning "" means the child closed stdout. The overwhelmingly
    most likely cause is that it exited before serving — for example because it
    rejected its registry during App::load — and the real reason is on stderr.
    Reporting that as JSONDecodeError hides the actual defect, so surface the
    exit code and a bounded, redacted stderr tail instead.
    """
    try:
        rc = proc.poll()
        if rc is None:
            # Still running but closed stdout: it is not going to answer.
            try:
                proc.terminate()
                proc.wait(timeout=5)
            except Exception:
                pass
            rc = proc.poll()
    except Exception:
        rc = None
    sys.stderr.write("MCP_CHILD_STARTUP_FAILURE: %s\n" % reason)
    sys.stderr.write("  child exit code: %s\n" % ("(still running)" if rc is None else rc))
    try:
        tail = proc.stderr.read() or ""
    except Exception:
        tail = ""
    if tail.strip():
        # Bounded tail, and strip anything token-shaped before printing: this
        # output is a diagnostic, but a harness must not become a leak path.
        safe = []
        for line in tail.strip().splitlines()[-25:]:
            low = line.lower()
            if any(m in low for m in ("bearer", "api_key", "api-key", "secret",
                                      "token", "password")):
                safe.append("  <redacted: credential-shaped stderr line>")
            else:
                safe.append("  " + line[:400])
        sys.stderr.write("  child stderr (bounded, redacted):\n" + "\n".join(safe) + "\n")
    else:
        sys.stderr.write("  child stderr: (empty)\n")
    sys.stderr.flush()
    raise SystemExit(3)


def send(method, params=None, notify=False):
    _rid[0] += 1
    msg = {"jsonrpc": "2.0", "method": method}
    if not notify:
        msg["id"] = _rid[0]
    if params is not None:
        msg["params"] = params
    try:
        proc.stdin.write(json.dumps(msg) + "\n"); proc.stdin.flush()
    except (BrokenPipeError, OSError) as exc:
        _child_startup_failure("could not write %s to the child (%s)" % (method, exc))
    if notify: return None
    try:
        line = proc.stdout.readline()
    except Exception as exc:
        _child_startup_failure("could not read the %s response (%s)" % (method, exc))
    if line == "":
        _child_startup_failure(
            "child closed stdout before answering %s" % method)
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        sys.stderr.write(
            "MCP_CHILD_STARTUP_FAILURE: non-JSON reply to %s: %r\n"
            % (method, line[:400]))
        sys.stderr.flush()
        raise SystemExit(3)

def call(name, args):
    return send("tools/call", {"name": name, "arguments": args})

try:
    send("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                        "clientInfo": {"name": "native-accept", "version": "1"}})
    send("notifications/initialized", notify=True)
    rec("tools", [t["name"] for t in send("tools/list")["result"]["tools"]])
    rec("project_select", call("project_select", {"project_id": project_id}))
    # CONTRACT: project_current requires {"project_id": ...}
    # (main.rs:517-518 exact_keys(&args,["project_id"]); schema required:["project_id"]).
    # Calling it with {} returns -32602, which would make this check fail for a
    # schema reason rather than because the project identity is wrong.
    rec("project_current", call("project_current", {"project_id": project_id}))

    rec("files_search", call("files_search", {"project_id": project_id, "query": "helper"}))
    rec("files_read", call("files_read", {"project_id": project_id, "path": "src/lib.rs",
                                         "offset": 0, "limit": 40}))
    rec("read_parent", call("files_read", {"project_id": project_id, "path": "../host-secret.txt",
                                           "offset": 0, "limit": 40}))
    rec("read_dotenv", call("files_read", {"project_id": project_id, "path": ".env",
                                           "offset": 0, "limit": 40}))
    rec("read_git", call("files_read", {"project_id": project_id, "path": ".git/config",
                                        "offset": 0, "limit": 40}))

    rec("lsp_symbols", call("lsp_symbols", {"project_id": project_id, "path": "src/lib.rs", "limit": 50}))
    rec("lsp_definition", call("lsp_definition", {"project_id": project_id, "path": "src/lib.rs",
                                                  "line": 9, "column": 12, "limit": 10}))
    rec("lsp_references", call("lsp_references", {"project_id": project_id, "path": "src/lib.rs",
                                                  "line": 4, "column": 8,
                                                  "include_declaration": True, "limit": 20}))
    rec("lsp_diagnostics", call("lsp_diagnostics", {"project_id": project_id, "path": "src/lib.rs", "limit": 50}))
    rec("lsp_escape", call("lsp_symbols", {"project_id": project_id, "path": "../host-secret.txt", "limit": 10}))
    # NEGATIVE CONTROLS. These inject fields the schemas do not declare, so the
    # candidate must reject them at the ARGUMENT LAYER (-32602 from
    # exact_keys_optional) before any LSP is spawned. That is the correct and
    # desired security outcome — but note the rejection is a schema rejection,
    # not an LSP-level refusal. Do not "fix" a failure here by adding these
    # fields to the lsp_* schemas: a field the model may send is a field the
    # model controls. lsp_definition declares only [limit] as optional;
    # lsp_references only [include_declaration, limit].
    rec("lsp_exec_arg", call("lsp_definition", {"project_id": project_id, "path": "src/lib.rs",
                                               "line": 1, "column": 1, "limit": 10,
                                               "executable": "/bin/sh"}))
    rec("lsp_exec_cmd", call("lsp_references", {"project_id": project_id, "path": "src/lib.rs",
                                               "line": 1, "column": 1, "limit": 10,
                                               "executeCommand": "workspace/executeCommand"}))

    def start(name, command, timeout):
        resp = call("job_start", {"project_id": project_id, "cwd": ".", "command": command,
                                  "timeout_seconds": timeout})
        try:
            job_id = resp["result"]["structuredContent"]["job_id"]
        except Exception:
            # Record the ACTUAL MCP error rather than degrading to a MISSING
            # status later. A refused start (e.g. the concurrency limit) is a
            # harness capacity bug, not a product lifecycle verdict.
            rec(name + "_start_error", resp)
            return None
        cursors.setdefault(name, {"stdout": 0, "stderr": 0})
        seen.setdefault(name, [])
        return job_id

    # ---------------------------------------------------------------------
    # PHASED JOB EXECUTION
    # ---------------------------------------------------------------------
    # The product bounds LIVE jobs at MAX_CONCURRENT_JOBS=8. The previous
    # version started eight authority/lifecycle jobs back to back and only then
    # started `cancel`, making it the 9th concurrent start. On a real run that
    # returned JOB_LIMIT_REACHED and `cancel` never existed, which surfaced as a
    # misleading MISSING rather than as a capacity bug in this harness.
    #
    # Phasing removes all dependence on scheduler timing: a phase is only
    # started after enough of the previous phase is terminal, so the suite never
    # requires more live jobs than the product documents. The product limit is
    # NOT raised to accommodate the suite.
    ids = {}
    # Seeded lazily. These used to be comprehensions over `ids`, evaluated before
    # any start() call — which captured ZERO keys, so the first poll raised
    # KeyError and aborted the whole driver before a single Job check ran. Each
    # successful start() now seeds its own entry via setdefault().
    cursors = {}
    seen = {}
    final = {}

    def poll_until_terminal(keys, max_seconds):
        """Poll `keys` until every one of them is terminal (or time runs out)."""
        import time as _time
        deadline = _time.time() + max_seconds
        while True:
            pending = 0
            for key in keys:
                job_id = ids.get(key)
                if not job_id:
                    # Terminal-by-absence: a refused start is already recorded.
                    continue
                cursors.setdefault(key, {"stdout": 0, "stderr": 0})
                seen.setdefault(key, [])
                resp = call("job_poll", {"project_id": project_id, "job_id": job_id,
                                         "stdout_cursor": cursors[key]["stdout"],
                                         "stderr_cursor": cursors[key]["stderr"]})
                try:
                    sc = resp["result"]["structuredContent"]
                except Exception:
                    rec(key + "_poll_error", resp)
                    continue
                if sc.get("stdout_delta") or sc.get("stderr_delta"):
                    seen[key].append({"stdout": sc.get("stdout_delta", ""),
                                      "stderr": sc.get("stderr_delta", "")})
                prev_out = cursors[key]["stdout"]
                cursors[key]["stdout"] = sc.get("stdout_cursor", prev_out)
                cursors[key]["stderr"] = sc.get("stderr_cursor", cursors[key]["stderr"])
                if cursors[key]["stdout"] < prev_out:
                    rec("cursor_regressed", {"key": key, "prev": prev_out,
                                             "now": cursors[key]["stdout"]})
                final[key] = sc
                if sc.get("status") != "JOB_RUNNING":
                    continue
                pending += 1
            if pending == 0:
                return
            if _time.time() > deadline:
                rec("phase_timeout", {"pending": [k for k in keys if ids.get(k)]})
                return
            _time.sleep(1)

    # --- PHASE A: lifecycle (at most 4 live jobs) ------------------------
    ids["long"] = start("long", "echo JOB-PART-1; sleep 1; echo JOB-PART-2; sleep 34; echo JOB-DONE", 150)
    ids["fail"] = start("fail", "echo TO-FAIL; exit 7", 60)
    ids["timeout"] = start("timeout", "sleep 300", 3)
    ids["cancel"] = start("cancel", "echo BEFORE-CANCEL; sleep 300", 150)
    if ids["cancel"]:
        rec("cancel_request", call("job_cancel", {"project_id": project_id, "job_id": ids["cancel"]}))
    poll_until_terminal(["long", "fail", "timeout", "cancel"], 240)

    # --- PHASE B: authority / security (at most 5 live jobs) --------------
    ids["secret"] = start("secret", "cat ../host-secret.txt; echo CANARY-END", 30)
    # EXTERNAL FILESYSTEM DENIAL.
    #
    # The previous check ran `cat /etc/hosts; echo EXTERNAL-END` and asserted the
    # marker never appeared. That assertion was invalid twice over: the `echo`
    # runs whether or not the `cat` succeeded, and `/etc/hosts` is a poor probe
    # because minimum platform/runtime read allowances may legitimately include
    # system files — so it tests policy, not authority.
    #
    # This instead uses a unique canary OUTSIDE the registered project whose
    # marker deliberately does NOT appear in the command string, so the only way
    # for it to reach the output is if the broker actually permitted the read.
    # The local_positive job proves the same command SHAPE succeeds for an
    # in-project file, which is what makes the negative meaningful.
    ids["external"] = start("external", "cat '%s'" % external_canary, 30)
    ids["local_positive"] = start("local_positive", "cat '%s'" % local_canary, 30)
    ids["env"] = start("env", "echo ENVLEAK; env | sort", 30)
    if listener_port:
        # NETWORK DENIAL — behavioural proof, not substring counting.
        #
        # The previous check ran a Python one-liner containing `print('CONNECTED')`
        # and asserted the string never appeared in stdout/stderr. Invalid: on a
        # connection failure Python prints a traceback that echoes the offending
        # source line, so the marker could appear WITHOUT a successful connection.
        #
        # This proves it behaviourally against a recording listener:
        #   1. the UNSANDBOXED host connects and sends HOST_CONTROL -> must be
        #      recorded, proving the listener is reachable and the protocol works;
        #   2. the BROKERED job attempts the same connection and sends JOB_CONTROL
        #      -> must never be recorded.
        # The token is read from a FILE by the job, never embedded in the job's
        # own source line, so a traceback cannot echo it.
        ids["net"] = start("net",
            "python3 -c \"import socket;s=socket.create_connection(('127.0.0.1',%s),3);"
            "s.sendall(open('%s').read());print('NETJOB-REACHED-LISTENER');s.close()\""
            % (listener_port, net_token_file), 30)
    poll_until_terminal(["secret", "external", "local_positive", "env", "net"], 180)

    # Every required job must have started. A refused start is a harness
    # capacity bug and is reported as such, never as a missing status.
    for required in ["long", "fail", "timeout", "cancel",
                     "secret", "external", "local_positive", "env"]:
        rec(required + "_started", bool(ids.get(required)))
    rec("net_started", bool(ids.get("net")) if listener_port else "listener-unavailable")

    rec("job_ids", ids)
    rec("job_deltas", seen)
    rec("job_final", final)
    # Read the recording listener's log and record which control tokens it
    # actually received. Behavioural evidence, unlike scanning job output for a
    # marker that a traceback could echo.
    token_log = os.path.join(work, ".listener-tokens")
    tokens = []
    try:
        with open(token_log) as fh:
            tokens = [ln.strip() for ln in fh if ln.strip()]
    except Exception:
        tokens = []
    rec("listener_tokens", tokens)
    rec("host_control_recorded", host_control_token in tokens)
    rec("job_control_recorded", job_control_token in tokens)
finally:
    try: proc.stdin.close()
    except Exception: pass
    proc.terminate()
    try: proc.wait(timeout=10)
    except Exception: proc.kill()

json.dump(W, open(out_path, "w"), indent=1)
PY

say "running the MCP acceptance driver against the real binary..."
if ! /usr/bin/python3 "$WORK/stage1.py" "$BIN_PATH" "$REGISTRY" "native-accept" \
      "$WORK" "${LISTENER_PORT:-}" "$WORK/result.json" \
      "$EXTERNAL_CANARY" "$LOCAL_CANARY" "$NET_TOKEN_FILE" \
      "$NET_HOST_TOKEN" "$NET_JOB_TOKEN" \
      >"$WORK/stage1.log" 2>&1; then
  say "driver failed; last 40 lines of driver log:"
  tail -40 "$WORK/stage1.log"
  say ""
  say "driver stdout/stderr:"
  fail "acceptance driver did not complete"
fi
say "driver completed"
say ""

kill "$LISTENER_PID" 2>/dev/null || true

[ -f "$WORK/result.json" ] || fail "driver produced no result file"
RESULT="$WORK/result.json"

say "--- RESULTS ---"
say ""

sc_of() { /usr/bin/python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
node=d
for k in sys.argv[2].split("."):
    if isinstance(node,list): node=node[int(k)]
    else: node=node.get(k)
print(json.dumps(node) if not isinstance(node,str) else node)
' "$RESULT" "$1" 2>/dev/null || echo '<missing>'; }

is_error() { /usr/bin/python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
try:
    n=d
    for k in sys.argv[2].split("."): n=n[k]
    print("true" if n.get("isError") else "false")
except Exception:
    print("unknown")
' "$RESULT" "$1" 2>/dev/null || echo 'unknown'; }

# Result readers. Each returns a normalized token so a missing key or a shape
# change reads as an explicit failure rather than silently comparing equal.
#
#   outcome <key>      -> OK | DENIED | ERROR:<code> | MISSING
#   success <key>      -> true | false
#   value <key>        -> the raw scalar at that key, or MISSING
#   count_matches <key> <needle>  -> number of times needle appears in that
#                                    value serialized to JSON
RESULT="$WORK/result.json"
readout() {
  # Forward a possible 4th argument so `count` receives its needle; without
  # this the needle silently arrives as "" and every count matches the whole
  # document, which would turn a real secret leak into a silent pass.
  /usr/bin/python3 - "$RESULT" "$1" "$2" "${3:-}" <<'PYEOF'
import json, sys
result_path, key, mode = sys.argv[1], sys.argv[2], sys.argv[3]
# NOTE: sys.argv[0] is "-" because this script is fed to python via a heredoc.
try:
    d = json.load(open(result_path))
except Exception:
    print("MISSING"); raise SystemExit(0)
# A dotted key addresses a nested path; a plain key addresses a top-level entry.
root = key.split(".")[0]
if root not in d:
    print("MISSING"); raise SystemExit(0)
node = d[root]

if mode == "outcome":
    # A tools/call denial is either a JSON-RPC error or isError with a
    # structuredContent whose success is false. A JSON-RPC error counts as a
    # rejection, which is what the schema/path gates produce.
    if isinstance(node, dict) and "error" in node and "result" not in node:
        print("DENIED"); raise SystemExit(0)
    if isinstance(node, dict) and "result" in node:
        r = node["result"]
        if r.get("isError") is True:
            print("DENIED"); raise SystemExit(0)
        sc = r.get("structuredContent")
        if isinstance(sc, dict) and sc.get("success") is False:
            print("DENIED"); raise SystemExit(0)
        if isinstance(sc, dict) and "outcome" in sc and sc["outcome"] not in ("OK", "SUCCESS"):
            print("DENIED"); raise SystemExit(0)
        print("OK"); raise SystemExit(0)
    print("MISSING"); raise SystemExit(0)

if mode == "value":
    # Walk the explicit dotted path. Callers name the whole path, e.g.
    # project_current.structuredContent -> project_id, so no implicit descent
    # is performed here (an implicit hop would consume the next segment).
    cur = d
    for part in key.split("."):
        if part == "value":
            continue
        if not isinstance(cur, dict) or part not in cur:
            print("MISSING"); raise SystemExit(0)
        cur = cur[part]
    print(cur if isinstance(cur, str) else json.dumps(cur, separators=(",", ":")))
    raise SystemExit(0)

if mode == "count":
    # Search this section plus job_final, because a marker can land in an
    # incremental delta or only in the terminal snapshot. The needle is the
    # 4th shell argument (sys.argv[4] under `python -`).
    needle = sys.argv[4] if len(sys.argv) > 4 else ""
    text = json.dumps(node, separators=(",", ":")) + json.dumps(
        d.get("job_final", {}), separators=(",", ":"))
    print(text.count(needle))
    raise SystemExit(0)

if mode == "tool_names":
    # The driver records the tool list directly, but tolerate a raw
    # tools/list response shape as well.
    try:
        if isinstance(node, list):
            names = [t["name"] if isinstance(t, dict) else t for t in node]
        else:
            names = [t["name"] for t in node["result"]["tools"]]
        print(json.dumps(names, separators=(",", ":")))
    except Exception:
        print("MISSING")
    raise SystemExit(0)

print("MISSING")
PYEOF
}

# --- Tool surface ---
say "MCP tool surface"
EXPECTED_TOOLS='["project_list","project_select","project_current","files_search","files_read","files_apply_patch","shell_run","job_start","job_poll","job_cancel","git_status","git_diff","lsp_symbols","lsp_definition","lsp_references","lsp_diagnostics"]'
ACTUAL_TOOLS="$(readout tools tool_names)"
check "tool surface is exactly the reviewed 16" "$EXPECTED_TOOLS" "$ACTUAL_TOOLS"
say ""

# --- Project state ---
say "Project state"
check "project_select succeeded" "OK" "$(readout project_select outcome)"
check "project_current reports the selected project" "native-accept" \
      "$(readout project_current.result.structuredContent.project_id value)"
say ""

# --- Files ---
say "Files"
check "files_search succeeded" "OK" "$(readout files_search outcome)"
check "files_read succeeded" "OK" "$(readout files_read outcome)"
check "parent-escape read is denied" "DENIED" "$(readout read_parent outcome)"
check "dotenv read is denied" "DENIED" "$(readout read_dotenv outcome)"
check "git-internals read is denied" "DENIED" "$(readout read_git outcome)"
say ""

# --- LSP ---
say "LSP (read-only facade)"
# NOTE: an LSP_UNAVAILABLE / server-unavailable outcome reads as DENIED here,
# which is intentional: the harness must not report a pass on a host where the
# language server never started.
#
# When a positive LSP call does NOT succeed, print the product's own structured
# code/message/status/path so the report names a root cause instead of collapsing
# every failure into "DENIED". The product preserves the underlying cause; hiding
# it behind one word is what made the previous run unclassifiable.
#
# The message is bounded and path-scrubbed: absolute paths are redacted so a
# harness report cannot leak the operator's directory layout.
lsp_diagnose() {
  /usr/bin/python3 -c '
import json, re, sys
d = json.load(open(sys.argv[1]))
key = sys.argv[2]
sc = ((d.get(key) or {}).get("result") or {}).get("structuredContent") or {}
msg = str(sc.get("message", ""))
# Redact absolute POSIX/macOS paths; keep the shape of the message.
msg = re.sub(r"/(?:[^\s/:]+/)+[^\s/:]*", "<path>", msg)
if len(msg) > 400:
    msg = msg[:400] + "...(truncated)"
print("    %s: status=%s code=%s success=%s path=%s" % (
    key, sc.get("status", "<none>"), sc.get("code", "<none>"),
    sc.get("success", "<none>"), sc.get("path", "<none>")))
if msg:
    print("      message: %s" % msg)
' "$RESULT" "$1" 2>/dev/null || echo "    $1: <diagnostic unavailable>"
}

for lsp_key in lsp_symbols lsp_definition lsp_references lsp_diagnostics; do
  lsp_state="$(readout "$lsp_key" outcome)"
  if [ "$lsp_state" != "OK" ]; then
    say "  LSP $lsp_key did not succeed; product-reported cause:"
    lsp_diagnose "$lsp_key"
  fi
done

check "lsp_symbols succeeded" "OK" "$(readout lsp_symbols outcome)"
check "lsp_definition succeeded" "OK" "$(readout lsp_definition outcome)"
check "lsp_references succeeded" "OK" "$(readout lsp_references outcome)"
check "lsp_diagnostics succeeded" "OK" "$(readout lsp_diagnostics outcome)"
check "lsp path escape is denied" "DENIED" "$(readout lsp_escape outcome)"
check "lsp executable injection is rejected" "DENIED" "$(readout lsp_exec_arg outcome)"
check "lsp workspace/executeCommand is rejected" "DENIED" "$(readout lsp_exec_cmd outcome)"
# The host secret lives outside the registered project; no LSP result may echo it.
check "no host secret appears in any LSP result" "0" "$(readout lsp_symbols count TOP-SECRET-CANARY)"
check "no host secret appears via lsp_references" "0" "$(readout lsp_references count TOP-SECRET-CANARY)"
say ""

# --- Jobs ---
say "Jobs"

# Every REQUIRED job must have started. A refused start is a harness capacity
# bug (the suite must never exceed the documented product concurrency limit), so
# it is reported explicitly with the product's own error instead of degrading
# into a MISSING status that looks like a lifecycle defect.
for required in long fail timeout cancel secret external local_positive env; do
  check "job '$required' started" "MISSING" "$(readout "${required}_start_error" value)"
done
check "no job recorded a start error" "MISSING" "$(readout long_start_error value)"
# A phase that runs out of time leaves jobs non-terminal; that must be named
# rather than surfacing later as confusing per-status failures.
check "no phase timed out before reaching terminal state" "MISSING" \
      "$(readout phase_timeout value)"
# `net_started` is recorded by the driver; assert it so an absent network job is
# reported here instead of only as MISSING inside the network checks.
check "network job started (or listener was unavailable)" "listener-unavailable" \
      "$(readout net_started value)"
say ""

jobfield() { /usr/bin/python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
try:
    v=d["job_final"][sys.argv[2]][sys.argv[3]]
except Exception:
    print("MISSING"); raise SystemExit(0)
print(v if isinstance(v,str) else json.dumps(v))
' "$RESULT" "$1" "$2"; }

check "long job reached SUCCEEDED" "SUCCEEDED" "$(jobfield long status)"
check "long job exceeded 30s" "true" "$(/usr/bin/python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
ms=d.get("job_final",{}).get("long",{}).get("duration_ms")
print("true" if isinstance(ms,(int,float)) and ms>=30000 else "false")' "$RESULT")"
check "long job produced incremental deltas" "true" "$(/usr/bin/python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
print("true" if len(d.get("job_deltas",{}).get("long",[]))>=2 else "false")' "$RESULT")"
check "long job stdout contains its final marker" "yes" "$( [ "$(readout job_deltas count JOB-DONE)" -ge 1 ] && echo yes || echo no )"
check "no cursor ever regressed" "MISSING" "$(readout cursor_regressed value)"

check "failing job reached FAILED" "FAILED" "$(jobfield fail status)"
check "failing job reported its exit code" "7" "$(jobfield fail exit_code)"

check "timeout job reached TIMED_OUT" "TIMED_OUT" "$(jobfield timeout status)"
check "cancel job reached CANCELLED" "CANCELLED" "$(jobfield cancel status)"

check "host secret is not readable by a job" "0" \
      "$(readout job_deltas count TOP-SECRET-CANARY-VALUE)"

# --- external filesystem: marker absent from output, and a positive control ---
# The marker is generated per run and does NOT appear in the command string, so
# the only way it can appear in captured output is if the broker allowed the read.
check "project-external absolute canary never appears in job output" "0" \
      "$(readout job_deltas count "$EXT_MARKER")"
check "project-external read did not succeed" "yes" \
      "$( [ "$(jobfield external status)" = "SUCCEEDED" ] && echo no || echo yes )"
# Positive control: the SAME command shape against an in-project file must work,
# otherwise "no marker" would prove nothing (it could just mean `cat` is missing).
check "project-local canary IS readable by the same command shape" "1" \
      "$( [ "$(readout job_deltas count "$LOC_MARKER")" -ge 1 ] && echo 1 || echo 0 )"

# The authoritative network proof is exactly the token pair plus the job's own
# verdict:
#   HOST_CONTROL recorded = true   (the listener is reachable and the protocol works)
#   JOB_CONTROL  recorded = false  (the brokered job never completed a connection)
#   brokered network job  != SUCCEEDED
# The older "listener never accepted a connection" line was removed: it was
# derived from a stale marker file, it contradicted the host positive control
# (which necessarily DOES accept a connection), and it proved nothing.
# The host control proves the listener is reachable, so a brokered-job failure to
# connect is attributable to the sandbox rather than a dead listener.
check "host positive control reached the listener" "true" \
      "$(/usr/bin/python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
print("true" if d.get("host_control_recorded") else "false")' "$RESULT")"
check "brokered job never reached the listener" "false" \
      "$(/usr/bin/python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
print("true" if d.get("job_control_recorded") else "false")' "$RESULT")"
check "brokered job did not report a successful connection" "yes" \
      "$( [ "$(jobfield net status)" = "SUCCEEDED" ] && echo no || echo yes )"
check "job environment carries no host secret" "0" \
      "$(readout job_deltas count TOP-SECRET-CANARY-VALUE)"
say ""

hr
say "SUMMARY"
hr
say "required checks passed: $PASS_COUNT"
say "required checks failed: $FAIL_COUNT"
say ""

# INDEPENDENT RESULT CLASSIFICATION.
#
# Deriving both verdicts from a single FAIL_COUNT loses causality: a pure LSP
# availability failure (the language server never started) would rewrite a
# proven Jobs lifecycle result to FAIL, and vice versa. Each verdict is derived
# from the failures that actually belong to its surface.
#
# This is classification, NOT skipping: every check above still ran and still
# prints PASS or FAIL, and the FAILED CHECKS list below is exhaustive.
declare -a FAILED_JOBS=() FAILED_LSP=() FAILED_SECURITY=()
for entry in "${FAILED_CHECKS[@]}"; do
  # Order matters. A refused start is a HARNESS CAPACITY problem, not a security
  # event: attributing it to the security surface would inflate that verdict's
  # blast radius and contradict the start-error reporting above.
  case "$entry" in
    *start_error*|*started*|*phase_timeout*)
      FAILED_JOBS+=("$entry") ;;
    *lsp_symbols*|*lsp_definition*|*lsp_references*|*lsp_diagnostics*)
      FAILED_LSP+=("$entry") ;;
    *host\ secret*|*external*|*project-local\ canary*|*network*|*listener*|*positive-control*)
      FAILED_SECURITY+=("$entry")
      FAILED_JOBS+=("$entry") ;;
    *job*|*cursor*|*timeout*|*cancel*|*deltas*|*secret*|*canary*)
      FAILED_JOBS+=("$entry") ;;
    *)
      # Unclassified failures are attributed to ALL surfaces: an unknown
      # failure must never silently pass by landing in no bucket.
      FAILED_JOBS+=("$entry")
      FAILED_LSP+=("$entry")
      FAILED_SECURITY+=("$entry") ;;
  esac
done

if [ "$FAIL_COUNT" -ne 0 ]; then
  say "FAILED CHECKS (all of them, by surface):"
  if [ "${#FAILED_JOBS[@]}" -gt 0 ]; then
    say "  Jobs / security:"
    for f in "${FAILED_JOBS[@]}"; do say "    - $f"; done
  fi
  if [ "${#FAILED_LSP[@]}" -gt 0 ]; then
    say "  LSP:"
    for f in "${FAILED_LSP[@]}"; do say "    - $f"; done
  fi
  say ""
fi

JOBS_VERDICT=PASS
LSP_VERDICT=PASS
SECURITY_VERDICT=PASS
[ "${#FAILED_JOBS[@]}" -gt 0 ] && JOBS_VERDICT=FAIL
[ "${#FAILED_LSP[@]}" -gt 0 ] && LSP_VERDICT=FAIL
[ "${#FAILED_SECURITY[@]}" -gt 0 ] && SECURITY_VERDICT=FAIL

say "NATIVE_JOBS_ACCEPTANCE=$JOBS_VERDICT"
say "NATIVE_LSP_ACCEPTANCE=$LSP_VERDICT"
say "NATIVE_SECURITY_ACCEPTANCE=$SECURITY_VERDICT"
say ""
say "These verdicts are INDEPENDENT. A FAIL on one surface does not rewrite the"
say "other: read each line against the failure list above."

if [ "$FAIL_COUNT" -ne 0 ]; then
  exit 1
fi
exit 0
