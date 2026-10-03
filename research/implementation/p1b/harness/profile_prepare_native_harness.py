#!/usr/bin/env python3
"""Exercise registered shell-profile preparation through a real Runner binary."""

import argparse
import hashlib
import http.server
import json
import os
import pathlib
import secrets
import signal
import subprocess
import tempfile
import threading
import time


CLIENT_ID = "p1b-shell-profile-evidence"
TOKEN = "synthetic-p1b-shell-profile-token"
TOTAL_TIMEOUT = 60.0


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def sh_quote(value):
    return "'" + value.replace("'", "'\\''") + "'"


def shell_request(request_id, cwd, command):
    return {
        "request_id": request_id,
        "client_id": CLIENT_ID,
        "kind": "run_shell",
        "cwd": str(cwd),
        "command": command,
        "timeout_secs": 15,
        "requested_by": "p1b-native-evidence",
        "created_at": int(time.time()),
    }


def wait_event(event, deadline, description):
    remaining = deadline - time.monotonic()
    if remaining <= 0 or not event.wait(remaining):
        raise TimeoutError(f"overall {TOTAL_TIMEOUT:.0f}s deadline expired waiting for {description}")


def kill_owned_pid(pid):
    if not pid:
        return
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    end = time.monotonic() + 0.5
    while time.monotonic() < end:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return
        time.sleep(0.02)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass


class PollingFixture:
    def __init__(self, project, registry_file, outside, secret):
        self.project = project
        self.registry_file = registry_file
        self.outside = outside
        self.secret = secret
        self.lock = threading.Lock()
        self.results = []
        self.delivered = 0
        self.requests = []
        self.request_paths = []
        self.first_result = threading.Event()
        self.second_result = threading.Event()
        self.failure = []
        fixture = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def _respond(self, body):
                encoded = json.dumps(body).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)

            def do_POST(self):
                length = int(self.headers.get("Content-Length", "0"))
                raw = self.rfile.read(length)
                body = json.loads(raw or b"{}")
                with fixture.lock:
                    fixture.request_paths.append(self.path)
                if self.headers.get("Authorization") != f"Bearer {TOKEN}":
                    self._respond({"success": False, "error": "unexpected synthetic auth"})
                    return
                if self.path == "/api/shell/agent/register":
                    self._respond({
                        "success": True,
                        "client": {
                            "client_id": CLIENT_ID,
                            "status": "online",
                            "connected": True,
                            "last_seen": int(time.time()),
                            "capabilities": {"shell": True},
                            "pending_requests": 0,
                            "agent_protocol_generation": 2,
                            "project_inventory": {
                                "sync_state": "pending",
                                "total_synced": 0,
                                "max_summaries_per_page": 100,
                                "max_serialized_bytes_per_page": 262144,
                            },
                        },
                    })
                    return
                if self.path == "/api/shell/agent/poll":
                    page = body.get("project_inventory_page")
                    with fixture.lock:
                        result_count = len(fixture.results)
                        request = None
                        if result_count == 0 and fixture.delivered == 0:
                            request = shell_request(
                                "profile-prepare-first",
                                project,
                                "profile-tool && { if IFS= read -r _; then printf 'stdin-open\\n'; else printf 'stdin-eof\\n'; fi; } && printf 'run-shell-ok\\n' && printf 'run-shell-stderr\\n' >&2",
                            )
                            fixture.delivered = 1
                        elif result_count == 1 and fixture.delivered == 1:
                            request = shell_request(
                                "profile-prepare-after-unregister",
                                project,
                                "printf 'must-not-run\\n' > second-started.txt",
                            )
                            fixture.delivered = 2
                    response = {"success": True, "request": request}
                    if isinstance(page, dict):
                        response["project_inventory"] = {
                            "sync_state": "complete" if page.get("complete") else "in_progress",
                            "generation": page.get("generation"),
                            "total_reported": page.get("total_reported"),
                            "total_synced": len(page.get("projects", [])),
                            "max_summaries_per_page": 100,
                            "max_serialized_bytes_per_page": 262144,
                        }
                    self._respond(response)
                    return
                if self.path == "/api/shell/agent/result":
                    request_id = body.get("request_id")
                    with fixture.lock:
                        fixture.results.append(body)
                        result_count = len(fixture.results)
                    if request_id == "profile-prepare-first":
                        try:
                            fixture.registry_file.unlink()
                        except FileNotFoundError:
                            pass
                        fixture.first_result.set()
                    elif request_id == "profile-prepare-after-unregister":
                        fixture.second_result.set()
                    else:
                        fixture.failure.append(f"unexpected result id: {request_id!r}")
                    self._respond({"success": True})
                    return
                if self.path == "/api/shell/agent/offline":
                    self._respond({"success": True})
                    return
                self.send_error(404)

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)

    @property
    def url(self):
        return f"http://127.0.0.1:{self.server.server_port}"

    def start(self):
        self.thread.start()

    def close(self, deadline):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(max(0.0, min(2.0, deadline - time.monotonic())))
        if self.thread.is_alive():
            raise RuntimeError("loopback HTTP fixture failed to stop within 2s")


