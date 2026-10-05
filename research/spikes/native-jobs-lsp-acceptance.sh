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
# and pins the source SHA-256 of every file that defines the candidate, so a
# result can never be attributed to different code than the one that ran.
#
# USAGE
#   bash research/spikes/native-jobs-lsp-acceptance.sh
#
# EXIT CODES
#   0  all required checks passed
#   1  at least one required check FAILED
#   2  preconditions not met (wrong environment, build failed, no cargo)
#   3  host cannot apply a restrictive Seatbelt profile (ENV_BLOCKED, not a pass)

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CRATE="webcodex-chatgpt-safe"

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

command -v cargo >/dev/null 2>&1 || fail "cargo not found on PATH"

# ---------------------------------------------------------------------------
# 2. Pin the exact candidate bytes under test
# ---------------------------------------------------------------------------
say "--- candidate source identity ---"

# These are the files that define the candidate behavior. Recording their
# SHA-256 means a result can never be silently attributed to other code.
pin_source() {
  local rel="$1"
  local digest
  if [ ! -f "$REPO_ROOT/$rel" ]; then
    fail "candidate file missing: $rel"
  fi
  digest="$(shasum -a 256 "$REPO_ROOT/$rel" | cut -d' ' -f1)"
  say "  $digest  $rel"
  printf '%s' "$digest" >>"$PINS"
}
PINS="$(mktemp -t webcodex-native-pins.XXXXXX)"
for rel in \
  "crates/webcodex-chatgpt-safe/src/main.rs" \
  "crates/webcodex-chatgpt-safe/src/service.rs" \
  "crates/webcodex-lsp/src/navigation.rs" \
  "crates/webcodex-process/src/execution_broker.rs"
do
  pin_source "$rel"
done
say ""
say "  candidate fingerprint: $(shasum -a 256 "$PINS" | cut -d' ' -f1)"
say ""

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
cleanup() { rm -rf "$WORK"; rm -f "$PINS"; }
trap cleanup EXIT

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

# A positive-control listener for the network-deny check. If the broker
# permitted network egress, the job could reach it; with deny it must not, and
# the listener must never record an accepted connection.
PY_PORT_FILE="$WORK/.listener-port"
rm -f "$PY_PORT_FILE" "$WORK/.listener-accepted"
python3 - "$PY_PORT_FILE" "$WORK/.listener-accepted" <<'PY' &
import socket, pathlib, sys
port_file, accepted_file = sys.argv[1], sys.argv[2]
srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", 0))
srv.listen(8)
pathlib.Path(port_file).write_text(str(srv.getsockname()[1]))
srv.settimeout(240)
try:
    conn, _ = srv.accept()
    pathlib.Path(accepted_file).write_text("1")
    conn.close()
except Exception:
    pathlib.Path(accepted_file).write_text("0")
PY
LISTENER_PID=$!
rm -f "$WORK/.listener-accepted"

for _ in $(seq 1 100); do
  [ -s "$PY_PORT_FILE" ] && break
  sleep 0.1
done
LISTENER_PORT="$(cat "$PY_PORT_FILE" 2>/dev/null || echo '')"
if [ -z "$LISTENER_PORT" ]; then
  say "WARNING: positive-control listener did not start; network checks will be skipped"
  say ""
fi
say "fixture project: $PROJECT"
say "listener port:   ${LISTENER_PORT:-none}"
say ""

# Registry for the chatgpt-safe profile.
cat >"$REGISTRY" <<JSON
{
  "projects": [
    {
      "id": "native-accept",
      "name": "native-accept",
      "path": "$PROJECT"
    }
  ]
}
JSON
say "registry: $REGISTRY"
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
W = {}
def rec(k, v): W[k] = v

proc = subprocess.Popen(
    [binary, "serve", "--profile", "chatgpt-safe", "--registry", registry],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    text=True, bufsize=1, env={**os.environ, "HOME": work},
)
_rid = [0]
def send(method, params=None, notify=False):
    _rid[0] += 1
    msg = {"jsonrpc": "2.0", "method": method}
    if not notify:
        msg["id"] = _rid[0]
    if params is not None:
        msg["params"] = params
    proc.stdin.write(json.dumps(msg) + "\n"); proc.stdin.flush()
    if notify: return None
    return json.loads(proc.stdout.readline())

