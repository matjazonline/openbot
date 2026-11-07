import os,json,pathlib,subprocess,sys,time,datetime
logs=pathlib.Path(__file__).resolve().parent
manifest=json.loads((logs/"shipping-source-manifest.json").read_text())
resources=json.loads((logs/"resources.json").read_text())
name,*command=sys.argv[1:]
env=os.environ.copy();env.pop("ALLOW_MISSING_DATABASE_URL",None)
settings={"DATABASE_URL":resources["database_url"],"TEST_DATABASE_URL":resources["test_database_url"],"SQLX_OFFLINE":"true","CARGO_INCREMENTAL":"1","CARGO_TARGET_DIR":resources["target"],"CARGO_BUILD_JOBS":"4","RUST_MIN_STACK":"2097152"}
env.update(settings);start=time.monotonic();at=datetime.datetime.now(datetime.timezone.utc).isoformat();output=logs/(name+".log")
with output.open("w") as stream:r=subprocess.run(command,cwd=manifest["worktree"],env=env,stdout=stream,stderr=subprocess.STDOUT)
receipt={"command":command,"cwd":manifest["worktree"],"environment":settings,"started":at,"seconds":round(time.monotonic()-start,3),"exit_code":r.returncode,"log":str(output),"shipping_source_manifest":"shipping-source-manifest.json"}
(logs/(name+".json")).write_text(json.dumps(receipt,indent=2)+"\n")
print(json.dumps(receipt));print("\n".join(output.read_text(errors="replace").replace("\r","\n").splitlines()[-20:]));sys.exit(r.returncode)
