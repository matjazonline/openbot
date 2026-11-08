import pathlib,subprocess,sys,json
logs=pathlib.Path(__file__).resolve().parent
runner=logs/"run-check.py"
checks=[
("reviewed-email-list-02",["cargo","test","--locked","--offline","--lib","workflow_cutover_reviewed_email_delivers_exact_accepted_artifact","--","--list"]),
("reviewed-email-runtime-02",["scripts/test-network-isolation.sh","cargo","test","--locked","--offline","--lib","workflow_cutover_reviewed_email_delivers_exact_accepted_artifact"]),
("message-delivery-list-01",["cargo","test","--locked","--offline","--lib","workflow_message_delivery_","--","--list"]),
("message-delivery-runtime-01",["scripts/test-network-isolation.sh","cargo","test","--locked","--offline","--lib","workflow_message_delivery_"]),
("source-modules-01",["cargo","test","--locked","--offline","--test","source_modules"]),
("sqlx-prepare-final-01",["cargo","sqlx","prepare","--","--all-targets"]),
("sqlx-check-final-01",["cargo","sqlx","prepare","--check","--","--all-targets"]),
("clippy-final-01",["cargo","clippy","--locked","--offline","--all-targets","--","-D","warnings"]),
("fmt-final-01",["cargo","fmt","--all","--","--check"]),
("diff-final-02",["git","diff","--check"]),
("graph-final-02",["graft","build"]),
]
results=[]
for name,command in checks:
 result=subprocess.run([sys.executable,str(runner),name,*command])
 results.append({"name":name,"exit_code":result.returncode})
 (logs/"final-checks-02.json").write_text(json.dumps(results,indent=2)+"\n")
 if result.returncode:sys.exit(result.returncode)
