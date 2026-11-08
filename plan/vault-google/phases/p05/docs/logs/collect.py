import hashlib,json,pathlib,re
logs=pathlib.Path(__file__).parent
labels=['affected-vault-01','contracts-01','connect-regression-01','connect-routes-01','http-regression-01','mcp-regression-01','approval-regression-01','rotation-regression-01','sqlx-prepare-01','sqlx-check-01','offline-check-01','clippy-02','format-01','diff-01','css-01','transport-fixtures-01','transport-boundary-01','sample-boundary-01','graft-build-01','migrate-01','renewal-scope-regression-01']
tests=set();receipts=[]
for label in labels:
 r=json.loads((logs/(label+'.json')).read_text());assert r['exit']==0,(label,r)
 content=(logs/(label+'.log')).read_text()
 names=re.findall(r'^test (.+) \.\.\. ok$',content,re.M)
 tests.update(names);r['test_results']=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',content)
 r['label']=label;r['log_sha256']=hashlib.sha256((logs/(label+'.log')).read_bytes()).hexdigest()
 if label in labels[:8]:assert names,(label,'zero matched tests')
 receipts.append(r)
inventory=(logs/'test-inventory-01.log').read_text()
families={prefix:sum(': test' in n and n.rsplit('::',1)[-1].startswith(prefix) for n in inventory.splitlines()) for prefix in ['integration_renewal_','google_refresh_','google_refresh_race_','google_connection_lifecycle_','vault_rotation_']}
assert all(families.values())
result={'distinct_tests':len(tests),'tests':sorted(tests),'listed_families':families,'receipts':receipts,'reuse':'Migrate01 followed exact-owned fresh reset; schema unchanged since. Accepted03/04 untouched evidence retained; current affected contracts/authority/temporal/connect/HTTP/MCP/approvals/rotation rerun. 45 new renewal cases included in current125vault PASS. Failed early receipts retained; final full suite pending after independent code acceptance.'}
assert not (logs/'checks-R1.json').exists()
(logs/'checks-R1.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'distinct_tests':len(tests),'listed_families':families}))
