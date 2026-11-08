import os,json,hashlib,subprocess,tarfile,re
from pathlib import Path
from datetime import datetime,timezone
r=Path.cwd(); l=r/"plan/04-autonomous-agent-history/phases/p04/docs/logs"
i=json.loads((l/"AH-P04-B1-R1-input.json").read_text())
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
env=os.environ.copy();env["PATH"]=i["snapshot_adapter"]+os.pathsep+env["PATH"]
s=subprocess.check_output(["bash",i["skill"]+"/scripts/snapshot.sh",*i["exclusions"]],env=env,text=True)
(l/"AH-P04-root-code-snapshot.txt").write_text(s)
assert "digest: "+i["source_digest"] in s
assert subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip()==i["head"]
assert subprocess.check_output(["git","branch","--show-current"],text=True).strip()==i["branch"]
for k in ["decision","scope","owned_archive","producer_archive","coverage"]: assert sha(i[k])==i[k+"_sha256"],k
for p in [i["coordination_authority"],i["coordination_pin"],r/"plan/04-autonomous-agent-history/COORDINATION.md"]: assert sha(p)==i["coordination_sha256"],str(p)
scope=json.loads(Path(i["scope"]).read_text())
assert len(scope["owned_hashes"])==i["owned_count"]==128
for p,h in {**scope["owned_hashes"],**i["external_paths"]}.items():
 f=r/p;assert (sha(f) if f.exists() else None)==h,p
with tarfile.open(i["owned_archive"]) as tar:
 members={m.name:m for m in tar.getmembers() if m.isfile()}
 assert set(members)==set(scope["owned_hashes"])
 for p,h in scope["owned_hashes"].items(): assert hashlib.sha256(tar.extractfile(members[p]).read()).hexdigest()==h,p
for name,c in i["checks"].items():
 for k in ["receipt","log"]: assert sha(c[k])==c[k+"_sha256"],name
 assert json.loads(Path(c["receipt"]).read_text())["exit_code"]==0,name
raw=(l/"final-focused-runtime-02.log").read_text()
passed=set(re.findall(r"^test (\S+) \.\.\. ok$",raw,re.M))
selected=set(re.findall(r"^(\S+): test$",(l/"final-focused-list-02.log").read_text(),re.M))
coverage=json.loads(Path(i["coverage"]).read_text())
assert passed==selected==set(coverage["test_names"]) and len(passed)==597
assert sum("agent_history_" in n for n in passed)==99
assert not re.search(r"^test \S+ \.\.\. (FAILED|ignored)",raw,re.M)
for n in ["AH-P04-B1-R1-reviewer-entry.json","AH-P04-B1-R1-reviewer-exit.json"]:
 d=json.loads((l/n).read_text())
 for k in ["digest","head","branch","decision","archive","producer_archive","scope","coordination","archive_names_match_scope"]: assert d[k] is True,(n,k)
 for k in ["owned_mismatches","external_mismatches","archive_content_mismatches"]: assert d[k]==[],(n,k)
 if "check_hash_mismatches" in d: assert d["check_hash_mismatches"]==[],n
assert "PASS" in (l.parent/"REVIEW.md").read_text().split("## AH-P04-B1-R1",1)[1]
out={"acceptance":"AH-P04-code-acceptance","at":datetime.now(timezone.utc).isoformat(),"result":"accepted in phase; not integrated/pushed","review":"AH-P04-B1-R1 PASS","source_digest":i["source_digest"],"head":i["head"],"coordination_revision":17,"coordination_sha256":i["coordination_sha256"],"owned":128,"phase_delta":42,"external":36,"archive_sha256":i["owned_archive_sha256"],"distinct_targeted_passes":597,"history_passes":99,"ignored":0,"checks":"all pinned receipts/log hashes and exit statuses, exact selected/executed names verified; no runtime reruns","records":"root read complete new B review/notes/findings/entry/exit before executing acceptance; no unresolved actionable findings","exclusions":i["exclusions"],"next":"publish accepted producer C005/r4 and start ordered P05"}
(l/"AH-P04-code-acceptance.json").write_text(json.dumps(out,indent=2)+"\n");print(json.dumps(out,indent=2))
