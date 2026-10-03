import http.server
import argparse
import json
import os
import pathlib
import signal
import subprocess
import tempfile
import threading
import time

GIT = pathlib.Path('/usr/bin/git')
if not GIT.exists():
    GIT = pathlib.Path(subprocess.check_output(['which', 'git'], text=True).strip())

class Handler(http.server.BaseHTTPRequestHandler):
    requests = []
    def log_message(self, *_args):
        pass
    def do_POST(self):
        length = int(self.headers.get('Content-Length', '0'))
        raw = self.rfile.read(length)
        body = json.loads(raw or b'{}')
        if self.path == '/api/shell/agent/register':
            self.requests.append({'path': self.path, 'client_id': body.get('client_id')})
            client_id = body.get('client_id', 'catalog-evidence')
            response = {
                'success': True,
                'client': {
                    'client_id': client_id,
                    'status': 'online',
                    'connected': True,
                    'last_seen': int(time.time()),
                    'capabilities': {'shell': True},
                    'pending_requests': 0,
                    'agent_protocol_generation': 2,
                    'project_inventory': {
                        'sync_state': 'pending', 'total_synced': 0,
                        'max_summaries_per_page': 100,
                        'max_serialized_bytes_per_page': 262144,
                    },
                },
            }
        elif self.path == '/api/shell/agent/poll':
            page = body.get('project_inventory_page')
            self.requests.append({'path': self.path, 'project_inventory_page': page})
            if not isinstance(page, dict):
                response = {'success': False, 'error': 'missing_project_inventory_page'}
            else:
                response = {
                    'success': True,
                    'request': None,
                    'project_inventory': {
                        'sync_state': 'complete' if page.get('complete') else 'in_progress',
                        'generation': page.get('generation'),
                        'total_reported': page.get('total_reported'),
                        'total_synced': len(page.get('projects', [])),
                        'max_summaries_per_page': 100,
                        'max_serialized_bytes_per_page': 262144,
                    },
                }
        elif self.path == '/api/shell/agent/offline':
            self.requests.append({'path': self.path, 'client_id': body.get('client_id')})
            response = {'success': True}
        else:
            self.send_error(404)
            return
        encoded = json.dumps(response).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


def run_git(args, cwd):
    return subprocess.check_output([str(GIT), *args], cwd=cwd, text=True).strip()


