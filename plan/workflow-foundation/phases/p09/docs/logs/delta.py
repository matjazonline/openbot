"""Files whose content differs from a recorded per-file hash list (default: the p09 start).
Usage (from the repository root): python3 <this> [hashes-file]. Prints one path per line."""
import hashlib, os, subprocess, sys
base = sys.argv[1] if len(sys.argv) > 1 else 'plan/workflow-foundation/phases/p09/docs/logs/start-filehashes.txt'
known = {}
for line in open(base):
    digest, path = line.rstrip('\n').split('  ', 1)
    known[path] = digest
excludes = ['plan/workflow-foundation/phases/p09/docs', 'plan/workflow-foundation/phases/p08/docs',
            'plan/workflow-foundation/COORDINATION.md', 'marketing', 'website']
out = subprocess.run(['bash', '.claude/skills/implement-opus-reviewed-2_2/scripts/snapshot.sh', *excludes],
                     capture_output=True, text=True, check=True).stdout
for path in [line.strip() for line in out.splitlines()[3:]] + [p for p in known if p not in out]:
    digest = hashlib.sha256(open(path, 'rb').read()).hexdigest() if os.path.isfile(path) else 'deleted'
    if known.get(path) != digest:
        print(path)
