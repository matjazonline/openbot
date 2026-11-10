from pathlib import Path
import json,os,subprocess,time
root=Path(__file__).resolve().parents[4];pub=Path(__file__).resolve().parent
name='adapters::mcp::tests::mcp_dropped_call_sends_protocol_cancellation_and_joins_cleanup'
env=os.environ|{'RUST_MIN_STACK':'16777216','CARGO_NET_OFFLINE':'true','NO_PROXY':'*','no_proxy':'*'}
for key in ['HTTP_PROXY','HTTPS_PROXY','ALL_PROXY','http_proxy','https_proxy','all_proxy']:env.pop(key,None)
profile='(version 1)(allow default)(deny network*)(allow network-bind (local ip "localhost:*"))(allow network-inbound (local ip "localhost:*"))(allow network-outbound (remote ip "localhost:*"))'
results=[]
for label,binary in [('accepted-parent','open_bots-50aeda0c9f5dbc3a'),('combined','open_bots-b3fb49a4c0332dfc')]:
 for trial in range(1,7):
  command=['sandbox-exec','-p',profile,'/Users/mac03/dev/openbot/target/debug/deps/'+binary,'--exact',name,'--nocapture'];file=f'logs/mcp-{label}-repeat-{trial:02}.log';start=time.monotonic()
  with (pub/file).open('wb') as f:r=subprocess.run(command,cwd=root,env=env,stdout=f,stderr=subprocess.STDOUT)
  result={'binary':binary,'baseline':label,'trial':trial,'exit':r.returncode,'seconds':round(time.monotonic()-start,2),'log':file};results.append(result);print(json.dumps(result),flush=True);(pub/'mcp-repeat-results.json').write_text(json.dumps(results,indent=2)+'\n')
