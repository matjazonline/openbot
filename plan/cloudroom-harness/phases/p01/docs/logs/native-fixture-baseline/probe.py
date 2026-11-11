from pathlib import Path
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import hashlib,json,subprocess,threading,os,time,signal
root=Path(__file__).parent
(root/'home').mkdir(exist_ok=True)
workspace=root/'workspace';workspace.mkdir(exist_ok=True); (workspace/'scripts').mkdir(exist_ok=True);(workspace/'assets').mkdir(exist_ok=True)
asset=bytes(range(256));(workspace/'assets/data.bin').write_bytes(asset)
script=workspace/'scripts/probe.sh';script.write_text('#!/bin/sh\nset -eu\ncd -- "$(dirname -- "$0")"\n/usr/bin/shasum -a 256 ../assets/data.bin\nprintf "OPENBOT_REAL_NATIVE_SCRIPT_OK\\n"\n');script.chmod(0o755)
marker='OPENBOT_LONG_INSTRUCTION_TAIL_e9c04b';observations=[]
class Handler(BaseHTTPRequestHandler):
 def log_message(self,*args): pass
 def do_POST(self):
  body=json.loads(self.rfile.read(int(self.headers.get('content-length','0'))))
  if 'count_tokens' in self.path:
   data=json.dumps({'input_tokens':100}).encode();self.send_response(200);self.send_header('content-type','application/json');self.send_header('content-length',str(len(data)));self.end_headers();self.wfile.write(data);return
  system=json.dumps(body.get('system',''));messages=json.dumps(body.get('messages',[]));done='OPENBOT_REAL_NATIVE_SCRIPT_OK' in messages
  observations.append({'path':self.path,'instruction_tail_received':marker in system,'script_result_received':done,'tools':[x.get('name') for x in body.get('tools',[])]})
  if done: block={'type':'text','text':'NATIVE_FIXTURE_COMPLETE'};delta={'type':'text_delta','text':'NATIVE_FIXTURE_COMPLETE'};reason='end_turn'
  else: block={'type':'tool_use','id':'toolu_openbot_fixture_01','name':'Bash','input':{}};delta={'type':'input_json_delta','partial_json':json.dumps({'command':'./scripts/probe.sh','description':'Run disposable Openbot fixture script','timeout':10000})};reason='tool_use'
  message={'id':'msg_openbot_fixture_'+str(len(observations)),'type':'message','role':'assistant','content':[],'model':body.get('model','claude-sonnet-4-6'),'stop_reason':None,'stop_sequence':None,'usage':{'input_tokens':100,'output_tokens':0}}
  events=[('message_start',{'type':'message_start','message':message}),('content_block_start',{'type':'content_block_start','index':0,'content_block':block}),('content_block_delta',{'type':'content_block_delta','index':0,'delta':delta}),('content_block_stop',{'type':'content_block_stop','index':0}),('message_delta',{'type':'message_delta','delta':{'stop_reason':reason,'stop_sequence':None},'usage':{'output_tokens':40}}),('message_stop',{'type':'message_stop'})]
  data=''.join('event: '+event+'\ndata: '+json.dumps(payload)+'\n\n' for event,payload in events).encode()
  self.send_response(200);self.send_header('content-type','text/event-stream');self.send_header('content-length',str(len(data)));self.end_headers();self.wfile.write(data)
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);thread=threading.Thread(target=server.serve_forever,daemon=True);thread.start()
env={'HOME':str(root/'home'),'PATH':'/opt/homebrew/bin:/usr/bin:/bin','CLAUDE_CONFIG_DIR':str(root/'home/.claude'),'ANTHROPIC_API_KEY':'sk-ant-openbot-disposable-fixture-only','ANTHROPIC_BASE_URL':'http://127.0.0.1:'+str(server.server_port),'DISABLE_TELEMETRY':'1','DISABLE_ERROR_REPORTING':'1','CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC':'1','NO_COLOR':'1'}
args=['/opt/homebrew/bin/claude','--print','--verbose','--output-format','stream-json','--permission-mode','bypassPermissions','--setting-sources','','--strict-mcp-config','--mcp-config','{"mcpServers":{}}','--no-session-persistence','--tools','Bash','--system-prompt','Disposable fixture. '+('Required instructions. '*400)+marker,'Execute the configured disposable script and report its result.']
p=subprocess.Popen(args,cwd=workspace,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,start_new_session=True)
try:
 try:out,err=p.communicate(timeout=45)
 except subprocess.TimeoutExpired:
  os.killpg(p.pid,signal.SIGTERM)
  try:out,err=p.communicate(timeout=5)
  except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);out,err=p.communicate()
 (root/'native-stdout.jsonl').write_text(out);(root/'native-stderr.txt').write_text(err)
 passed=p.returncode==0 and any(o['instruction_tail_received'] for o in observations) and any(o['script_result_received'] for o in observations) and hashlib.sha256(asset).hexdigest() in out
 result={'status':'PASS' if passed else 'FAIL','exit':p.returncode,'observations':observations,'asset_sha256':hashlib.sha256(asset).hexdigest(),'script_mode':oct(script.stat().st_mode&0o777),'limitations':'Standalone real Claude harness with scripted local API; no Cloudroom staging, Openbot integration or reasoning proved.'}
 (root/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
finally:server.shutdown();server.server_close();thread.join(timeout=2)
