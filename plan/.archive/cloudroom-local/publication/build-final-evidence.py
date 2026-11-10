"""Build the final immutable publication manifest only after every required gate ends."""
from pathlib import Path
import hashlib,json,re,shlex
PUB=Path(__file__).resolve().parent
ARTIFACT='fbc00a75eb16d9c9cc00f4441976aaa9e176503a54d7dfad8a896d96d1143110'
checks=[]
def file(name):
 p=PUB/name;assert p.is_file(),name
 return {'path':name,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
def add(criteria,command,status,names,fixture='frozen publication tree f63f08e0; task-owned fixtures only',reason=None):
 check={'criteria':criteria,'command':command,'fixture':fixture,'status':status,'files':[file(n) for n in names]}
 if reason:check['reason']=reason
 checks.append(check)
def results(name,criterion,exclude=()):
 for row in json.loads((PUB/name).read_text()):
  if row['name'] not in exclude:add([criterion,row['name']],shlex.join(row['command']),row['status'],[row['log']])
final=json.loads((PUB/'final-rust-checks.json').read_text())
assert {r['name'] for r in final}=={'full-suite','stack-budget','lease-budget'},'required Rust gates unfinished'
assert next(r for r in final if r['name']=='lease-budget')['status']=='PASS','lease budget failed'
stock=(PUB/'logs/stack-budget.log').read_text();summary=re.search(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',stock);assert summary,'stock suite did not finish'
assert sum(map(int,summary.groups()))==3761,'stock inventory count changed'
allowed={'adapters::http::pages::tests::rendered_markup_carries_no_inline_javascript','adapters::mcp::tests::mcp_dropped_call_sends_protocol_cancellation_and_joins_cleanup'}
failed=set(re.findall(r'^test (.*?) \.\.\. FAILED$',stock,re.M));assert failed<=allowed,failed
assert 'has overflowed its stack' not in stock
cloudroom=[line for line in stock.splitlines() if line.startswith('test ') and 'cloudroom_' in line and line.endswith(' ... ok')];assert len(cloudroom)==129,len(cloudroom)
results('round1-rust-checks.json','AC-shared-static',exclude=('full-suite',))
results('static-checks.json','AC-static-and-browser')
results('browser-checks.json','AC-generated-browser')
results('final-rust-checks.json','AC-combined-Rust')
results('image-checks.json','AC-current-image',exclude=('image-check',))
results('image-correction-checks.json','AC-current-image')
add(['AC-current-local-effects','AC-current-recovery','AC-current-2MiB'],'scripts/test-network-isolation.sh scripts/stack-budget.sh --offline; inspect complete current registered local-effect families','PASS',['logs/stack-budget.log','test-inventory-comparison.json'],reason=None)
add(['AC-recovery-recheck'],'exact current cloudroom_e2e_cut_points_reconstruct_server_plugin_and_workers_without_blind_native_retry','PASS',['logs/cut-points-focused-r1.log'],'same frozen binary and owned UTF8 PostgreSQL18.6 port56390; earlier full-run timeout retained with unknown cause')
add(['AC-inherited-MCP-disposition'],'compile exact origin b8c46300 and run its unchanged MCP cancellation test','FAIL',['logs/mcp-origin-baseline-build-and-test-r4.log','mcp-origin-baseline-disposition.json','logs/reviewer-pub-r1-mcp-baseline-audit.json'],'exact unchanged origin baseline; identical3of4-exchanges failure predates Cloudroom merge')
add(['AC-inherited-CSP-disposition'],'compile exact origin scanner test/helper against its own origin source snapshot and run1test','FAIL',['logs/origin-markup-test.rs','logs/origin-markup-test.log','markup-baseline-disposition.json','logs/reviewer-pub-r1-markup-baseline-audit.json'])
add(['AC-inherited-transport-disposition'],'run exact origin transport boundary checker against exact origin source snapshot','FAIL',['logs/transport-boundary-origin-baseline.log','transport-boundary-disposition.json'])
add(['AC-targets'],'cargo test --locked --offline --bin open_bots --test source_modules','PASS',['logs/main-and-module-tests.log'])
add(['AC-plugin'],'npm ci; plugin typecheck/build/test/test:guards','PASS',['logs/preflight-plugin-install.log','logs/preflight-plugin-check.log','logs/preflight-plugin-test.log','logs/preflight-plugin-guards.log'])
add(['AC-reproducible-package'],'npm run package (two exact builds); fresh packaged npm ci --omit=dev','PASS',['logs/plugin-package.log','logs/plugin-runtime-install.log','plugin-package-identity.json','plugin-package-content-comparison.json'])
add(['AC-retained-native-Mac-runtime'],'SHA256 comparison of every packaged runtime file against actual three-provider accepted native artifact; retain prior acceptance identity without claiming a new inference run','PASS',['plugin-package-content-comparison.json','retained-package-artifact.json'])
add(['AC-schema'],'cargo sqlx migrate run on fresh UTF8 PostgreSQL18.6; rerun both Unicode identity tests','PASS',['logs/migrate-utf8.log','logs/auth-locale-correction.log'])
add(['AC-archive'],'compare all1636 original file hashes and historical relocation index; validate live Markdown/startup paths','PASS',['archive-preservation.json','archive-links-final.json','historical-invalid-evidence.json'])
fixtures=json.loads((PUB/'browser-fixture-retention.json').read_text())['files']
add(['AC-retained-browser-inputs'],'retain exact generated production markup and shipped JavaScript used by browser drivers','PASS',['browser-fixture-retention.json',*[p['path'] for p in fixtures]])
add(['AC-source-freeze'],'installed snapshot.sh with exact9workflow exclusions; independent reviewed image input identity','PASS',['logs/review-entry-snapshot.txt','image-inputs.json','logs/reviewer-pub-r1-image-input-audit.json'])
add(['AC-native-Linux'],'real native Linux provider/platform acceptance','NOT EXERCISED',[],reason='User explicitly chose Leave Linux acceptance unverified (I005/r1). Current Linux core/image checks are exercised separately.')
add(['AC-CSS'],'npm run check:css','PASS',['logs/preflight-css.log'])
add(['AC-repo-graph'],'graft build from primary checkout fast-forwarded to exact frozen source f63f08e0','PASS',['logs/graft-primary-final.log'])
add(['AC-resource-cleanup'],'stop/remove only owned fixtures after proof retention; preserve prepared source/packages/ordinary accounts','PASS',['cleanup.json','image-cleanup.json','postgres-capacity-audit.json','logs/cleanup-postgres.log','logs/owned-utf8-postgres.log.gz'])
add(['AC-runtime-identity'],'rustc/cargo/node/npm/psql versions and host architecture','PASS',['runtime-versions.json'])
manifest={'version':1,'round':'CL-PUB-R1.final','artifact':ARTIFACT,'checks':checks}
p=PUB/'logs/CL-PUB-R1.final.evidence.json';assert not p.exists(),'never overwrite immutable acceptance evidence';p.write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({'manifest':str(p),'checks':len(checks),'stock_summary':summary.groups(),'stock_failures':sorted(failed),'cloudroom_pass':len(cloudroom)}))
