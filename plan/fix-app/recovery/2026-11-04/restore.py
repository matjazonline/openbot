#!/usr/bin/env python3
"""Restore paused fix-app work into a NEW directory; never resumes implementation."""
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tarfile

package = Path(__file__).resolve().parent

def run(*args, cwd=None):
    subprocess.run(args, cwd=cwd, check=True)

def extract(archive, destination):
    with tarfile.open(archive, 'r:gz') as source:
        for member in source:
            relative = PurePosixPath(member.name)
            if relative.is_absolute() or '..' in relative.parts or not member.isfile():
                raise RuntimeError('Unsupported archive entry: ' + member.name)
            target = destination.joinpath(*relative.parts)
            target.parent.mkdir(parents=True, exist_ok=True)
            with source.extractfile(member) as content:
                target.write_bytes(content.read())
            target.chmod(0o755 if member.mode & 0o111 else 0o644)

if len(sys.argv) != 2:
    sys.exit('Usage: python3 restore.py /absolute/path/to/NEW/openbot-recovery')
destination = Path(sys.argv[1]).expanduser().absolute()
if destination.exists():
    sys.exit('Refusing to overwrite an existing destination: ' + str(destination))

checksums = json.loads((package / 'checksums.json').read_text())
for name, expected in checksums.items():
    actual = hashlib.sha256((package / name).read_bytes()).hexdigest()
    if actual != expected:
        sys.exit('Package checksum mismatch: ' + name)
manifest = json.loads((package / 'manifest.json').read_text())
repository = subprocess.check_output(
    ['git', 'rev-parse', '--show-toplevel'], cwd=package, text=True).strip()
run('git', 'cat-file', '-e', manifest['base'] + '^{commit}', cwd=repository)
run('git', 'clone', '--no-hardlinks', '--origin', 'checkpoint_source', '--branch', 'main',
    repository, str(destination))
run('git', 'merge-base', '--is-ancestor', manifest['main'], 'HEAD', cwd=destination)
exclude = destination / '.git/info/exclude'
with exclude.open('a') as handle:
    handle.write('\n.worktrees/\n.recovery-workflow/\n')
for phase, details in manifest['phases'].items():
    worktree = destination / '.worktrees' / ('fix-app-' + phase)
    run('git', 'worktree', 'add', '-b', 'fix-app/' + phase, str(worktree), details['head'], cwd=destination)
    run('git', 'apply', '--binary', str(package / (phase + '.patch')), cwd=worktree)
    extract(package / (phase + '-files.tar.gz'), worktree)
    for relative, expected in details['files'].items():
        path = worktree / relative
        if 'symlink' in expected:
            matches = path.is_symlink() and os.readlink(path) == expected['symlink']
        else:
            matches = path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == expected['sha256']
            matches = matches and bool(path.stat().st_mode & 0o111) == expected['executable']
        if not matches:
            sys.exit('Restored source differs: ' + phase + '/' + relative)
    print('VERIFIED', phase, len(details['files']), 'source files', flush=True)
extract(package / 'manager-files.tar.gz', destination)
extract(package / 'workflow-skill.tar.gz', destination / '.recovery-workflow/implement-astra-reviewed-2-2')
print('\nRestored paused work. No implementation, tests, commits or network operations were run.')
print('Read:', destination / 'plan/fix-app/integration/resumed-all/docs/HANDOFF.md')
print('Old /Users/mac03/dev/openbot paths in historical records must be mapped to:', destination)
print('Recreate disposable databases locally; do not try to restore the old host clusters.')
