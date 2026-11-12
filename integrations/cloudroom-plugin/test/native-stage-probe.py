"""Opt-in real Claude CLI fixture; isolated local scripted Messages API only."""
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib, json, os, shlex, signal, subprocess, sys, threading

staged, plugin, instructions_file, output = map(Path, sys.argv[1:])
output.mkdir(parents=True, exist_ok=True)
workspace = output / 'workspace'
workspace.mkdir(exist_ok=True)
home = output / 'home'
home.mkdir(exist_ok=True)
instructions = instructions_file.read_text()
marker = 'OPENBOT_REQUIRED_NATIVE_INSTRUCTION_TAIL'
assert len(instructions) > 4096 and marker in instructions
assert staged.is_dir() and (staged / 'scripts/run.sh').stat().st_mode & 0o777 == 0o755
plugin_name = json.loads((plugin / '.claude-plugin/plugin.json').read_text())['name']
asset_hash = hashlib.sha256((staged / 'assets/data.bin').read_bytes()).hexdigest()
observations = []

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_POST(self):
        length = int(self.headers.get('content-length', '0'))
        if length > 4 * 1024 * 1024:
            self.send_error(413)
            return
        body = json.loads(self.rfile.read(length))
        if 'count_tokens' in self.path:
            data = json.dumps({'input_tokens': 100}).encode()
            self.send_response(200)
            self.send_header('content-type', 'application/json')
            self.send_header('content-length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        system = json.dumps(body.get('system', ''))
        messages = json.dumps(body.get('messages', []))
        skill_received = 'OPENBOT_REQUIRED_NATIVE_SKILL_BODY' in messages
        script_received = 'preserved supporting bytes' in messages and asset_hash in messages
        observations.append({'path': self.path, 'instruction_tail_received': marker in system,
            'skill_body_received': skill_received, 'script_result_received': script_received,
            'tools': [item.get('name') for item in body.get('tools', [])]})
        if script_received or len(observations) > 6:
            block = {'type': 'text', 'text': ''}
            delta = {'type': 'text_delta', 'text': 'OPENBOT_NATIVE_STAGED_COMPLETE'}
            reason = 'end_turn'
        else:
            name = 'Bash' if skill_received else 'Skill'
            args = {'command': shlex.quote(str(staged / 'scripts/run.sh')), 'description': 'Execute isolated staged skill script', 'timeout': 10000} if skill_received else {'skill': plugin_name + ':required-proof'}
            block = {'type': 'tool_use', 'id': 'toolu_staged_' + str(len(observations)), 'name': name, 'input': {}}
            delta = {'type': 'input_json_delta', 'partial_json': json.dumps(args)}
            reason = 'tool_use'
        message = {'id': 'msg_staged_' + str(len(observations)), 'type': 'message', 'role': 'assistant', 'content': [],
            'model': body.get('model', 'claude-sonnet-4-6'), 'stop_reason': None, 'stop_sequence': None,
            'usage': {'input_tokens': 100, 'output_tokens': 0}}
        events = [('message_start', {'type': 'message_start', 'message': message}),
            ('content_block_start', {'type': 'content_block_start', 'index': 0, 'content_block': block}),
            ('content_block_delta', {'type': 'content_block_delta', 'index': 0, 'delta': delta}),
            ('content_block_stop', {'type': 'content_block_stop', 'index': 0}),
            ('message_delta', {'type': 'message_delta', 'delta': {'stop_reason': reason, 'stop_sequence': None}, 'usage': {'output_tokens': 40}}),
            ('message_stop', {'type': 'message_stop'})]
        data = ''.join('event: ' + name + '\ndata: ' + json.dumps(payload) + '\n\n' for name, payload in events).encode()
        self.send_response(200)
        self.send_header('content-type', 'text/event-stream')
        self.send_header('content-length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
env = {'HOME': str(home), 'PATH': '/opt/homebrew/bin:/usr/bin:/bin', 'CLAUDE_CONFIG_DIR': str(home / '.claude'),
    'ANTHROPIC_API_KEY': 'sk-ant-openbot-disposable-fixture-only', 'ANTHROPIC_BASE_URL': 'http://127.0.0.1:' + str(server.server_port),
    'DISABLE_TELEMETRY': '1', 'DISABLE_ERROR_REPORTING': '1', 'CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC': '1', 'NO_COLOR': '1'}
args = ['/opt/homebrew/bin/claude', '--print', '--verbose', '--output-format', 'stream-json', '--permission-mode', 'bypassPermissions',
    '--setting-sources', '', '--strict-mcp-config', '--mcp-config', '{"mcpServers":{}}', '--no-session-persistence', '--tools', 'Bash,Skill',
    '--plugin-dir', str(plugin), '--system-prompt', instructions, 'Use the configured skill and execute its staged script.']
try:
    process = subprocess.Popen(args, cwd=workspace, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, start_new_session=True)
    try:
        stdout, stderr = process.communicate(timeout=45)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            stdout, stderr = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate()
    (output / 'native-stdout.jsonl').write_text(stdout)
    (output / 'native-stderr.txt').write_text(stderr)
    passed = process.returncode == 0 and any(item['instruction_tail_received'] for item in observations) and any(item['skill_body_received'] for item in observations) and any(item['script_result_received'] for item in observations) and asset_hash in stdout
    result = {'status': 'PASS' if passed else 'FAIL', 'exit': process.returncode, 'observations': observations, 'asset_sha256': asset_hash,
        'script_mode': oct((staged / 'scripts/run.sh').stat().st_mode & 0o777),
        'limitations': 'Real CLI and production Cloudroom staging/plugin assembly; scripted local model replies do not prove model reasoning or live paid provider behavior.'}
    (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
    sys.exit(0 if passed else 1)
finally:
    server.shutdown()
    server.server_close()
    thread.join(timeout=2)
