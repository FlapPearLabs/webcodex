#!/usr/bin/env python3
"""Review probes against the actual harness main and verdict AST, no native claim."""

import ast
import importlib.util
import json
import pathlib
import subprocess
import sys
import types

HARNESS = pathlib.Path(__file__).resolve().with_name("profile_prepare_native_harness.py")


def load_harness():
    spec = importlib.util.spec_from_file_location("reviewed_shell_harness", HARNESS)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def child_main(state):
    module = load_harness()
    outcomes = iter([
        {"verdict": "RED", "raw_launch_effects": "OBSERVED"},
        {"verdict": state, "raw_launch_effects": "ABSENT"},
    ])
    module.run_binary = lambda _binary: next(outcomes)
    sys.argv = [str(HARNESS), "--baseline", "/tmp/injected-old", "--candidate", "/tmp/injected-new"]
    module.main()


def verdict_case(**values):
    tree = ast.parse(HARNESS.read_text())
    run = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "run_binary")
    branch = next(node for node in ast.walk(run) if isinstance(node, ast.If) and isinstance(node.test, ast.Name) and node.test.id == "raw_red")
    namespace = dict(raw_red=False, brokered_profile=True, second_refused=True,
                     process=types.SimpleNamespace(returncode=0), first_success=True,
                     path_output_preserved=True, quantitative_output_preserved=True)
    namespace.update(values)
    exec(compile(ast.Module(body=[branch], type_ignores=[]), str(HARNESS), "exec"), namespace)
    return namespace["verdict"]


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--main-state":
        child_main(sys.argv[2])
        return
    result = {"harness": str(HARNESS), "actual_main": [], "actual_verdict_branch": []}
    for state in ["PASS", "ENV_BLOCKED", "HOST_UNAVAILABLE", "FAIL", "RED"]:
        run = subprocess.run([sys.executable, __file__, "--main-state", state], capture_output=True, text=True, timeout=5)
        result["actual_main"].append({"candidate": state, "exit_code": run.returncode,
                                       "expected_exit_code": 0 if state == "PASS" else 1,
                                       "json_retained": '"candidate"' in run.stdout})
    for name, values, expected in [
        ("valid_observations", {}, "PASS"),
        ("runner_nonzero_without_host_diagnostic", {"process": types.SimpleNamespace(returncode=42)}, "FAIL"),
        ("first_operation_failed_without_host_diagnostic", {"first_success": False}, "FAIL"),
        ("stdout_fidelity_mismatch", {"path_output_preserved": False}, "FAIL"),
        ("stderr_fidelity_mismatch", {"quantitative_output_preserved": False}, "FAIL"),
    ]:
        result["actual_verdict_branch"].append({"scenario": name, "actual": verdict_case(**values), "expected": expected})
    print(json.dumps(result, indent=2))
    if any(row["exit_code"] != row["expected_exit_code"] or not row["json_retained"] for row in result["actual_main"]):
        raise SystemExit(1)
    if any(row["actual"] != row["expected"] for row in result["actual_verdict_branch"]):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
