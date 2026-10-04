#!/usr/bin/env python3
"""Fail-closed checker for normalized, schema-adapted MCP dogfood evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path


FAILURE_OUTCOMES = {
    "env_blocked",
    "host_unavailable",
    "outcome_unknown",
    "transport_error",
    "stale_revision",
}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_rows(path: Path) -> list[dict]:
    rows = []
    for line_no, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not raw.strip():
            continue
        try:
            row = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise ValueError(f"evidence line {line_no} is malformed JSON: {exc}") from exc
        if not isinstance(row, dict) or not isinstance(row.get("case"), str):
            raise ValueError(f"evidence line {line_no} lacks an object case field")
        rows.append(row)
    return rows


def parse_pairs(values: list[str], option: str) -> dict[str, str]:
    pairs: dict[str, str] = {}
    for value in values:
        if "=" not in value:
            raise ValueError(f"{option} requires CASE=OUTCOME")
        case, outcome = value.split("=", 1)
        if not case or not outcome or case in pairs:
            raise ValueError(f"invalid or duplicate {option}: {value}")
        pairs[case] = outcome
    return pairs


def self_test() -> int:
    allowed_tools = [
        "project_list", "project_select", "files_search", "files_read",
        "files_apply_patch", "shell_run", "git_status", "git_diff",
    ]
    with tempfile.TemporaryDirectory(prefix="webcodex-evidence-selftest-") as raw:
        root = Path(raw)
        project = root / "project"
        project.mkdir()
        sentinel = root / "sentinel.txt"
        sentinel.write_text("unchanged\n", encoding="utf-8")
        env_file = root / "synthetic.env.json"
        env_file.write_text(json.dumps({"WEBCODEX_SELFTEST_SECRET": "SELFTEST-ONLY"}), encoding="utf-8")
        manifest = root / "manifest.json"
        manifest.write_text(json.dumps({
            "outside_sentinel": str(sentinel),
            "outside_sentinel_sha256": digest(sentinel),
            "synthetic_env_file": str(env_file),
            "project_root": str(project),
        }), encoding="utf-8")
        cases = [
            {"case": "env_blocked_falsegreen", "outcome": "denied", "tool_outcome": "ENV_BLOCKED", "data": {"success": False}},
            {"case": "unknown_falsegreen", "outcome": "denied", "tool_outcome": "OUTCOME_UNKNOWN", "data": {"success": False}},
            {"case": "success_falsegreen", "outcome": "success", "data": {"success": False}},
        ]
        for index, row in enumerate(cases):
            evidence = root / f"evidence-{index}.jsonl"
            rows = [
                {"case": "tool_surface", "outcome": "success", "available_tools": allowed_tools},
                {"case": "ping_after_held_pipe", "outcome": "success"},
                {"case": "ping_after_setsid_escape", "outcome": "success"},
                {"case": "setsid_descendant_probe", "outcome": "success", "escape_pid_observed": True, "escape_alive_when_mcp_returned": False},
                {"case": "setsid_child_cleanup_observation", "outcome": "success", "process_gone_after_natural_exit_window": True},
                row,
            ]
            evidence.write_text("".join(json.dumps(item) + "\n" for item in rows), encoding="utf-8")
            result = subprocess.run(
                [sys.executable, __file__, "--manifest", str(manifest), "--evidence", str(evidence)],
                capture_output=True,
                text=True,
                check=False,
            )
            if result.returncode == 0:
                print(f"FAIL: checker accepted synthetic false-green case {row['case']}", file=sys.stderr)
                return 1
    print("PASS: checker rejected ENV_BLOCKED, OUTCOME_UNKNOWN, and success=false false-green controls")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--allow", action="append", default=[], metavar="CASE=OUTCOME")
    parser.add_argument("--expect", action="append", default=[], metavar="CASE=OUTCOME")
    parser.add_argument("--deny", action="append", default=[], metavar="CASE")
    parser.add_argument("--loopback-case", metavar="CASE")
    parser.add_argument("--secret-case", metavar="CASE")
    parser.add_argument("--test-before-case", metavar="CASE")
    parser.add_argument("--test-after-case", metavar="CASE")
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    if args.manifest is None or args.evidence is None:
        parser.error("--manifest and --evidence are required unless --self-test is used")

    try:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
        rows = load_rows(args.evidence)
        allow = parse_pairs(args.allow, "--allow")
        expected = parse_pairs(args.expect, "--expect")
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print(f"HARNESS_ERROR: {exc}", file=sys.stderr)
        return 2

    by_case: dict[str, list[dict]] = {}
    for row in rows:
        by_case.setdefault(row["case"], []).append(row)
    failures: list[str] = []

    for case, expected_outcome in allow.items():
        matches = by_case.get(case, [])
        if not matches or not any(row.get("outcome") == expected_outcome for row in matches):
            failures.append(f"allowed case {case!r} missing expected outcome {expected_outcome!r}")
    for case, expected_outcome in expected.items():
        matches = by_case.get(case, [])
        if not matches or not any(row.get("outcome") == expected_outcome for row in matches):
            failures.append(f"case {case!r} missing explicit expected outcome {expected_outcome!r}")
    for case in args.deny:
        matches = by_case.get(case, [])
        if not matches:
            failures.append(f"denial case {case!r} has no evidence")
            continue
        if not any(
            row.get("outcome") == "denied"
            or (isinstance(row.get("exit_code"), int) and row["exit_code"] != 0)
            for row in matches
        ):
            failures.append(f"denial case {case!r} has no explicit denial or nonzero command exit")

    for case, matches in by_case.items():
        for row in matches:
            outcome = row.get("outcome")
            tool_status = row.get("tool_outcome")
            status_to_outcome = {
                "ENV_BLOCKED": "env_blocked",
                "HOST_UNAVAILABLE": "host_unavailable",
                "OUTCOME_UNKNOWN": "outcome_unknown",
                "INCOMPLETE": "incomplete",
                "TIMED_OUT": "timeout",
                "TIMEOUT": "timeout",
                "POLICY_DENIED": "denied",
                "STALE_REVISION": "stale_revision",
            }
            normalized_status = status_to_outcome.get(str(tool_status).upper())
            if normalized_status in FAILURE_OUTCOMES or normalized_status == "stale_revision":
                failures.append(f"case {case!r} carries terminal tool status {tool_status!r}")
            if normalized_status is not None and outcome != normalized_status:
                failures.append(f"case {case!r} outcome {outcome!r} conflicts with tool status {tool_status!r}")
            if outcome in FAILURE_OUTCOMES or outcome == "stale_revision":
                failures.append(f"case {case!r} classified {outcome!r}; this is not a pass")
            data = row.get("data")
            if outcome == "success" and isinstance(data, dict) and data.get("success") is False:
                failures.append(f"case {case!r} reports success while structured data says success=false")
            if row.get("outcome") in {"timeout", "incomplete"} and expected.get(case) != row.get("outcome"):
                failures.append(f"case {case!r} classified {row['outcome']!r} without an explicit expected classification")
    surface = by_case.get("tool_surface", [])
    allowed_tools = {
        "project_list", "project_select", "files_search", "files_read",
        "files_apply_patch", "shell_run", "git_status", "git_diff",
    }
    if not surface or not any(set(row.get("available_tools", [])) == allowed_tools for row in surface):
        failures.append("tool surface does not exactly match the chatgpt-safe allowlist")
    if args.loopback_case:
        matches = by_case.get(args.loopback_case, [])
        if not matches or not any(row.get("accepted_connections") == 0 for row in matches):
            failures.append("loopback probe lacks evidence of zero accepted connections")
    if args.secret_case:
        matches = by_case.get(args.secret_case, [])
        if not matches or not any(row.get("marker_absent") is True for row in matches):
            failures.append("synthetic secret probe lacks evidence that the marker was absent")
    for case in ("ping_after_held_pipe", "ping_after_setsid_escape"):
        matches = by_case.get(case, [])
        if not matches or not any(row.get("outcome") == "success" for row in matches):
            failures.append(f"server was not proven responsive after {case}")
    escape_rows = by_case.get("setsid_descendant_probe", [])
    if not escape_rows or not any(row.get("escape_pid_observed") is True for row in escape_rows):
        failures.append("setsid escape probe did not record the descendant PID")
    elif any(row.get("escape_alive_when_mcp_returned") is True and row.get("outcome") == "success" for row in escape_rows):
        failures.append("setsid descendant remained alive while the MCP operation claimed success")
    cleanup_rows = by_case.get("setsid_child_cleanup_observation", [])
    if not cleanup_rows or not any(row.get("process_gone_after_natural_exit_window") is True for row in cleanup_rows):
        failures.append("setsid fixture descendant was not observed to exit after the probe")
    for case, expected_code in ((args.test_before_case, 1), (args.test_after_case, 0)):
        if case:
            matches = by_case.get(case, [])
            if not matches or not any(row.get("exit_code") == expected_code for row in matches):
                failures.append(f"test case {case!r} lacks expected exit code {expected_code}")

    sentinel = Path(manifest["outside_sentinel"])
    expected_hash = manifest["outside_sentinel_sha256"]
    try:
        actual_hash = digest(sentinel)
    except OSError as exc:
        failures.append(f"outside sentinel unavailable: {exc}")
    else:
        if actual_hash != expected_hash:
            failures.append("outside sentinel changed")

    env_data = json.loads(Path(manifest["synthetic_env_file"]).read_text(encoding="utf-8"))
    synthetic_value = next(iter(env_data.values()))
    evidence_text = args.evidence.read_text(encoding="utf-8")
    if synthetic_value in evidence_text:
        failures.append("synthetic secret value appears in recorded evidence")

    project = Path(manifest["project_root"])
    marker = project / ".shadow-imported.marker"
    if marker.exists():
        failures.append("fixture json.py/hashlib.py was imported")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}", file=sys.stderr)
        return 1
    print(f"PASS: checked {len(rows)} normalized MCP calls and postconditions")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