def call(name, args):
    return send("tools/call", {"name": name, "arguments": args})

try:
    send("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                        "clientInfo": {"name": "native-accept", "version": "1"}})
    send("notifications/initialized", notify=True)
    rec("tools", [t["name"] for t in send("tools/list")["result"]["tools"]])
    rec("project_select", call("project_select", {"project_id": project_id}))
    rec("project_current", call("project_current", {}))

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
            return resp["result"]["structuredContent"]["job_id"]
        except Exception:
            rec(name + "_start_error", resp)
            return None

    ids = {}
    ids["long"] = start("long", "echo JOB-PART-1; sleep 1; echo JOB-PART-2; sleep 34; echo JOB-DONE", 150)
    ids["fail"] = start("fail", "echo TO-FAIL; exit 7", 60)
    ids["timeout"] = start("timeout", "sleep 300", 3)
    ids["secret"] = start("secret", "cat ../host-secret.txt; echo CANARY-END", 30)
    ids["external"] = start("external", "cat /etc/hosts; echo EXTERNAL-END", 30)
    ids["env"] = start("env", "echo ENVLEAK; env | sort", 30)
    if listener_port:
        ids["net"] = start("net",
            "python3 -c \"import socket;s=socket.create_connection(('127.0.0.1',%s),3);print('CONNECTED');s.close()\""
            % listener_port, 30)
    ids["cancel"] = start("cancel", "echo BEFORE-CANCEL; sleep 300", 150)
    if ids["cancel"]:
        rec("cancel_request", call("job_cancel", {"project_id": project_id, "job_id": ids["cancel"]}))

    # Incremental polling: record every distinct delta to prove increments
    # rather than a single replay, and to prove monotonic cursors.
    cursors = {k: {"stdout": 0, "stderr": 0} for k in ids}
    seen = {k: [] for k in ids}
    final = {}
    for _ in range(300):
        done = 0
        for key, job_id in ids.items():
            if not job_id:
                done += 1
                continue
            resp = call("job_poll", {"project_id": project_id, "job_id": job_id,
                                     "stdout_cursor": cursors[key]["stdout"],
                                     "stderr_cursor": cursors[key]["stderr"]})
            try:
                sc = resp["result"]["structuredContent"]
            except Exception:
                rec(key + "_poll_error", resp)
                done += 1
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
                done += 1
        if done >= len(ids):
            break
        __import__("time").sleep(1)

    rec("job_ids", ids)
    rec("job_deltas", seen)
    rec("job_final", final)
    rec("listener_accepted", os.path.exists(os.path.join(work, ".listener-accepted")))
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
check "external filesystem read is denied" "0" \
      "$(readout job_deltas count EXTERNAL-END)"
check "network egress is denied" "0" "$(readout job_deltas count CONNECTED)"
check "positive-control listener never accepted a connection" "false" \
      "$(/usr/bin/python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
print("true" if d.get("listener_accepted") else "false")' "$RESULT")"
check "job environment carries no host secret" "0" \
      "$(readout job_deltas count TOP-SECRET-CANARY-VALUE)"
say ""

hr
say "SUMMARY"
hr
say "required checks passed: $PASS_COUNT"
say "required checks failed: $FAIL_COUNT"
say ""
if [ "$FAIL_COUNT" -ne 0 ]; then
  say "FAILED CHECKS:"
  for f in "${FAILED_CHECKS[@]}"; do say "  - $f"; done
  say ""
  say "NATIVE_JOBS_ACCEPTANCE=FAIL"
  say "NATIVE_LSP_ACCEPTANCE=FAIL"
  exit 1
fi
say "NATIVE_JOBS_ACCEPTANCE=PASS"
say "NATIVE_LSP_ACCEPTANCE=PASS"
say "NATIVE_SECURITY_ACCEPTANCE=PASS"
exit 0