def run_once(binary, config):
    process = subprocess.Popen(
        [str(binary), '--config', str(config), '--once'],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    try:
        output, _ = process.communicate(timeout=40)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            output, _ = process.communicate(timeout=2)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            output, _ = process.communicate(timeout=2)
        raise RuntimeError('runner timed out; process group terminated; output=' + output.decode(errors='replace'))
    return process.returncode, output.decode(errors='replace')


def run(runner):
    if not runner.is_file():
        raise FileNotFoundError(f'runner binary not found: {runner}')
    results = []
    with tempfile.TemporaryDirectory(prefix='wcb-p1b-catalog-') as td:
        root = pathlib.Path(td)
        project = root / 'project'
        project.mkdir()
        registry = root / 'registry'
        registry.mkdir()
        state = root / 'state'
        state.mkdir()
        tracked = project / 'tracked.txt'
        tracked.write_text('clean fixture\n')
        run_git(['init', '-q', '-b', 'p1b-evidence'], project)
        run_git(['-c', 'user.name=WebCodex Test', '-c', 'user.email=test@example.invalid', 'add', 'tracked.txt'], project)
        run_git(['-c', 'user.name=WebCodex Test', '-c', 'user.email=test@example.invalid', 'commit', '-qm', 'catalog fixture'], project)
        (registry / 'project.toml').write_text(f'id = "catalog-evidence"\npath = {json.dumps(str(project))}\nname = "catalog evidence"\n')

        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        Handler.requests = []
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        config = root / 'runner.toml'
        config.write_text(
            f'server_url = "http://127.0.0.1:{server.server_port}"\n'
            'token = "synthetic-p1b-evidence-token"\n'
            'client_id = "catalog-evidence"\n'
            'transport = "polling"\n'
            'poll_interval_ms = 10\n'
            f'project_registry_dir = {json.dumps(str(registry))}\n'
            '[policy]\n'
            f'allowed_roots = [{json.dumps(str(project))}]\n'
        )
        expected_branch = run_git(['rev-parse', '--abbrev-ref', 'HEAD'], project)
        expected_head = run_git(['log', '-1', '--pretty=format:%h'], project)
        expected_clean = run_git(['status', '--short'], project)
        for label, dirty in [('clean', False), ('dirty', True)]:
            if dirty:
                tracked.write_text('dirty fixture\n')
            expected_status = run_git(['status', '--short'], project)
            if (bool(expected_status) != dirty):
                raise AssertionError(f'fixture dirty state mismatch: {expected_status!r}')
            start = len(Handler.requests)
            code, output = run_once(runner, config)
            request_slice = Handler.requests[start:]
            polls = [item for item in request_slice if item['path'] == '/api/shell/agent/poll']
            if code != 0:
                raise RuntimeError(f'runner {label} invocation exited {code}: {output}')
            if len(polls) != 1:
                raise AssertionError(f'expected one real polling inventory request, observed {len(polls)}: {request_slice!r}; runner={output}')
            page = polls[0]['project_inventory_page']
            projects = page.get('projects', [])
            if len(projects) != 1:
                raise AssertionError(f'expected exactly one project summary: {page!r}')
            summary = projects[0]
            observed = {
                'branch': summary.get('git_branch'),
                'head': summary.get('git_head'),
                'dirty': summary.get('git_dirty'),
                'path': summary.get('path'),
                'inventory_complete': page.get('complete'),
                'page_total': page.get('total_reported'),
                'request_count': len(request_slice),
            }
            if observed['branch'] != expected_branch:
                raise AssertionError(f'{label}: branch mismatch expected={expected_branch!r} observed={observed!r}')
            if observed['head'] != expected_head:
                raise AssertionError(f'{label}: head mismatch expected={expected_head!r} observed={observed!r}')
            if observed['dirty'] is not dirty:
                raise AssertionError(f'{label}: dirty mismatch expected={dirty!r} observed={observed!r}')
            if observed['path'] != str(project.resolve()):
                raise AssertionError(f'{label}: canonical project path mismatch {observed!r}')
            if observed['inventory_complete'] is not True or observed['page_total'] != 1:
                raise AssertionError(f'{label}: incomplete inventory page {observed!r}')
            if 'did not complete inside its budget' in output or 'broker refused' in output:
                raise AssertionError(f'{label}: incomplete/refused Git capture was logged despite success: {output}')
            results.append({'case': label, 'expected_status': expected_status, 'observed': observed, 'runner_output': output})

        # Exercise a real incomplete `git status` capture without replacing Git
        # or editing Runner code. Git's repository-local fsmonitor hook stalls
        # only the project metadata read; the broker capability probe uses its
        # own throwaway repository and remains unaffected.
        hook = project / 'fsmonitor-hang.sh'
        marker = project / '.git' / 'fsmonitor-hook-ran'
        hook.write_text(f'#!/bin/sh\nprintf ran > {json.dumps(str(marker))}\nsleep 10\nprintf "token\\n"\n')
        hook.chmod(0o755)
        run_git(['config', 'core.fsmonitor', str(hook)], project)
        start = len(Handler.requests)
        started_at = time.monotonic()
        code, output = run_once(runner, config)
        elapsed = time.monotonic() - started_at
        request_slice = Handler.requests[start:]
        polls = [item for item in request_slice if item['path'] == '/api/shell/agent/poll']
        if code != 0:
            raise RuntimeError(f'runner timeout invocation exited {code}: {output}')
        if len(polls) != 1:
            raise AssertionError(f'expected one timeout inventory request, observed {len(polls)}: {request_slice!r}; runner={output}')
        timeout_projects = polls[0]['project_inventory_page'].get('projects', [])
        if len(timeout_projects) != 1:
            raise AssertionError(f'expected one project summary in timeout inventory: {polls[0]!r}')
        timeout_summary = timeout_projects[0]
        if not marker.is_file():
            raise AssertionError('timeout hook marker is missing; the intended Git hook was not observed')
        if timeout_summary.get('git_branch') != expected_branch or timeout_summary.get('git_head') != expected_head:
            raise AssertionError(f'timeout must preserve completed branch/head reads: {timeout_summary!r}')
        if timeout_summary.get('git_dirty') is not None:
            raise AssertionError(f'incomplete status capture must be absent, never false/true: {timeout_summary!r}')
        if 'reporting the capture as incomplete' not in output:
            raise AssertionError(f'incomplete status capture was not reported: {output!r}')
        if elapsed >= 8:
            raise AssertionError(f'brokered catalog operation exceeded the 8s evidence bound: {elapsed:.3f}s')
        results.append({
            'case': 'status-timeout-incomplete',
            'elapsed_seconds': round(elapsed, 3),
            'hook_marker': str(marker),
            'observed': {
                'branch': timeout_summary.get('git_branch'),
                'head': timeout_summary.get('git_head'),
                'dirty': timeout_summary.get('git_dirty'),
                'inventory_complete': polls[0]['project_inventory_page'].get('complete'),
            },
            'runner_output': output,
        })
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
        if thread.is_alive():
            raise RuntimeError('mock server did not stop within 2s')
        out = {
            'runner_binary': str(runner),
            'runner_argv': [str(runner), '--config', '<temporary>/runner.toml', '--once'],
            'config_redacted': {
                'server_url': 'http://127.0.0.1:<ephemeral-port>',
                'token': '<synthetic token; redacted>',
                'client_id': 'catalog-evidence',
                'transport': 'polling',
                'poll_interval_ms': 10,
                'project_registry_dir': '<temporary>/registry',
                'policy.allowed_roots': ['<temporary>/project'],
            },
            'expected_branch': expected_branch,
            'expected_head': expected_head,
            'initial_status': expected_clean,
            'results': results,
            'http_events_redacted': [
                {'path': event['path'], 'project_count': len(event.get('project_inventory_page', {}).get('projects', []))}
                for event in Handler.requests
            ],
        }
        return out

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--runner', required=True, type=pathlib.Path)
    args = parser.parse_args()
    try:
        result = run(args.runner)
        result['status'] = 'PASS'
        print(json.dumps(result, indent=2))
    except Exception as error:
        diagnostic = f'{type(error).__name__}: {error}'
        if isinstance(error, FileNotFoundError) and any(
            value in diagnostic.lower() for value in ('runner', 'git', 'which')
        ):
            status = 'HOST_UNAVAILABLE'
        elif 'sandbox-exec' in diagnostic and 'sandbox_apply: Operation not permitted' in diagnostic:
            status = 'ENV_BLOCKED'
        else:
            status = 'FAIL'
        print(json.dumps({'status': status, 'diagnostic': diagnostic}, indent=2))
        raise SystemExit(0 if status == 'PASS' else 1)

if __name__ == '__main__':
    main()
