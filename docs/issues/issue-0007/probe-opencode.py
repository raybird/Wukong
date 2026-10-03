import json, os, pathlib, shlex, subprocess, tempfile, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
root = pathlib.Path(tempfile.mkdtemp(prefix='issue7-opencode-'))
text_only = False
entry_mode = False

class Model(BaseHTTPRequestHandler):

    def log_message(self, *a):
        pass

    def do_POST(self):
        req = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        (root / 'request.json').write_text(json.dumps(req))
        tool_seen = any((m.get('role') == 'tool' for m in req.get('messages', [])))
        choice = {'index': 0, 'delta': {'content': 'PROBE_OK'}, 'finish_reason': 'stop'} if tool_seen else {'index': 0, 'delta': {'tool_calls': [{'index': 0, 'id': 'call_probe', 'type': 'function', 'function': {'name': 'bash', 'arguments': json.dumps({'command': 'printf CLI_PERMISSION_PROBE > permission-marker', 'description': 'permission probe', 'workdir': str(root)})}}]}, 'finish_reason': 'tool_calls'}
        if text_only:
            users = [json.dumps(m) for m in req.get('messages', []) if m.get('role') == 'user']
            choice = {'index': 0, 'delta': {'content': 'HISTORY_PRESENT' if any(('ISSUE7_HISTORY_TOKEN' in m for m in users)) else 'NO_HISTORY'}, 'finish_reason': 'stop'}
        if entry_mode:
            messages = req.get('messages', [])
            user_index = max((i for i, m in enumerate(messages) if m.get('role') == 'user'))
            user = json.dumps(messages[user_index])
            if '<role>|<skill-or-none>' in user:
                choice = {'index': 0, 'delta': {'content': 'fixer|none'}, 'finish_reason': 'stop'}
            elif any((m.get('role') == 'tool' for m in messages[user_index + 1:])):
                time.sleep(0.5)
                choice = {'index': 0, 'delta': {'content': '' if 'EMPTY_OUTPUT_PROBE' in user else 'CLI_INTEGRATION_OK'}, 'finish_reason': 'stop'}
            else:
                choice = {'index': 0, 'delta': {'tool_calls': [{'index': 0, 'id': 'call_read', 'type': 'function', 'function': {'name': 'read', 'arguments': json.dumps({'filePath': str(root / 'probe.txt')})}}]}, 'finish_reason': 'tool_calls'}
        chunk = {'id': 'probe', 'object': 'chat.completion.chunk', 'created': 1, 'model': 'probe', 'choices': [choice]}
        payload = ('data: ' + json.dumps(chunk) + '\n\ndata: [DONE]\n\n').encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
server = ThreadingHTTPServer(('127.0.0.1', 0), Model)
threading.Thread(target=server.serve_forever, daemon=True).start()
env = os.environ.copy()
for kind in ['DATA', 'CONFIG', 'CACHE', 'STATE']:
    env['XDG_' + kind + '_HOME'] = str(root / kind.lower())