def run_binary(binary):
    if not binary.is_file():
        raise FileNotFoundError(f"Runner binary not found: {binary}")
    deadline = time.monotonic() + TOTAL_TIMEOUT
    with tempfile.TemporaryDirectory(prefix="wcb-p1b-shell-profile-") as temporary:
        root = pathlib.Path(temporary)
        project = root / "project"
        project.mkdir()
        bin_dir = project / "bin"
        bin_dir.mkdir()
        profile_tool = bin_dir / "profile-tool"
        profile_tool.write_text("#!/bin/sh\nprintf 'profile-path-ok\\n'\n")
        profile_tool.chmod(0o700)
        outside = root / "outside"
        outside.mkdir()
        registry = root / "registry"
        registry.mkdir()
        registry_file = registry / "project.toml"
        registry_file.write_text(
            f'id = "{CLIENT_ID}"\npath = {json.dumps(str(project.resolve()))}\n'
        )

        secret = "synthetic-host-value-" + secrets.token_hex(16)
        inside_marker = project / "init-marker.txt"
        secret_marker = project / "host-env-marker.txt"
        outside_marker = outside / "outside-write.txt"
        count_marker = project / "prepare-count.txt"
        prepare_stdin_marker = project / "prepare-stdin-status.txt"
        descendant_pid_file = project / "prepare-descendant.pid"
        init_script = "\n".join([
            f"printf 'inside\\n' > {sh_quote(str(inside_marker))}",
            f"printf 'x' >> {sh_quote(str(count_marker))}",
            f"if IFS= read -r -t 1 _; then printf 'data\\n' > {sh_quote(str(prepare_stdin_marker))}; else status=$?; if [ $status -eq 1 ]; then printf 'eof\\n' > {sh_quote(str(prepare_stdin_marker))}; else printf 'still-open\\n' > {sh_quote(str(prepare_stdin_marker))}; fi; fi",
            f"printf '%s' \"${{P1B_PROFILE_HOST_MARKER-unavailable}}\" > {sh_quote(str(secret_marker))}",
            f"printf 'outside\\n' > {sh_quote(str(outside_marker))} 2>/dev/null || true",
            f"export PATH={sh_quote(str(bin_dir))}:\"$PATH\"",
            "sleep 60 &",
            f"printf '%s\\n' \"$!\" > {sh_quote(str(descendant_pid_file))}",
            "printf 'profile-prepare-stdout\\n'",
            "printf 'profile-prepare-stderr\\n' >&2",
        ])
        fixture = PollingFixture(project, registry_file, outside, secret)
        config = root / "runner.toml"
        config.write_text(
            f'server_url = {json.dumps(fixture.url)}\n'
            f'token = {json.dumps(TOKEN)}\n'
            f'client_id = {json.dumps(CLIENT_ID)}\n'
            'transport = "polling"\n'
            'poll_interval_ms = 10\n'
            f'project_registry_dir = {json.dumps(str(registry))}\n'
            "[policy]\n"
            f'allowed_roots = [{json.dumps(str(project))}]\n'
            "[shell]\n"
            'environment_mode = "inherit"\n'
            'default_profile = "registered"\n'
            "[shell.profiles.registered]\n"
            'program = "/bin/sh"\n'
            'args = ["-c"]\n'
            'dialect = "posix"\n'
            f'init_script = {json.dumps(init_script)}\n'
        )
        process = None
        runner_output = b""
        child_pids = set()
        fixture.start()
        try:
            environment = os.environ.copy()
            environment["P1B_PROFILE_HOST_MARKER"] = secret
            process = subprocess.Popen(
                [str(binary), "--config", str(config), "--stop-on-stdin-eof"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                start_new_session=True,
                env=environment,
            )
            try:
                wait_event(fixture.second_result, deadline, "second RunShell result")
            except TimeoutError as error:
                if process.stdin is not None:
                    process.stdin.close()
                try:
                    runner_output, _ = process.communicate(timeout=1.0)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        runner_output, _ = process.communicate(timeout=1.0)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        runner_output, _ = process.communicate(timeout=1.0)
                paths = list(fixture.request_paths)
                with fixture.lock:
                    result_diagnostics = [
                        {
                            "request_id": item.get("request_id"),
                            "command_execution_state": item.get("command_execution_state"),
                            "exit_code": item.get("exit_code"),
                            "error": item.get("error"),
                        }
                        for item in fixture.results
                    ]
                redacted = runner_output.decode(errors="replace").replace(TOKEN, "<synthetic-token>").replace(secret, "<synthetic-host-value>")
                raise RuntimeError(
                    f"{error}; runner_rc={process.returncode}; http_paths={paths!r}; results={result_diagnostics!r}; fixture_failures={fixture.failure!r}; runner_output={redacted!r}"
                ) from error
            if process.poll() is not None:
                raise AssertionError("Runner exited before operation-return descendant check")
            try:
                operation_descendant_pid = int(descendant_pid_file.read_text().strip())
            except (FileNotFoundError, ValueError) as error:
                raise AssertionError("prepare did not record its owned descendant PID") from error
            child_pids.add(operation_descendant_pid)
            descendant_gone_at_operation_return = not pid_exists(operation_descendant_pid)
            if process.stdin is not None:
                process.stdin.close()
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("overall deadline expired before Runner exit")
            try:
                runner_output, _ = process.communicate(timeout=remaining)
            except subprocess.TimeoutExpired as error:
                raise TimeoutError("Runner did not stop after parent stdin EOF") from error

            with fixture.lock:
                results = list(fixture.results)
            if fixture.failure:
                raise AssertionError("; ".join(fixture.failure))
            if len(results) != 2:
                raise AssertionError(f"expected two real RunShell results, observed {len(results)}")
            first, second = results
            count = len(count_marker.read_text()) if count_marker.exists() else 0
            marker_value = secret_marker.read_text() if secret_marker.exists() else None
            prepare_stdin_value = prepare_stdin_marker.read_text() if prepare_stdin_marker.exists() else None
            inside_written = inside_marker.is_file()
            outside_written = outside_marker.is_file()
            second_started = (project / "second-started.txt").exists()
            first_state = first.get("command_execution_state")
            first_success = first.get("exit_code") == 0 and first_state == "completed"
            first_stdout = first.get("stdout") or ""
            first_stderr = first.get("stderr") or ""
            path_output_preserved = first_stdout == "profile-path-ok\nstdin-eof\nrun-shell-ok\n"
            quantitative_output_preserved = first_stderr == "run-shell-stderr\n"
            second_refused = (
                second.get("command_execution_state") == "not_started"
                and second.get("exit_code") is None
                and "no trusted project context" in (second.get("error") or "")
                and not second_started
            )
            raw_red = outside_written and marker_value == secret
            brokered_profile = (
                inside_written
                and not outside_written
                and marker_value == "unavailable"
                and count == 1
                and prepare_stdin_value == "eof\n"
                and descendant_gone_at_operation_return
            )
            if raw_red:
                verdict = "RED"
            elif process.returncode != 0:
                verdict = "FAIL"
            elif not brokered_profile or not second_refused:
                verdict = "FAIL"
            elif first_success and path_output_preserved and quantitative_output_preserved:
                verdict = "PASS"
            else:
                verdict = "FAIL"

            return {
                "binary": str(binary),
                "sha256": digest(binary),
                "verdict": verdict,
                "raw_launch_effects": "OBSERVED" if raw_red else "ABSENT",
                "registered_prepare": {
                    "workspace_marker_written": inside_written,
                    "outside_write_denied": not outside_written,
                    "synthetic_host_value_absent": marker_value == "unavailable",
                    "prepare_count_after_unregister_request": count,
                    "prepare_stdin_eof": prepare_stdin_value == "eof\n",
                    "profile_path_used_by_real_run_shell": path_output_preserved,
                    "run_shell_stdin_eof": "stdin-eof\n" in first_stdout and "stdin-open\n" not in first_stdout,
                    "run_shell_stdout_bytes": len(first_stdout.encode()),
                    "run_shell_stderr_bytes": len(first_stderr.encode()),
                    "run_shell_stderr_preserved": quantitative_output_preserved,
                    "first_result_state": first_state,
                    "first_result_exit_code": first.get("exit_code"),
                    "second_result_state": second.get("command_execution_state"),
                    "second_result_error": second.get("error"),
                    "second_payload_not_started": second_refused,
                    "descendant_gone_at_operation_return": descendant_gone_at_operation_return,
                },
                "runner_return_code": process.returncode,
                "runner_output_redacted": runner_output.decode(errors="replace").replace(TOKEN, "<synthetic-token>").replace(secret, "<synthetic-host-value>"),
                "http_result_ids": [item.get("request_id") for item in results],
            }
        finally:
            if process is not None and process.poll() is None:
                try:
                    if process.stdin is not None and not process.stdin.closed:
                        process.stdin.close()
                except BrokenPipeError:
                    pass
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=1.0)
                except (ProcessLookupError, subprocess.TimeoutExpired):
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    try:
                        process.wait(timeout=1.0)
                    except subprocess.TimeoutExpired:
                        pass
                try:
                    runner_output, _ = process.communicate(timeout=0.5)
                except subprocess.TimeoutExpired:
                    pass
            for pid in child_pids:
                kill_owned_pid(pid)
            fixture.close(deadline)


def pid_exists(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", type=pathlib.Path, required=True)
    parser.add_argument("--candidate", type=pathlib.Path, required=True)
    args = parser.parse_args()

    baseline = run_binary(args.baseline.resolve())
    candidate = run_binary(args.candidate.resolve())
    print(json.dumps({"baseline": baseline, "candidate": candidate}, indent=2))
    if baseline["verdict"] != "RED" or baseline["raw_launch_effects"] != "OBSERVED":
        raise SystemExit(1)
    if candidate["verdict"] != "PASS" or candidate["raw_launch_effects"] != "ABSENT":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
