from pathlib import Path
import json,re,hashlib
l=Path(__file__).resolve().parent
found=set(re.findall(r'^(\S+): test$',(l/'pub-discovery-01.txt').read_text(),re.M));assert found
actual=dict(re.findall(r'^test (\S+) \.\.\. (ok|FAILED|ignored[^\n]*)$',(l/'pub-runtime-01.txt').read_text(),re.M))
assert found==set(actual),{'missing':sorted(found-set(actual)),'extra':sorted(set(actual)-found)}
assert all(v=='ok' for v in actual.values())
selectors=json.loads((l/'selectors.json').read_text());counts={s:sum(s in n for n in found) for s in selectors};assert all(counts.values())
result={'status':'PASS','selected':len(found),'latest_passes':len(found),'ignored':0,'all_selectors_nonzero':True,'selectors':counts,'cases':{n:actual[n] for n in sorted(found)}}
(l/'case-inventory.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps({k:v for k,v in result.items() if k!='cases'},indent=2))
