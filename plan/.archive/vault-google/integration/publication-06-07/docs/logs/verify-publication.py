from pathlib import Path
import hashlib,json,re,subprocess,tarfile
root=Path(__file__).resolve().parents[7];plan=root/'plan/.archive/vault-google';logs=Path(__file__).parent
preservation=json.loads((logs/'archive-preservation.json').read_text());allowed={'README.md','07-integration-verification-and-operations.md','COORDINATION.md'}
for rel,digest in preservation['before_files'].items():
 file=plan/rel;assert file.is_file(),rel
 if rel not in allowed:assert hashlib.sha256(file.read_bytes()).hexdigest()==digest,rel
with tarfile.open(plan/'phases/p07/docs/logs/source-B1-R2.tar.gz') as archive:
 source={m.name:archive.extractfile(m).read() for m in archive.getmembers() if m.isfile()}
adapted={'.github/workflows/ci.yml','docs/vault-integrations.md','scripts/check-integration-docs.py','src/main.rs'}
for rel,content in source.items():
 if rel not in adapted:assert (root/rel).read_bytes()==content,rel
links=[]
for file in list(plan.glob('*.md'))+[root/p for p in ['docs/vault-integrations.md','docs/gmail-workflows.md','plan/cloudroom-local/README.md','plan/doop-canvas/README.md']]:
 for target in re.findall(r'\[[^\]]+\]\(([^)]+)\)',file.read_text()):
  path=target.partition('#')[0]
  if not path or '://' in path or path.startswith(('mailto:','/')):continue
  assert (file.parent/path).exists(),(str(file),target)
  links.append(target)
assert not (root/'plan/vault-google').exists()
assert '<<<<<<<' not in (root/'.github/workflows/ci.yml').read_text()
assert not subprocess.check_output(['git','diff','--name-only','--diff-filter=U'],cwd=root,text=True).strip()
print(json.dumps({'preserved_archival_files':len(preservation['before_files']),'byte_identical_source_files':len(source)-len(adapted),'adapted_only':sorted(adapted),'valid_local_links':len(links),'old_plan_absent':True,'merge_conflicts_resolved':True},sort_keys=True))
