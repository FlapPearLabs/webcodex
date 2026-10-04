"""Diagnostic-only helper mutation; not a production broker proof."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile


SOURCE = Path('/Users/songshiyao/Desktop/Projects/webcodex/crates/webcodex-chatgpt-safe/src/main.rs')
EXPECTED = '8160f4e2a13755a1a3a9ef9c9787a5f466ce9ba398510a4cb9d0a85e4bacf557'
OUTPUT = Path('/Users/songshiyao/Documents/Codex/2026-10-02/role-webcodex-security-architect-reviewer-mode-5/outputs/dogfood-evidence/directory-search-diagnostic-negative-control.jsonl')

raw = SOURCE.read_bytes()
assert hashlib.sha256(raw).hexdigest() == EXPECTED
assert os.getuid() != 0, 'permission test requires non-root operator'
source = raw.decode()
marker = 'const HELPER: &str = r#"'
assert source.count(marker) == 1
helper = source.split(marker, 1)[1].split('"#;', 1)[0]
mutation = ',onerror=on_walk_error'
assert helper.count(mutation) == 1
mutant = helper.replace(mutation, '', 1)
request = {'op': 'search', 'args': {'query': 'DIRECTORY_SEARCH_SYNTHETIC_NEEDLE'}}
rows = [{'context': {
    'kind': 'DIAGNOSTIC_ONLY_NOT_BROKER_PROOF',
    'source_sha256': EXPECTED,
    'operator_uid': os.getuid(),
    'mutation': 'remove only os.walk onerror keyword argument',
    'helper_sha256': hashlib.sha256(helper.encode()).hexdigest(),
    'mutant_helper_sha256': hashlib.sha256(mutant.encode()).hexdigest(),
}}]

with tempfile.TemporaryDirectory(prefix='webcodex-review-walk-diagnostic-', dir='/private/tmp') as tmp:
    root = Path(tmp) / 'project'
    directory = root / 'restricted-directory'
    directory.mkdir(parents=True)
    (directory / 'needle.txt').write_text('DIRECTORY_SEARCH_SYNTHETIC_NEEDLE\n')

    def run(label, program):
        result = subprocess.run(
            ['/opt/homebrew/bin/python3', '-I', '-S', '-c', program, json.dumps(request)],
            cwd=root,
            env={'PATH': '/usr/bin:/bin', 'HOME': str(root), 'TMPDIR': str(root)},
            capture_output=True,
            text=True,
            timeout=5,
        )
        rows.append({'case': label, 'request': request, 'returncode': result.returncode,
                     'stdout': result.stdout, 'stderr': result.stderr})
        assert result.returncode == 0 and not result.stderr
        return json.loads(result.stdout)

    positive = run('unchanged_helper_readable_directory_positive', helper)
    assert positive['success'] is True and positive['truncated'] is False
    assert positive['files_scanned'] == 1 and len(positive['matches']) == 1
    directory.chmod(0)
    try:
        actual = run('unchanged_helper_unreadable_directory_partial', helper)
        assert actual['success'] is True and actual['truncated'] is True
        assert actual['files_scanned'] == 0 and actual['matches'] == []
        old_behavior = run('mutant_without_onerror_false_complete', mutant)
        assert old_behavior['success'] is True and old_behavior['truncated'] is False
        assert old_behavior['files_scanned'] == 0 and old_behavior['matches'] == []
    finally:
        directory.chmod(0o700)

assert SOURCE.read_bytes() == raw
OUTPUT.write_text(''.join(json.dumps(row, ensure_ascii=False) + '\n' for row in rows))
print(json.dumps({'diagnostic': 'PASS', 'negative_control': 'mutant reproduces false completeness',
                  'production_source_unchanged': True, 'artifact': str(OUTPUT)}))
