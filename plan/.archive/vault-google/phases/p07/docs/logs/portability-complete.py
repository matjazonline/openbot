import json,pathlib,re,subprocess,sys
logs=pathlib.Path(__file__).parent
for family in ['integration_portability_','integration_renewal_','integration_service_contract_']:
 label='portability-'+family.rstrip('_')
 cargo=['cargo','test','--locked','--offline','--lib',family]
 for suffix,args in [('list',cargo+['--','--list']),('run',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh']+cargo)]:
  check_label=label+'-'+suffix+'02'
  receipt=logs/(check_label+'.json')
  if receipt.exists() and json.loads(receipt.read_text())['exit']==0:
   print('reuse unchanged-source PASS',check_label,flush=True)
  else:
   result=subprocess.run([sys.executable,str(logs/'check.py'),check_label]+args)
   if result.returncode:sys.exit(result.returncode)
  if suffix=='list':
   count=len(re.findall(r': test$',(logs/(check_label+'.log')).read_text(),re.M));print(family,'count',count,flush=True)
   if not count:sys.exit('empty filter')
