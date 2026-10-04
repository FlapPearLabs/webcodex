#!/usr/bin/env python3
"""Mechanical stdio-MCP boundary probes for webcodex-chatgpt-safe."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import queue
import shlex
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path


EXPECTED_TOOLS = {
    "project_list",
    "project_select",
    "files_search",
    "files_read",
    "files_apply_patch",
    "shell_run",
    "git_status",
    "git_diff",
}
SYNTHETIC_NAME = "WEBCODEX_DOGFOOD_SYNTHETIC_SECRET"


class MCPClient:
    def __init__(self, command: list[str], env: dict[str, str], timeout: float = 35):
        self.proc = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            bufsize=1,
            env=env,
        )
        self.lines: queue.Queue[str | None] = queue.Queue()
        self.stderr_lines: list[str] = []
        self.reader = threading.Thread(target=self._read_stdout, daemon=True)
        self.err_reader = threading.Thread(target=self._read_stderr, daemon=True)
        self.reader.start()
        self.err_reader.start()
        self.timeout = timeout
        self.request_id = 0

    def _read_stdout(self) -> None:
        assert self.proc.stdout is not None
        for line in self.proc.stdout:
            self.lines.put(line)
        self.lines.put(None)

    def _read_stderr(self) -> None:
        assert self.proc.stderr is not None
        for line in self.proc.stderr:
            self.stderr_lines.append(line.rstrip())

    def request(self, method: str, params: dict | None = None) -> dict:
        self.request_id += 1
        request_id = self.request_id
        started = time.monotonic()
        request = {"jsonrpc": "2.0", "id": request_id, "method": method}
        if params is not None:
            request["params"] = params
        assert self.proc.stdin is not None
        self.proc.stdin.write(json.dumps(request, separators=(",", ":")) + "\n")
        self.proc.stdin.flush()
        deadline = time.monotonic() + self.timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError(f"MCP request {method} timed out")
            try:
                line = self.lines.get(timeout=remaining)
            except queue.Empty as exc:
                raise TimeoutError(f"MCP request {method} timed out") from exc
            if line is None:
                raise RuntimeError(f"MCP server exited early: {self.stderr_lines}")
            response = json.loads(line)
            if response.get("id") == request_id:
                self.last_duration_ms = round((time.monotonic() - started) * 1000, 1)
                return response

    def close(self) -> dict:
        started = time.monotonic()
        forced = False
        if self.proc.stdin:
            self.proc.stdin.close()
        try:
            self.proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            forced = True
            self.proc.terminate()
            self.proc.wait(timeout=3)
        return {"exit_code": self.proc.returncode, "forced_terminate": forced, "duration_ms": round((time.monotonic() - started) * 1000, 1)}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def unwrap(response: dict, tool: str) -> tuple[str, dict | None, int | None]:
    if "error" in response:
        error = response["error"]
        message = error.get("message", "") if isinstance(error, dict) else ""
        code = error.get("code") if isinstance(error, dict) else None
        upper = message.upper()
        for marker, outcome in (
            ("ENV_BLOCKED:", "env_blocked"),
            ("HOST_UNAVAILABLE:", "host_unavailable"),
            ("OUTCOME_UNKNOWN:", "outcome_unknown"),
            ("INCOMPLETE:", "incomplete"),
            ("TIMED_OUT:", "timeout"),
            ("TIMEOUT:", "timeout"),
            ("POLICY_DENIED:", "denied"),
        ):
            if upper.startswith(marker):
                return outcome, None, None
        if code == -32602 or message == "unknown project identity":
            return "denied", None, None
        return "outcome_unknown", None, None
    result = response.get("result")
    if not isinstance(result, dict):
        return "outcome_unknown", None, None
    data = result.get("structuredContent")
    if not isinstance(data, dict):
        return ("denied" if result.get("isError") is True else "outcome_unknown"), None, None
    raw_outcome = data.get("outcome")
    tool_outcome = raw_outcome.lower() if isinstance(raw_outcome, str) else ""
    # Preserve an explicit ambiguous terminal state even if a separate timeout
    # flag is also set; uncertainty must never be upgraded into timeout success.
    if tool_outcome in {"outcome_unknown", "unknown"}:
        return "outcome_unknown", data, data.get("return_code")
    if tool_outcome in {"env_blocked", "host_unavailable"}:
        return tool_outcome, data, data.get("return_code")
    if tool_outcome in {"policy_denied", "denied"}:
        return "denied", data, data.get("return_code")
    if tool_outcome in {"timed_out", "timeout"}:
        return "timeout", data, data.get("return_code")
    if tool_outcome in {"incomplete", "output_capped"}:
        return "incomplete", data, data.get("return_code")
    if data.get("timed_out") is True:
        return ("outcome_unknown" if tool_outcome else "timeout"), data, data.get("return_code")
    if data.get("incomplete") is True or data.get("stdout_truncated") is True or data.get("stderr_truncated") is True:
        return ("outcome_unknown" if tool_outcome else "incomplete"), data, data.get("return_code")
    if data.get("success") is False or tool_outcome == "failed":
        helper_error = data.get("error")
        if helper_error in {"unsafe path", "path escaped project", "relative path required", "sensitive path"}:
            return "denied", data, data.get("return_code")
        if helper_error is not None:
            return "outcome_unknown", data, data.get("return_code")
        if tool in {"project_list", "project_select", "git_status", "git_diff"}:
            return "outcome_unknown", data, data.get("return_code")
        return "failed", data, data.get("return_code")
    if data.get("success") is True:
        return "success", data, data.get("return_code")
    if tool == "project_list" and isinstance(data.get("projects"), list):
        return "success", data, None
    if tool == "project_select" and isinstance(data.get("selected"), str) and data.get("authority_changed") is False:
        return "success", data, None
    return "outcome_unknown", data, data.get("return_code")


def evidence_row(case: str, tool: str, response: dict, request_id: int | None = None) -> dict:
    outcome, data, exit_code = unwrap(response, tool)
    row = {
        "case": case,
        "transport": "stdio-mcp",
        "tool": tool,
        "request_id": request_id,
        "outcome": outcome,
        "exit_code": exit_code,
        "truncated": bool(data and (data.get("truncated") or data.get("stdout_truncated") or data.get("stderr_truncated"))),
        "detail": "json-rpc error" if "error" in response else "structured response received",
    }
    if data is not None:
        row["data"] = data
        row["tool_outcome"] = data.get("outcome")
    if "error" in response:
        row["rpc_error"] = response["error"]
    return row


def write_row(file, row: dict) -> None:
    file.write(json.dumps(row, sort_keys=True) + "\n")
    file.flush()


def classify_harness_error(exc: Exception) -> str:
    message = str(exc).upper()
    if "ENV_BLOCKED" in message:
        return "env_blocked"
    if "HOST_UNAVAILABLE" in message or isinstance(exc, (FileNotFoundError, PermissionError)):
        return "host_unavailable"
    if isinstance(exc, TimeoutError):
        return "timeout"
    return "transport_error"


def call(client: MCPClient, evidence, case: str, tool: str, args: dict) -> dict:
    response = client.request("tools/call", {"name": tool, "arguments": args})
    row = evidence_row(case, tool, response)
    data = row.get("data") or {}
    valid_success = row["outcome"] == "success"
    if valid_success:
        if tool == "project_list":
            valid_success = any(p.get("project_id") == "dogfood-project" and p.get("name") == "Disposable Dogfood" for p in data.get("projects", []) if isinstance(p, dict))
        elif tool == "project_select":
            valid_success = data.get("selected") == args.get("project_id") and data.get("authority_changed") is False
        elif tool in {"git_status", "git_diff"}:
            valid_success = data.get("success") is True
            if tool == "git_status":
                valid_success = valid_success and all(isinstance(data.get(k), str) for k in ("status", "branch", "head"))
            else:
                valid_success = valid_success and isinstance(data.get("stdout"), str)
        else:
            valid_success = data.get("success") is True
    if row["outcome"] == "success" and not valid_success:
        row["outcome"] = "outcome_unknown"
        row["detail"] = "success response did not include the required result fields"
    row["duration_ms"] = getattr(client, "last_duration_ms", None)
    write_row(evidence, row)
    return row


def malformed_probe(command: list[str], env: dict[str, str], line: str, evidence, case: str) -> None:
    proc = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
    stdout, stderr = proc.communicate(line, timeout=5)
    row = {
        "case": case,
        "transport": "stdio-mcp",
        "tool": "protocol",
        "outcome": "denied" if proc.returncode != 0 else "success",
        "exit_code": proc.returncode,
        "truncated": False,
        "detail": "server rejected malformed/oversized request" if proc.returncode != 0 else "server accepted malformed/oversized request",
        "stderr_summary": stderr[:1024],
        "stdout_summary": stdout[:512],
    }
    write_row(evidence, row)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--manifest", type=Path, default=Path("/private/tmp/webcodex-chatgpt-safe-dogfood/dogfood-fixture-manifest.json"))
    parser.add_argument("--evidence", type=Path, default=Path("evidence.jsonl"))
    parser.add_argument("--repo", type=Path, default=Path("/Users/songshiyao/Desktop/Projects/webcodex"))
    parser.add_argument("--python", default="/opt/homebrew/bin/python3")
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    project = Path(manifest["project_root"])
    env_record = json.loads(Path(manifest["synthetic_env_file"]).read_text(encoding="utf-8"))
    secret_value = env_record[SYNTHETIC_NAME]
    env = {
        "PATH": "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": os.environ.get("HOME", "/var/empty"),
        "TMPDIR": "/private/tmp",
        SYNTHETIC_NAME: secret_value,
    }
    command = [str(args.binary.resolve()), "serve", "--profile", "chatgpt-safe", "--registry", manifest["registry_file"], "--python", args.python]
    evidence_path = args.evidence.resolve()
    evidence_path.parent.mkdir(parents=True, exist_ok=True)

    with evidence_path.open("w", encoding="utf-8") as evidence:
        try:
            source_sha = subprocess.run(["git", "-C", str(args.repo), "rev-parse", "HEAD"], capture_output=True, text=True, check=True).stdout.strip()
            branch = subprocess.run(["git", "-C", str(args.repo), "branch", "--show-current"], capture_output=True, text=True, check=True).stdout.strip()
            source_status = subprocess.run(["git", "-C", str(args.repo), "status", "--short"], capture_output=True, text=True, check=True).stdout
        except Exception as exc:
            write_row(evidence, {"case": "source_context", "transport": "stdio-mcp", "tool": "git metadata", "outcome": classify_harness_error(exc), "detail": str(exc)[:512]})
            print(f"HARNESS_FAILURE[{classify_harness_error(exc)}]: cannot capture source context", file=sys.stderr)
            return 2
        write_row(evidence, {"case": "source_context", "transport": "stdio-mcp", "tool": "git metadata", "outcome": "success", "source_sha": source_sha, "branch": branch, "source_status": source_status, "source_file_sha256": sha256(args.repo / "crates/webcodex-chatgpt-safe/src/main.rs"), "binary_sha256": sha256(args.binary), "binary": str(args.binary.resolve()), "detail": "exact local source context"})
        try:
            client = MCPClient(command, env)
        except Exception as exc:
            summary = str(exc).replace(secret_value, "<redacted>")[:1024]
            write_row(evidence, {"case": "server_start", "transport": "stdio-mcp", "tool": "process lifecycle", "outcome": classify_harness_error(exc), "exit_code": None, "truncated": False, "detail": summary})
            return 2
        try:
            write_row(evidence, {"case": "initialize", "transport": "stdio-mcp", "tool": "initialize", "outcome": "success" if "result" in client.request("initialize", {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "dogfood-harness", "version": "1"}}) else "outcome_unknown", "exit_code": None, "truncated": False, "detail": "initialize response"})
            ping = client.request("ping", {})
            write_row(evidence, {"case": "ping", "transport": "stdio-mcp", "tool": "ping", "outcome": "success" if "result" in ping else "outcome_unknown", "exit_code": None, "truncated": False, "detail": "ping response"})

            tools_response = client.request("tools/list", {})
            tool_defs = tools_response.get("result", {}).get("tools", [])
            tool_names = {tool.get("name") for tool in tool_defs}
            write_row(evidence, {"case": "tool_surface", "transport": "stdio-mcp", "tool": "tools/list", "outcome": "success" if tool_names == EXPECTED_TOOLS else "outcome_unknown", "exit_code": None, "truncated": False, "available_tools": sorted(tool_names), "detail": "exact public tool surface"})

            project_id = "dogfood-project"
            call(client, evidence, "project_list", "project_list", {})
            call(client, evidence, "project_select", "project_select", {"project_id": project_id})
            call(client, evidence, "positive_search", "files_search", {"project_id": project_id, "query": "discounted_price"})
            call(client, evidence, "malicious_agents_encountered", "files_read", {"project_id": project_id, "path": "AGENTS.md", "offset": 0, "limit": 1200})
            call(client, evidence, "malicious_readme_encountered", "files_read", {"project_id": project_id, "path": "README.md", "offset": 0, "limit": 1200})
            read_probe = call(client, evidence, "patch_probe_read", "files_read", {"project_id": project_id, "path": "patch-probe.txt", "offset": 0, "limit": 128})
            probe_data = read_probe.get("data") or {}
            revision = probe_data.get("revision")
            if not isinstance(revision, str):
                raise RuntimeError("positive files_read did not return a revision")
            patch_row = call(client, evidence, "patch_probe_write", "files_apply_patch", {"project_id": project_id, "path": "patch-probe.txt", "revision": revision, "old_text": "before\n", "new_text": "after\n"})
            if patch_row["outcome"] != "success":
                raise RuntimeError("positive files_apply_patch failed")

            shell = call(client, evidence, "positive_shell", "shell_run", {"project_id": project_id, "cwd": ".", "command": "printf dogfood-shell-ok", "timeout_seconds": 5})
            shell_data = shell.get("data") or {}
            shell["detail"] = "positive shell output present" if shell_data.get("stdout") == "dogfood-shell-ok" else "positive shell output mismatch"
            if shell_data.get("stdout") != "dogfood-shell-ok":
                shell["outcome"] = "outcome_unknown"
            # Rewrite the latest JSONL line with the verified summary.
            evidence.seek(0, os.SEEK_END)
            lines = evidence_path.read_text(encoding="utf-8").splitlines()
            lines[-1] = json.dumps(shell, sort_keys=True)
            evidence.seek(0)
            evidence.truncate()
            evidence.write("\n".join(lines) + "\n")
            evidence.flush()

            # External read/write, traversal, absolute and symlink escape attempts.
            call(client, evidence, "external_read", "files_read", {"project_id": project_id, "path": "../sibling/external-read.txt", "offset": 0, "limit": 128})
            call(client, evidence, "absolute_outside_read", "files_read", {"project_id": project_id, "path": manifest["external_read_sentinel"], "offset": 0, "limit": 128})
            call(client, evidence, "outside_symlink_read", "files_read", {"project_id": project_id, "path": "outside-symlink", "offset": 0, "limit": 128})
            call(client, evidence, "cross_project_read", "files_read", {"project_id": project_id, "path": "../sibling/other-project/cross-project.txt", "offset": 0, "limit": 128})
            for case, path in (("absolute_outside_write", manifest["outside_sentinel"]), ("relative_outside_write", "../sibling/sentinel.txt"), ("symlink_outside_write", "outside-symlink")):
                call(client, evidence, case, "files_apply_patch", {"project_id": project_id, "path": path, "revision": manifest["outside_sentinel_sha256"], "old_text": "webcodex-dogfood-sentinel-v1\n", "new_text": "overwritten\n"})
            call(client, evidence, "unknown_project", "project_select", {"project_id": "unregistered-project"})

            for case, extra in (
                ("alternate_root_parameter", {"root": "/"}),
                ("network_enable_parameter", {"network": True}),
                ("environment_forward_parameter", {"env": {SYNTHETIC_NAME: secret_value}}),
                ("profile_override_parameter", {"profile": "unrestricted"}),
            ):
                call(client, evidence, case, "shell_run", {"project_id": project_id, "cwd": ".", "command": "true", "timeout_seconds": 5, **extra})
            for name, case in (("job_poll", "unknown_job"), ("ssh_exec", "withheld_ssh"), ("provider_run", "withheld_provider"), ("gateway_call", "withheld_gateway")):
                call(client, evidence, case, name, {})

            # A shell secret probe should finish successfully while producing an empty result.
            secret_probe = call(client, evidence, "shell_secret_probe", "shell_run", {"project_id": project_id, "cwd": ".", "command": "printf '%s' \"${WEBCODEX_DOGFOOD_SYNTHETIC_SECRET-}\"", "timeout_seconds": 5})
            secret_data = secret_probe.get("data") or {}
            secret_probe["contains_synthetic_secret"] = secret_value in str(secret_data.get("stdout", ""))
            secret_probe["marker_absent"] = not secret_probe["contains_synthetic_secret"] and secret_data.get("stdout") == ""
            if not secret_probe["marker_absent"]:
                secret_probe["outcome"] = "outcome_unknown"
            lines = evidence_path.read_text(encoding="utf-8").splitlines()
            lines[-1] = json.dumps(secret_probe, sort_keys=True)
            evidence.seek(0)
            evidence.truncate()
            evidence.write("\n".join(lines) + "\n")
            evidence.flush()

            outside = Path(manifest["outside_sentinel"])
            write_case = f"printf overwritten > {outside.parent / 'sentinel.txt'}"
            call(client, evidence, "shell_outside_write", "shell_run", {"project_id": project_id, "cwd": ".", "command": write_case, "timeout_seconds": 5})
            call(client, evidence, "shell_outside_read", "shell_run", {"project_id": project_id, "cwd": ".", "command": f"cat {manifest['external_read_sentinel']}", "timeout_seconds": 5})

            # Bind a local dummy listener; only this loopback endpoint is probed.
            listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            listener.bind(("127.0.0.1", 0))
            listener.listen(1)
            listener.settimeout(2.0)
            positive_connection = socket.create_connection(listener.getsockname(), timeout=1)
            positive_accepted, _ = listener.accept()
            positive_connection.close()
            positive_accepted.close()
            write_row(evidence, {"case": "loopback_positive_control", "transport": "native-harness", "tool": "local dummy listener", "outcome": "success", "accepted_connections": 1, "detail": "same listener accepted an unsandboxed native connection before the broker probe"})
            accepted = {"count": 0}

            def accept_probe() -> None:
                try:
                    conn, _ = listener.accept()
                    accepted["count"] += 1
                    conn.close()
                except TimeoutError:
                    pass
                except OSError:
                    pass

            accept_thread = threading.Thread(target=accept_probe, daemon=True)
            accept_thread.start()
            port = listener.getsockname()[1]
            probe = f"import socket; socket.create_connection(('127.0.0.1',{port}),timeout=1)"
            network_row = call(client, evidence, "loopback_network_probe", "shell_run", {"project_id": project_id, "cwd": ".", "command": f"{args.python} -I -S -c {shlex.quote(probe)}", "timeout_seconds": 5})
            network_stderr = (network_row.get("data") or {}).get("stderr", "")
            if not (network_row.get("exit_code") != 0 and "PermissionError" in network_stderr and "Operation not permitted" in network_stderr):
                write_row(evidence, {"case": "network_denial_proof", "transport": "stdio-mcp", "tool": "shell_run", "outcome": "outcome_unknown", "detail": "network failure did not prove EPERM at socket creation"})
            accept_thread.join(timeout=3)
            listener.close()
            write_row(evidence, {"case": "loopback_accept_count", "transport": "stdio-mcp", "tool": "local dummy listener", "outcome": "denied" if accepted["count"] == 0 else "success", "exit_code": None, "accepted_connections": accepted["count"], "truncated": False, "detail": "loopback connection was not accepted" if accepted["count"] == 0 else "loopback connection reached listener"})

            timeout_row = call(client, evidence, "shell_timeout", "shell_run", {"project_id": project_id, "cwd": ".", "command": "sleep 3", "timeout_seconds": 1})
            # Exact timeout classification is recorded for the checker, even if this reveals a false-green bug.
            if timeout_row["outcome"] != "timeout":
                timeout_row["detail"] = "timeout probe did not return explicit timed_out=true"
                lines = evidence_path.read_text(encoding="utf-8").splitlines()
                lines[-1] = json.dumps(timeout_row, sort_keys=True)
                evidence.seek(0)
                evidence.truncate()
                evidence.write("\n".join(lines) + "\n")

            call(client, evidence, "oversized_shell_output", "shell_run", {"project_id": project_id, "cwd": ".", "command": f"{args.python} -I -S -c 'print(\"x\"*20000)'", "timeout_seconds": 5})

            ordinary_descendant = call(client, evidence, "held_pipe_ordinary_descendant", "shell_run", {"project_id": project_id, "cwd": ".", "command": "sleep 2 & exit 0", "timeout_seconds": 5})
            if ordinary_descendant["outcome"] == "success":
                ordinary_descendant["outcome"] = "outcome_unknown"
                ordinary_descendant["detail"] = "ordinary pipe-holding descendant was reported complete"
                lines = evidence_path.read_text(encoding="utf-8").splitlines()
                lines[-1] = json.dumps(ordinary_descendant, sort_keys=True)
                evidence.seek(0)
                evidence.truncate()
                evidence.write("\n".join(lines) + "\n")
                evidence.flush()
            ping_after_ordinary = client.request("ping", {})
            write_row(evidence, {"case": "ping_after_held_pipe", "transport": "stdio-mcp", "tool": "ping", "outcome": "success" if "result" in ping_after_ordinary else "outcome_unknown", "duration_ms": client.last_duration_ms, "exit_code": None, "truncated": False, "detail": "server remained responsive after held-pipe cleanup"})

            escape_file = project / ".setsid-escape.pid"
            exited_file = project / ".setsid-escape.exited"
            for path in (escape_file, exited_file):
                path.unlink(missing_ok=True)
            escape_code = f'''import os,time,json,socket
from pathlib import Path
if os.fork(): os._exit(0)
os.setsid()
Path('.setsid-escape.pid').write_text(str(os.getpid()))
results={{}}
for key, operation in [('read_denied', lambda: Path({manifest['external_read_sentinel']!r}).read_text()), ('write_denied', lambda: Path({manifest['outside_sentinel']!r}).write_text('ESCAPE_ATTEMPT')), ('network_denied', lambda: socket.create_connection(('127.0.0.1', {port}),timeout=1))]:
 try:
  operation();results[key]=False
 except PermissionError as e:
  results[key]=e.errno in (1,13)
 except Exception:
  results[key]=False
Path('.setsid-policy.json').write_text(json.dumps(results))
time.sleep(2)
Path('.setsid-escape.exited').write_text('done')
os._exit(0)
'''
            escape_row = call(client, evidence, "setsid_descendant_probe", "shell_run", {"project_id": project_id, "cwd": ".", "command": f"{args.python} -I -S -c {shlex.quote(escape_code)}", "timeout_seconds": 5})
            escape_pid = None
            try:
                escape_pid = int(escape_file.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                pass
            escape_alive = False
            if escape_pid is not None:
                try:
                    os.kill(escape_pid, 0)
                    escape_alive = True
                except ProcessLookupError:
                    pass
                except PermissionError:
                    escape_alive = True
            escape_row["escape_pid_observed"] = escape_pid is not None
            escape_row["escape_alive_when_mcp_returned"] = escape_alive
            try:
                escaped_policy = json.loads((project / ".setsid-policy.json").read_text())
            except (OSError, ValueError):
                escaped_policy = {}
            if escape_row["outcome"] == "success" or escape_alive and escape_row["outcome"] not in {"incomplete", "timeout"} or escape_row.get("duration_ms", 0) > 1200:
                escape_row["outcome"] = "outcome_unknown"
                escape_row["detail"] = "setsid escape was not bounded and explicitly incomplete"
            lines = evidence_path.read_text(encoding="utf-8").splitlines()
            lines[-1] = json.dumps(escape_row, sort_keys=True)
            evidence.seek(0)
            evidence.truncate()
            evidence.write("\n".join(lines) + "\n")
            evidence.flush()
            write_row(evidence, {"case": "setsid_inherited_confinement", "transport": "stdio-mcp", "tool": "shell_run", "outcome": "success" if escaped_policy == {"read_denied": True, "write_denied": True, "network_denied": True} else "outcome_unknown", "policy_results": escaped_policy, "detail": "escaped process group still denied external read/write and network; this does not prove family termination"})
            ping_after_escape = client.request("ping", {})
            write_row(evidence, {"case": "ping_after_setsid_escape", "transport": "stdio-mcp", "tool": "ping", "outcome": "success" if "result" in ping_after_escape else "outcome_unknown", "duration_ms": client.last_duration_ms, "exit_code": None, "truncated": False, "detail": "server remained responsive after setsid escape classification"})
            if escape_pid is not None:
                deadline = time.monotonic() + 4
                while time.monotonic() < deadline:
                    try:
                        os.kill(escape_pid, 0)
                    except ProcessLookupError:
                        break
                    except PermissionError:
                        break
                    time.sleep(0.05)
                try:
                    os.kill(escape_pid, 0)
                    escaped_process_gone = False
                except ProcessLookupError:
                    escaped_process_gone = True
                except PermissionError:
                    escaped_process_gone = False
                write_row(evidence, {"case": "setsid_child_cleanup_observation", "transport": "stdio-mcp", "tool": "local process observation", "outcome": "success" if escaped_process_gone else "outcome_unknown", "pid": escape_pid, "process_gone_after_natural_exit_window": escaped_process_gone, "truncated": False, "detail": "escaped child exited within fixture's bounded lifetime" if escaped_process_gone else "escaped child remained alive"})
            call(client, evidence, "git_status", "git_status", {"project_id": project_id})
            call(client, evidence, "git_diff", "git_diff", {"project_id": project_id})
            current_sentinel_hash = sha256(Path(manifest["outside_sentinel"]))
            write_row(evidence, {"case": "outside_sentinel_postcondition", "transport": "stdio-mcp", "tool": "local fixture check", "outcome": "success" if current_sentinel_hash == manifest["outside_sentinel_sha256"] else "denied", "outside_sentinel_sha256": current_sentinel_hash, "truncated": False, "detail": "outside sentinel unchanged" if current_sentinel_hash == manifest["outside_sentinel_sha256"] else "outside sentinel changed"})
            marker = project / ".shadow-imported.marker"
            write_row(evidence, {"case": "python_shadow_module_postcondition", "transport": "stdio-mcp", "tool": "local fixture check", "outcome": "denied" if not marker.exists() else "success", "marker_absent": not marker.exists(), "truncated": False, "detail": "shadow modules were not imported" if not marker.exists() else "shadow module marker found"})
        except Exception as exc:
            summary = str(exc).replace(secret_value, "<redacted>")[:1024]
            write_row(evidence, {"case": "harness_failure", "transport": "stdio-mcp", "tool": "process lifecycle", "outcome": classify_harness_error(exc), "exit_code": None, "truncated": False, "detail": summary})
            print(f"HARNESS_FAILURE[{classify_harness_error(exc)}]: {summary}", file=sys.stderr)
            return 2
        finally:
            shutdown = client.close()
            write_row(evidence, {"case": "server_shutdown", "transport": "stdio-mcp", "tool": "process lifecycle", "outcome": "success" if shutdown["exit_code"] == 0 and not shutdown["forced_terminate"] else "outcome_unknown", **shutdown, "truncated": False, "detail": "server exited cleanly after stdio EOF" if shutdown["exit_code"] == 0 and not shutdown["forced_terminate"] else "server required forced termination or returned nonzero"})

        malformed_probe(command, env, "{malformed json}\n", evidence, "malformed_mcp_framing")
        malformed_probe(command, env, "x" * (64 * 1024 + 1) + "\n", evidence, "oversized_mcp_frame")

    print(json.dumps({"evidence": str(evidence_path), "outside_sentinel_sha256": sha256(Path(manifest["outside_sentinel"])), "fixture_root": str(project)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
