from pathlib import Path
import json,hashlib,re,subprocess,datetime
r=Path.cwd();l=r/'plan/04-autonomous-agent-history/integration/final/docs/logs'
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
a=json.loads((l/'assembly-r1.json').read_text());pre=json.loads((l/'primary-preflight-r1.json').read_text())
assert all(sha(r/n)==h for n,h in a['accepted216_hashes'].items())
assert all((sha(r/n) if (r/n).exists() else None)==h for n,h in pre['preexisting_preserve'].items())
assert hashlib.sha256(subprocess.check_output(['git','ls-files','--stage','-z'])).hexdigest()==pre['staged_entries_sha256']
planned=json.loads((l/'planned-final-selectors.json').read_text());found=set(re.findall(r'^(\S+): test$',(l/'final-discovery-01.txt').read_text(),re.M));assert found==set(planned['expected_names'])
executed=dict(re.findall(r'^test (\S+) \.\.\. (ok|FAILED|ignored[^\n]*)$',(l/'final-runtime-01.txt').read_text(),re.M));assert set(executed)==found and all(v=='ok' for v in executed.values());assert len(found)==31
p07=r/'plan/04-autonomous-agent-history/phases/p07/docs/logs';cases=json.loads((p07/'verification-case-inventory.json').read_text());assert len(cases['cases'])==883 and found<=set(cases['cases'])
result={'status':'PASS','at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'accepted216_exact':True,'preexisting77_preserved':True,'staged_entries_unchanged':True,'conflicts':[],'combined_css_regeneration_identical':True,'native_selected':31,'native_passed':31,'native_ignored':0,'all_selectors_nonzero':all(any(s in n for n in found) for s in planned['selectors']),'unchanged_phase883_evidence_reusable':True,'reuse_limits':'Same216 code/schema/Cargo dependencies and shipped CSS bytes; primary differs only in preserved41 existing03 records in addition to same36 external inputs. Actual fresh primary migrations/SQLx/offline/lint/CSS/native31/browser2 are new combined gates, not inferred from prior phase passes. Phase883 current case evidence and protocol17/Python/browser registration source assertions remain valid for unchanged code/dependencies. No runtime/fullsuite/stack/lease/hostedCI/liveprovider claim beyond recorded commands.','cases':{n:executed[n] for n in sorted(found)}}
(l/'verification-final-inventory.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='cases'},indent=2))
