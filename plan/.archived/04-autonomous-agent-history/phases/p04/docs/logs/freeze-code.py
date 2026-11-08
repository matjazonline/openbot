import os,json,hashlib,subprocess,tarfile,re,difflib
from pathlib import Path
from datetime import datetime,timezone
r=Path.cwd(); l=r/"plan/04-autonomous-agent-history/phases/p04/docs/logs"
i=json.loads((l/"AH-P04-AG1-R3-input.json").read_text())
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(p,d): p.write_text(json.dumps(d,indent=2)+"\n")
for p,h in i["external_paths"].items():
 f=r/p; assert (sha(f) if f.exists() else None)==h,p
args=[".",*[":(top,exclude,literal)"+p for p in i["exclusions"]]]
paths=set()
for cmd in [["git","diff","--name-only","HEAD","--",*args],["git","ls-files","--others","--exclude-standard","--",*args]]:
 paths.update(subprocess.check_output(cmd,text=True).splitlines())
owned={p:sha(r/p) if (r/p).exists() else None for p in sorted(paths-set(i["external_paths"]))}
assert all(v is not None for v in owned.values()),"unexpected owned deletion"
with tarfile.open(i["producer_archive"]) as tar:
 old={m.name:tar.extractfile(m).read() for m in tar.getmembers() if m.isfile()}
delta=[p for p in sorted(owned) if (r/p).read_bytes()!=old.get(p)]
assert set(old)<=set(owned)
scope=l/"author-scope-code-r1.json"
save(scope,{"owned_hashes":owned,"owned_count":len(owned),"phase04_delta":delta,"phase04_delta_count":len(delta),"external_paths":i["external_paths"]})
archive=l/"AH-P04-B1-R1-owned-source.tar.gz"
with tarfile.open(archive,"w:gz") as tar:
 for p in owned: tar.add(r/p,arcname=p)
diffs=[]
for p in delta:
 diffs.extend(difflib.unified_diff(old.get(p,b"").decode().splitlines(True),(r/p).read_text().splitlines(True),fromfile="accepted-p03/"+p,tofile="current-p04/"+p))
(l/"AH-P04-B1-R1-phase-delta.diff").write_text("".join(diffs))
selected=set(re.findall(r"^(\S+): test$",(l/"final-focused-list-02.log").read_text(),re.M))
passed=set(re.findall(r"^test (\S+) \.\.\. ok$",(l/"final-focused-runtime-02.log").read_text(),re.M))
assert selected==passed and len(passed)==597
assert not re.search(r"^test \S+ \.\.\. (FAILED|ignored)",(l/"final-focused-runtime-02.log").read_text(),re.M)
family={f:sum(f in n for n in passed) for f in ["agent_history_selection_","agent_history_level_","agent_history_preparation_","agent_history_protocol_"]}
assert all(family.values())
cov=l/"verification-coverage-02.json"
save(cov,{"selected":597,"passed":597,"history":sum("agent_history_" in n for n in passed),"families":family,"test_names":sorted(passed),"ignored":0,"explicit_excluded_unrelated_fixture_exporter":"dump_autonomous_workflow_agent_picker_browser_fixtures"})
checks={}
for name in ["migrations-main-final-01","migrations-test-final-01","trusted-scope-audience-runtime-01","final-focused-list-02","final-focused-runtime-02","sqlx-prepare-01","sqlx-check-01","offline-alltargets-01","clippy-01","fmt-check-01","diff-check-01","graft-build-01"]:
 receipt=l/(name+".json"); raw=json.loads(receipt.read_text()); assert raw["exit_code"]==0,name
 log=Path(raw["log"]); checks[name]={"receipt":str(receipt),"receipt_sha256":sha(receipt),"log":str(log),"log_sha256":sha(log),"command":raw["command"],"exit_code":0}
auth=Path(i["coordination_authority"]); coordination=auth.read_bytes()
assert b"Revision: 17" in coordination
pin=l/"coordination-r17.md"; pin.write_bytes(coordination)
(r/"plan/04-autonomous-agent-history/COORDINATION.md").write_bytes(coordination)
env=os.environ.copy(); env["PATH"]=i["snapshot_adapter"]+os.pathsep+env["PATH"]
snapshot=subprocess.check_output(["bash",i["skill"]+"/scripts/snapshot.sh",*i["exclusions"]],env=env,text=True)
(l/"AH-P04-B1-R1-author-snapshot.txt").write_text(snapshot)
digest=re.search(r"^digest: (\w+)$",snapshot,re.M)[1]
i.update({"review":"AH-P04-B1-R1","stage":"whole phase04 code review","at":datetime.now(timezone.utc).isoformat(),"source_digest":digest,"owned_archive":str(archive),"owned_archive_sha256":sha(archive),"scope":str(scope),"scope_sha256":sha(scope),"owned_count":len(owned),"phase04_delta_count":len(delta),"coordination_revision":17,"coordination_sha256":sha(auth),"coordination_pin":str(pin),"coverage":str(cov),"coverage_sha256":sha(cov),"checks":checks,"current_passes":597,"history_passes":99,"ignored":0,"checks_in_progress":False,"architecture_acceptance":str(l/"AH-P04-architecture-acceptance-r3.json"),"source_status":"FROZEN for B; all required affected checks pass; prior failed receipts retained; no full suite or live provider claim"})
i["notices"][-1]="AH-C005/r3 active architecture"
save(l/"AH-P04-B1-R1-input.json",i)
print(json.dumps({"digest":digest,"owned":len(owned),"delta":len(delta),"passed":597,"families":family,"input":str(l/"AH-P04-B1-R1-input.json")},indent=2))
