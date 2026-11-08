import hashlib,json,pathlib,re
logs=pathlib.Path(__file__).parent
old=json.loads((logs/'checks-R1.json').read_text());tests=set(old['tests']);receipts=[]
labels=['style-correction-02','vault-routes-correction-01','main-binary-01','offline-correction-01','clippy-correction-01','format-correction-01','diff-correction-01','transport-correction-01','graft-correction-01']
for label in labels:
 r=json.loads((logs/(label+'.json')).read_text());assert r['exit']==0,(label,r)
 s=(logs/(label+'.log')).read_text();names=re.findall(r'^test (.+) \.\.\. ok$',s,re.M)
 if label in labels[:3]:assert names,(label,'zero tests')
 tests.update(names);r['label']=label;r['test_results']=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',s)
 r['log_sha256']=hashlib.sha256((logs/(label+'.log')).read_bytes()).hexdigest();receipts.append(r)
assert '3347 filtered out' in (logs/'style-correction-02.log').read_text()
base=json.loads((logs/'baseline-overflow-01.json').read_text());assert base['same_failure_reproduced'] and not base['baseline_status_before'] and not base['baseline_status_after']
proof=json.loads((logs/'fixture-relocation-R2.json').read_text());assert proof['old_sha256']==proof['new_sha256']
full=json.loads((logs/'full-coverage-R1.json').read_text());assert len(full['successful_gates'])==13 and not full['stack_overflow_seen']
r={'distinct_focused_tests_with_reuse':len(tests),'tests':sorted(tests),'current_receipts':receipts,'reuse':'R1 whole52path independent review/238author tests/45independent renewal tests; every production/schema/query/contract byte unchanged in R2. New evidence covers unchanged fixture relocation, scanner and previously-unreached main target, current offline/lint/fmt/boundaries. Two full library runs3337PASS/2FAIL/9ignored preserved; no stackoverflow. Original migrations/SQLx/13green full gates reused with unchanged source.','correction':'VF1 resolved without changing scanner or escaping assertions; independent reviewer pure test PASS','unrelated_failure':'VF2 exact same SQLSTATE55006 at setup line85 reproduced on clean baselineHEAD97e5d60d/fresh owned DB. Leave unrelated source unchanged.','full_suite_status':'NOT GREEN; scripts/check.sh exit1 retained. Root affected verification passes; known unrelated overflow fixture remains.9defaultignored exports/optionalDNS listed in full-coverage-R1.json.','failed_receipts':'Early phase failures, full run, baselineexpectedfailure, and style-correction01 stale baseline artifact retained. Own package cleaned; style02 fresh current3348-test executable verified.'}
assert not (logs/'checks-R2.json').exists();(logs/'checks-R2.json').write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'distinct_focused_tests_with_reuse':len(tests),'current_test_results':[(x['label'],x['test_results']) for x in receipts[:3]],'full_suite_status':r['full_suite_status']}))
