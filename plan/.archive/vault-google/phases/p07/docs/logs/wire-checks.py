import pathlib,re,subprocess,sys
logs=pathlib.Path(__file__).parent
checks=[('wire-list01',['cargo','test','--locked','--offline','--lib','integration_service_contract_','--','--list']),('wire-run01',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','integration_service_contract_'])]
for label,cmd in checks:
 result=subprocess.run([sys.executable,str(logs/'check.py'),label]+cmd)
 if result.returncode:sys.exit(result.returncode)
 if '-list' in label:
  count=len(re.findall(r': test$',(logs/(label+'.log')).read_text(),re.M));print('listed',count,flush=True)
  if not count:sys.exit('empty filter')
