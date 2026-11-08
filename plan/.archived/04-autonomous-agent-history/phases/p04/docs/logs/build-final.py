import subprocess
runner="plan/04-autonomous-agent-history/phases/p04/docs/logs/run-check.py"
checks=[
("sqlx-prepare-01",["cargo","sqlx","prepare","--","--locked","--offline","--all-targets"]),
("sqlx-check-01",["cargo","sqlx","prepare","--check","--","--locked","--offline","--all-targets"]),
("offline-alltargets-01",["cargo","check","--locked","--offline","--all-targets"]),
("clippy-01",["cargo","clippy","--locked","--offline","--all-targets","--","-D","warnings"]),
("fmt-check-01",["cargo","fmt","--all","--","--check"]),
("diff-check-01",["git","diff","--check"]),
("graft-build-01",["graft","build"])]
for name,command in checks:
 result=subprocess.run(["python3",runner,name,*command])
 if result.returncode: raise SystemExit(result.returncode)