config = {'plugin': [], 'model': 'probe/probe', 'provider': {'probe': {'npm': '@ai-sdk/openai-compatible', 'name': 'Probe', 'options': {'baseURL': f'http://127.0.0.1:{server.server_port}/v1', 'apiKey': 'probe'}, 'models': {'probe': {'name': 'Probe', 'limit': {'context': 8192, 'output': 1024}}}}}, 'permission': {'bash': 'deny'}, 'snapshot': False}
env['OPENCODE_CONFIG_CONTENT'] = json.dumps(config)
try:
    for rule, flag in [('deny', True), ('ask', True), ('ask', False)]:
        config['permission']['bash'] = rule
        env['OPENCODE_CONFIG_CONTENT'] = json.dumps(config)
        marker = root / 'permission-marker'
        marker.unlink(missing_ok=True)
        cmd = ['opencode', 'run', '--dir', str(root)] + (['--dangerously-skip-permissions'] if flag else []) + ['--format', 'json', '--print-logs', '--log-level', 'ERROR', 'run the bash permission probe']
        try:
            result = subprocess.run(cmd, cwd=root, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15)
            assert result.returncode == 0, result.stderr
            events = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
            tools = [{'tool': e['part']['tool'], 'state': e['part'].get('state')} for e in events if e['type'] == 'tool_use']
            assert (marker.read_text() if marker.exists() else None) == ('CLI_PERMISSION_PROBE' if rule == 'ask' and flag else None)
            print(json.dumps({'root': str(root), 'permission': rule, 'skip': flag, 'exit': result.returncode, 'marker': marker.read_text() if marker.exists() else None, 'tools': tools, 'session': next((e.get('sessionID') for e in events), None), 'stderr': result.stderr}), flush=True)
        except subprocess.TimeoutExpired:
            raise AssertionError('permission probe timed out')
    text_only = True
    previous = None
    for prompt, resume, expected in [('Remember ISSUE7_HISTORY_TOKEN.', False, 'HISTORY_PRESENT'), ('What was the previous token?', True, 'HISTORY_PRESENT'), ('What was the previous token?', False, 'NO_HISTORY')]:
        cmd = ['opencode', 'run', '--dir', str(root), '--format', 'json'] + (['-s', previous] if resume else []) + [prompt]
        result = subprocess.run(cmd, cwd=root, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=15)
        events = [json.loads(line) for line in result.stdout.splitlines() if line.startswith('{')]
        session = next((e['sessionID'] for e in events))
        text = ''.join((e['part']['text'] for e in events if e['type'] == 'text'))
        assert result.returncode == 0 and text == expected, (result.returncode, text, result.stderr)
        assert not resume or session == previous
        print(json.dumps({'session': session, 'resume': resume, 'text': text, 'exit': result.returncode}), flush=True)
        previous = session
    entry_mode = True
    config['permission'] = {'read': 'allow', 'external_directory': {str(root) + '/*': 'allow'}}
    config['compaction'] = {'auto': False}
    (root / 'probe.txt').write_text('CLI entrypoint probe')
    wrapper = root / 'opencode-wrapper.sh'
    wrapper.write_text('state=$1\nshift\nfor arg in "$@"; do printf "%s\\n" "$arg" >> "$state/argv"; done\nsession=""\nstream=false\nprev=""\nfor arg in "$@"; do if [ "$prev" = "-s" ]; then session=$arg; fi; if [ "$arg" = "--format" ]; then stream=true; fi; prev=$arg; done\nif [ "$stream" = true ]; then printf "resumed=%s\\n" "$session" >> "$state/sessions"; fi\n' + ''.join(('export XDG_' + kind + '_HOME=' + shlex.quote(str(root / kind.lower())) + '\n' for kind in ['DATA', 'CONFIG', 'CACHE', 'STATE'])) + 'export OPENCODE_CONFIG_CONTENT=' + shlex.quote(json.dumps(config)) + '\n' + 'opencode run --dir "$state" "$@"\ncode=$?\nif [ "$stream" = true ]; then touch "$state/finished"; fi\nexit "$code"\n')
    integration_env = os.environ.copy()
    integration_env['WUKONG_ISSUE7_OPENCODE_PROBE'] = str(wrapper)
    result = subprocess.run(['cargo', 'test', '-p', 'wukong-cli', '-p', 'wukong-web', '-p', 'wukong-telegram', '-p', 'wukong-scheduler', '--test', 'cli_backend'], cwd=pathlib.Path(__file__).resolve().parents[3], env=integration_env, capture_output=True, text=True, timeout=120)
    print(result.stdout, flush=True)
    print(result.stderr, flush=True)
    assert result.returncode == 0, 'real OpenCode entrypoint tests failed'
finally:
    server.shutdown()
    server.server_close()
