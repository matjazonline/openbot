import pathlib,re,subprocess,sys
logs=pathlib.Path(__file__).parent
fixture=logs/'vault-browser-09'; fixture.mkdir(exist_ok=True)
families=['integration_portability_','integration_service_contract_','vault_google_e2e_','vault_authoring_picker_is_owner_scoped_and_only_exposes_approved_metadata','vault_authoring_owner_forms_and_reconnect_states_emit_real_browser_markup']
failures=[]
for index,family in enumerate(families):
 label=f'wave09-{index+1}'
 cargo=['cargo','test','--locked','--offline','--lib',family]
 for suffix,args in [('list',cargo+['--','--list']),('run',['env','RUST_MIN_STACK=2097152','RUST_TEST_THREADS=1',f'VAULT_AUTHORING_BROWSER_FIXTURES={fixture.resolve()}','bash','scripts/test-network-isolation.sh']+cargo)]:
  name=label+'-'+suffix
  code=subprocess.run([sys.executable,str(logs/'check.py'),name]+args).returncode
  if code: failures.append((family,suffix,code)); break
  if suffix=='list':
   count=len(re.findall(r': test$',(logs/(name+'.log')).read_text(),re.M)); print(family,count,flush=True)
   if not count:failures.append((family,suffix,'empty'));break
print('failed',failures,flush=True)
sys.exit(bool(failures))
