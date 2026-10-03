#!/usr/bin/env python3
"""Exercise the permanent catalog harness's actual top-level verdict wrapper."""

import contextlib
import importlib.util
import io
import json
import pathlib
import sys


HARNESS = pathlib.Path(__file__).resolve().with_name("runner_catalog_native_harness.py")


def invoke(state, diagnostic=None):
    spec = importlib.util.spec_from_file_location("catalog_harness_under_test", HARNESS)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    if state == "PASS":
        module.run = lambda _runner: {"results": [{"case": "injected-pass"}]}
    else:
        error_type = {
            "HOST_UNAVAILABLE": FileNotFoundError,
            "ENV_BLOCKED": RuntimeError,
            "FAIL": RuntimeError,
            "ASSERTION": AssertionError,
            "TIMEOUT": TimeoutError,
            "UNKNOWN_PERMISSION": PermissionError,
        }[state]

        def fail(_runner):
            raise error_type(diagnostic)

        module.run = fail
    sys.argv = [str(HARNESS), "--runner", "/tmp/injected-runner"]
    output = io.StringIO()
    try:
        with contextlib.redirect_stdout(output):
            module.main()
        exit_code = 0
    except SystemExit as error:
        exit_code = int(error.code)
    result = json.loads(output.getvalue())
    return {"input": state, "status": result["status"], "exit_code": exit_code, "diagnostic_retained": "diagnostic" in result}


def main():
    cases = [
        invoke("PASS"),
        invoke("HOST_UNAVAILABLE", "runner binary not found: /tmp/missing"),
        invoke("ENV_BLOCKED", "sandbox-exec denied request; sandbox_apply: Operation not permitted"),
        invoke("FAIL", "ordinary runtime failure"),
        invoke("ASSERTION", "ordinary assertion failure"),
        invoke("TIMEOUT", "runner timeout"),
        invoke("UNKNOWN_PERMISSION", "Operation not permitted"),
    ]
    print(json.dumps({"probe": "actual catalog main classifier; not native proof", "cases": cases}, indent=2))
    expected = [
        ("PASS", "PASS", 0),
        ("HOST_UNAVAILABLE", "HOST_UNAVAILABLE", 1),
        ("ENV_BLOCKED", "ENV_BLOCKED", 1),
        ("FAIL", "FAIL", 1),
        ("ASSERTION", "FAIL", 1),
        ("TIMEOUT", "FAIL", 1),
        ("UNKNOWN_PERMISSION", "FAIL", 1),
    ]
    actual = [(row["input"], row["status"], row["exit_code"]) for row in cases]
    if actual != expected or any(row["status"] != "PASS" and not row["diagnostic_retained"] for row in cases):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
