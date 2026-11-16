from pathlib import Path
import json
import os
import signal
import subprocess
import time

logs = Path(__file__).resolve().parent
w = Path(__file__).resolve().parents[6]
r = json.loads((logs / 'resources.json').read_text())
env = os.environ.copy()
env.update(PATH=r['node'] + ':/opt/homebrew/opt/coreutils/libexec/gnubin:/Users/mac02/.cargo/bin:/opt/homebrew/opt/postgresql@18/bin:/opt/homebrew/bin:' + env['PATH'],
           DATABASE_URL=r['database'], TEST_DATABASE_URL=r['database'], SQLX_OFFLINE='true', CARGO_TARGET_DIR=r['target'], CARGO_BUILD_JOBS='2',
           PLAYWRIGHT_BROWSERS_PATH=r['playwright'], SKILL_BUNDLE_BROWSER_FIXTURES=r['parent'] + '/runtime/skill-browser')
for key in ['ALLOW_MISSING_DATABASE_URL', 'RUST_MIN_STACK', 'RUST_TEST_THREADS', 'OPENAI_API_KEY', 'ANTHROPIC_API_KEY',
            'OPENROUTER_API_KEY', 'NODE_OPTIONS', 'NODE_TLS_REJECT_UNAUTHORIZED']:
    env.pop(key, None)
commands = [('format', ['cargo', 'fmt', '--all'], {}),
            ('offline-alltargets', ['cargo', 'check', '--locked', '--offline', '--all-targets'], {})]
results = []


def run(name, command, extra=None):
    output = logs / ('B1-focused-R5-' + name + '.txt')
    started = time.monotonic()
    print(json.dumps({'starting': name, 'command': command}), flush=True)
    with output.open('w') as stream:
        process = subprocess.Popen(command, cwd=w, env={**env, **(extra or {})}, stdout=stream, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=600)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try: process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            code = 124
    item = {'name': name, 'command': command, 'exit': code, 'elapsed_s': round(time.monotonic()-started, 3), 'log': output.name}
    results.append(item)
    (logs / 'B1-focused-R5-results.json').write_text(json.dumps(results, indent=2) + '\n')
    print(json.dumps(item), flush=True)
    return code


for name, command, extra in commands:
    if run(name, command, extra): raise SystemExit(1)
with (logs / 'B1-focused-R5.snapshot.txt').open('w') as output:
    subprocess.run(['/Users/mac02/.codex/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh',
                    'plan/cloudroom-harness/COORDINATION.md', 'plan/cloudroom-harness/phases/p01/docs'],
                   cwd=w, env=env, check=True, stdout=output)
for name, args in [('all-skill-callers', ['--lib', 'skill']),
                   ('browser-fixtures', ['--lib', 'dump_skill_bundle_browser_fixtures', '--', '--ignored'])]:
    run(name, ['cargo', 'test', '--locked', '--offline', *args])
run('browser', ['node', '--test', 'scripts/tests/skill-bundle-browser.mjs'])
run('clippy', ['cargo', 'clippy', '--locked', '--offline', '--all-targets', '--', '-D', 'warnings'])
run('sqlx-prepare-check', ['cargo', 'sqlx', 'prepare', '--check', '--', '--all-targets'], {'SQLX_OFFLINE': 'false'})
print(json.dumps({'completed': len(results), 'failures': [x['name'] for x in results if x['exit']]}), flush=True)
raise SystemExit(1 if any(x['exit'] for x in results) else 0)
