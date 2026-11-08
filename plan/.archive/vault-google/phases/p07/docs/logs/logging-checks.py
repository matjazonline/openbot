import pathlib,re,subprocess,sys
logs=pathlib.Path(__file__).parent
for family in ['integration_service_contract_','integration_gateway_','security_trace_','security_session_metrics_export_']:
 label='logging-'+family.rstrip('_')
 cargo=['cargo','test','--locked','--offline','--lib',family]
 for suffix,args in [('list',cargo+['--','--list']),('run',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=4','bash','scripts/test-network-isolation.sh']+cargo)]:
  result=subprocess.run([sys.executable,str(logs/'check.py'),label+'-'+suffix+'01']+args)
  if result.returncode:sys.exit(result.returncode)
  if suffix=='list':
   count=len(re.findall(r': test$',(logs/(label+'-'+suffix+'01.log')).read_text(),re.M));print(family,'count',count,flush=True)
   if not count:sys.exit('empty filter')
