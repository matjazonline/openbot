from pathlib import Path
import json,collections
edits=collections.defaultdict(set)
for line in Path('plan/01-knowledge-access-levels/phases/p01/docs/logs/WB1-r2-clippy.jsonl').read_text().splitlines():
 try: obj=json.loads(line)
 except:continue
 if obj.get('reason')!='compiler-message' or obj['message']['level']!='error':continue
 for child in obj['message']['children']:
  for span in child['spans']:
   if span.get('suggestion_applicability')=='MachineApplicable' and span.get('suggested_replacement') is not None:
    edits[span['file_name']].add((span['byte_start'],span['byte_end'],span['suggested_replacement']))
for filename,changes in edits.items():
 p=Path(filename); data=p.read_bytes(); last=len(data)+1
 for start,end,replacement in sorted(changes,reverse=True):
  assert end<=last,(filename,start,end,last)
  data=data[:start]+replacement.encode()+data[end:];last=start
 p.write_bytes(data)
print({'files':len(edits),'machine_applicable_edits':sum(map(len,edits.values()))})
