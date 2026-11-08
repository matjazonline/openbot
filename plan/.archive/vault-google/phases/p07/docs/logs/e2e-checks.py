import pathlib,re,subprocess,sys
logs=pathlib.Path(__file__).parent
cargo=['cargo','test','--locked','--offline','--lib','vault_google_e2e_']
for label,command in [('e2e-list02',cargo+['--','--list']),('e2e-run01',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh']+cargo)]:
 result=subprocess.run([sys.executable,str(logs/'check.py'),label]+command)
 if result.returncode:sys.exit(result.returncode)
 if '-list' in label:
  count=len(re.findall(r': test$',(logs/(label+'.log')).read_text(),re.M));print('listed',count,flush=True)
  if not count:sys.exit('empty filter')
