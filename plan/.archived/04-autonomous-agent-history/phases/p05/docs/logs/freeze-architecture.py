import os,json,hashlib,subprocess,tarfile,re
from pathlib import Path
from datetime import datetime,timezone
r=Path.cwd();l=r/"plan/04-autonomous-agent-history/phases/p05/docs/logs"
producer=json.loads((l/"producer-input.json").read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def save(p,d):p.write_text(json.dumps(d,indent=2)+"\n")
for name,h in producer["external_paths"].items():
 p=r/name;assert (sha(p) if p.exists() else None)==h,name
excludes=producer["exclusions"];spec=[".",*[":(top,exclude,literal)"+p for p in excludes]]
paths=set()
for cmd in [["git","diff","--name-only","HEAD","--",*spec],["git","ls-files","--others","--exclude-standard","--",*spec]]:paths.update(subprocess.check_output(cmd,text=True).splitlines())
owned={p:sha(r/p) if (r/p).exists() else None for p in sorted(paths-set(producer["external_paths"]))}
assert all(v is not None for v in owned.values())
scope=l/"author-scope-architecture-r1.json";save(scope,{"owned_hashes":owned,"owned_count":len(owned),"status":"current unaccepted P05; architecture only"})
archive=l/"AH-P05-AG1-R1-owned-source.tar"
with tarfile.open(archive,"w") as tar:
 for name in owned:tar.add(r/name,arcname=name)
assert subprocess.run(["git","check-ignore",str(archive)],stdout=subprocess.DEVNULL).returncode==1
env=os.environ.copy();env["PATH"]=str(l/"tools")+os.pathsep+env["PATH"]
skill="/private/tmp/openbot-ah-p05-workflow-skill"
snapshot=subprocess.check_output(["bash",skill+"/scripts/snapshot.sh",*excludes],env=env,text=True)
(l/"AH-P05-AG1-R1-author-snapshot.txt").write_text(snapshot)
digest=re.search(r"^digest: (\w+)$",snapshot,re.M)[1]
auth=Path("/Users/mac02/dev/openbot/plan/04-autonomous-agent-history/COORDINATION.md");pin=l/"coordination-r20.md"
assert sha(auth)==sha(pin)==sha(r/"plan/04-autonomous-agent-history/COORDINATION.md")
repro=l/"AH-P05-AG1-R1-current-repro.json";receipt=json.loads(repro.read_text());assert receipt["exit_code"]==101
log=Path(receipt["log"]);assert "left: None" in log.read_text() and "knowledge.audience_below_content" in log.read_text()
# Preserve earlier exploratory output without editing any original receipt or log.
for old in l.glob("*.log"):
 copy=l/(old.stem+".retained.txt")
 if copy.exists():assert sha(copy)==sha(old)
 else:copy.write_bytes(old.read_bytes())
evidence=l/"AH-P05-AG1-R1.evidence.json"
save(evidence,{"version":1,"round":"AH-P05-AG1-R1","artifact":digest,"checks":[
 {"criteria":["AC6","AC8-reproduction"],"command":" ".join(receipt["command"]),"fixture":"owned loopback PG18.6/P05 fixtures; scripted providers; stock2MiB stack","status":"FAIL","files":[{"path":"logs/"+log.name,"sha256":sha(log)},{"path":"logs/"+repro.name,"sha256":sha(repro)}]},
 {"criteria":["AC8-correction-owner-matrix"],"command":"planned focused native final-response owner/refusal correction checks","fixture":"P05 owned databases","status":"NOT EXERCISED","reason":"architecture gate precedes dependent native implementation"}]})
validation=subprocess.run(["python3",skill+"/scripts/check-evidence.py",str(evidence),"--expect-artifact",digest],capture_output=True,text=True)
(l/"AH-P05-AG1-R1-evidence-validation.txt").write_text(validation.stdout+validation.stderr);assert validation.returncode==0,validation.stdout+validation.stderr
i={"review":"AH-P05-AG1-R1","stage":"consequential architecture gate; NOT code review/acceptance","at":datetime.now(timezone.utc).isoformat(),"worktree":str(r),"branch":subprocess.check_output(["git","branch","--show-current"],text=True).strip(),"phase_start":producer["phase_start"],"review_base":producer["review_base"],"head":subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),"source_digest":digest,"exclusions":excludes,"snapshot_adapter":str(l/"tools"),"skill":skill,"skill_sha256":sha(Path(skill)/"SKILL.md"),"decision":str(l/"AH-P05-D1-r1.md"),"decision_revision":"AH-P05-D1/r1","decision_sha256":sha(l/"AH-P05-D1-r1.md"),"coordination_authority":str(auth),"coordination_revision":20,"coordination_sha256":sha(auth),"coordination_pin":str(pin),"notices":[*["AH-I00"+str(n)+"/r1" for n in range(1,8)],"AH-C001/r1","AH-C002/r5","AH-C003/r2","AH-C004/r3","AH-C005/r4","AH-C006/r1 proposed"],"scope":str(scope),"scope_sha256":sha(scope),"owned_count":len(owned),"owned_archive":str(archive),"owned_archive_sha256":sha(archive),"external_paths":producer["external_paths"],"producer_source":producer["source_digest"],"producer_archive":producer["owned_archive"],"producer_archive_sha256":producer["owned_archive_sha256"],"producer_acceptance":str(r/"plan/04-autonomous-agent-history/phases/p04/docs/logs/AH-P04-code-acceptance.json"),"evidence":str(evidence),"evidence_sha256":sha(evidence),"reproduction_receipt":str(repro),"reproduction_receipt_sha256":sha(repro),"reproduction_log":str(log),"reproduction_log_sha256":sha(log),"checks_in_progress":False,"source_status":"FROZEN; current required typed-code case FAIL; no dependent native edits; design gate only"}
save(l/"AH-P05-AG1-R1-input.json",i)
print(json.dumps({"digest":digest,"owned":len(owned),"archive_sha256":sha(archive),"decision":i["decision_sha256"],"evidence_validator":"PASS (retention/schema only; current reproduction FAIL and correction NOT EXERCISED)"},indent=2))
