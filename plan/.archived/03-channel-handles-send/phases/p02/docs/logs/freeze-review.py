#!/usr/bin/env python3
"""Freeze cumulative source and the p02 delta relative to accepted p01."""
import difflib,hashlib,json,os,subprocess,sys,tarfile
from pathlib import Path
root=Path.cwd();logs=Path(__file__).resolve().parent
previous=root/"plan/03-channel-handles-send/phases/p01/docs/logs/CHS-P01-B1-R2-manifest.json"
base=json.loads(previous.read_text());excludes=base["exclusions"]
identity=sys.argv[1] if len(sys.argv)>1 else "CHS-P02-B1-R1"
assert not (logs/(identity+"-manifest.json")).exists(),"Never overwrite a reviewed identity"
env=os.environ.copy();env["PATH"]=str(previous.parent/"tools")+os.pathsep+env["PATH"]
result=subprocess.check_output(["bash",str(root/".agents/skills/implement-astra-reviewed-2-2/scripts/snapshot.sh"),*excludes],env=env,text=True)
(logs/(identity+"-snapshot.txt")).write_text(result)
lines=result.splitlines();digest=next(line.split()[1] for line in lines if line.startswith("digest:"))
paths=[line.strip() for line in lines if line.startswith("  ")]
sha=lambda data:hashlib.sha256(data).hexdigest()
with tarfile.open(previous.parent/"CHS-P01-B1-R2-source.tar.gz") as archive:
 saved={m.name:archive.extractfile(m).read() for m in archive.getmembers() if m.isfile()}
changes=[];delta=[]
for path in paths:
 if path in saved:old=saved[path]
 else:
  r=subprocess.run(["git","show",base["head"]+":"+path],capture_output=True)
  old=r.stdout if r.returncode==0 else b""
 current=(root/path).read_bytes() if (root/path).exists() else b""
 if old!=current:
  changes.append(path)
  delta.extend(difflib.unified_diff(old.decode().splitlines(keepends=True),current.decode().splitlines(keepends=True),fromfile="a/"+path,tofile="b/"+path))
(logs/(identity+"-p02.diff")).write_text("".join(delta))
archive_path=logs/(identity+"-source.tar.gz")
with tarfile.open(archive_path,"w:gz") as archive:
 for path in paths:archive.add(root/path,arcname=path,recursive=False)
coord=root/"plan/03-channel-handles-send/COORDINATION.md"
assert coord.read_bytes()==(logs/"coordination-r3.md").read_bytes()
manifest={"phase":"p02","batch":"CHS-P02-B1","start":base["head"],"review_base":base["head"],
 "head":subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),"digest":digest,
 "exclusions":excludes,"source_files":{path:sha((root/path).read_bytes()) for path in paths},
 "p02_changed_files":changes,"preserved_prerequisite_manifest":str(previous),"previous_digest":base["digest"],
 "source_archive":archive_path.name,"source_archive_sha256":sha(archive_path.read_bytes()),
 "p02_delta":identity+"-p02.diff","architecture":"CHS-P02-D1/r2","architecture_pass":"CHS-P02-AG1-R1",
 "coordination_revision":3,"coordination_sha256":sha(coord.read_bytes()),"notice_revisions":["I001-I003/r1","I004/r2","I005-I006/r1","C001/r1"],
 "checks":"author-focused-checks.json and final-*-01.json","full_suite":False}
if identity=="CHS-P02-B1-R2":
 first=json.loads((logs/"CHS-P02-B1-R1-manifest.json").read_text())
 with tarfile.open(logs/first["source_archive"]) as archive:
  first_source={m.name:archive.extractfile(m).read() for m in archive.getmembers() if m.isfile()}
 correction=[];correction_files=[]
 for path in paths:
  if path in first_source:old=first_source[path]
  else:
   r=subprocess.run(["git","show",first["head"]+":"+path],capture_output=True)
   old=r.stdout if r.returncode==0 else b""
  current=(root/path).read_bytes()
  if old!=current:
   correction_files.append(path)
   correction.extend(difflib.unified_diff(old.decode().splitlines(keepends=True),current.decode().splitlines(keepends=True),fromfile="a/"+path,tofile="b/"+path))
 correction_name="CHS-P02-B1-R1-to-R2.diff"
 (logs/correction_name).write_text("".join(correction))
 manifest.update({"previous_review_digest":first["digest"],"correction_files":correction_files,
  "correction_delta":correction_name,"findings":["CHS-P02-B1-F1","CHS-P02-B1-F2"],
  "correction_checks":"author-correction-checks.json and correction-{clippy,fmt,diff,graft}-01.json",
  "reused_checks":"R1 SQL/schema/query metadata unchanged; prior220 checks plus correction rechecks"})
(logs/(identity+"-manifest.json")).write_text(json.dumps(manifest,indent=2)+"\n")
print(json.dumps({"digest":digest,"source_files":len(paths),"p02_changed_files":len(changes)}))
