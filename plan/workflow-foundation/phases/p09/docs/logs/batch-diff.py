"""The p09-only diff of a batch, and a copy of its tree for the next one.
Usage (repository root): python3 <this> <batch> [<previous-batch>]
- delta: files differing from the previous batch's tree hashes (or the p09 start hashes);
- base of each file: logs/tree-<previous>/<path>, else logs/start-base/<path> (pre-p09 copy of a
  file p08 had already changed, hash-verified), else HEAD's content, else empty (new file);
- writes logs/<batch>.diff and copies every p09-changed file into logs/tree-<batch>/."""
import hashlib, os, shutil, subprocess, sys
LOGS = 'plan/workflow-foundation/phases/p09/docs/logs/'
batch = sys.argv[1]
previous = sys.argv[2] if len(sys.argv) > 2 else None
hashes = LOGS + (f'{previous}-filehashes.txt' if previous else 'start-filehashes.txt')
delta = subprocess.run(['python3', LOGS + 'delta.py', hashes], capture_output=True, text=True, check=True).stdout.split()
since_start = subprocess.run(['python3', LOGS + 'delta.py'], capture_output=True, text=True, check=True).stdout.split()

def base_text(path):
    for root in ([LOGS + f'tree-{previous}/'] if previous else []) + [LOGS + 'start-base/']:
        if os.path.isfile(root + path):
            return open(root + path, 'rb').read()
    shown = subprocess.run(['git', 'show', f'HEAD:{path}'], capture_output=True)
    return shown.stdout if shown.returncode == 0 else b''

os.makedirs(LOGS + 'tmp-diff', exist_ok=True)
out = open(LOGS + f'{batch}.diff', 'wb')
for path in sorted(delta):
    old = LOGS + 'tmp-diff/old'
    open(old, 'wb').write(base_text(path))
    new = path if os.path.isfile(path) else '/dev/null'
    result = subprocess.run(['diff', '-u', '--label', f'a/{path}', '--label', f'b/{path}', old, new], capture_output=True)
    out.write(result.stdout)
out.close()
shutil.rmtree(LOGS + 'tmp-diff')
tree = LOGS + f'tree-{batch}/'
for path in since_start:
    if os.path.isfile(path):
        os.makedirs(os.path.dirname(tree + path), exist_ok=True)
        shutil.copy2(path, tree + path)
print(f'{len(delta)} files in the {batch} delta; {len(since_start)} p09 files copied to {tree}')
