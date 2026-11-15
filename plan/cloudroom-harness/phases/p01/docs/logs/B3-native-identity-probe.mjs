import {createRequire} from 'node:module';
import {createServer} from 'node:http';
import {mkdir,mkdtemp,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {once} from 'node:events';
const require=createRequire('/private/tmp/openbot-cloudroom-harness-t1gfnhz1/cloudroom/gui/plugins/provider-claude-code/package.json');
const {query}=await import(require.resolve('@anthropic-ai/claude-agent-sdk'));
const {McpServer}=await import(require.resolve('@modelcontextprotocol/sdk/server/mcp.js'));
const {CallToolRequestSchema,ListToolsRequestSchema}=await import(require.resolve('@modelcontextprotocol/sdk/types.js'));
const root=await mkdtemp(path.join(tmpdir(),'openbot-mcp-identity-'));
const observed=[];
let releaseInput;const inputDone=new Promise(resolve=>{releaseInput=resolve;});
const promptUuid='81a8723a-4509-4ac1-8ad8-9a81c0dcf7e5';
const instance=new McpServer({name:'identity',version:'1.0.0'},{capabilities:{tools:{}}});
instance.server.setRequestHandler(ListToolsRequestSchema,()=>({tools:[{name:'lookup',description:'Disposable identity probe',inputSchema:{type:'object',properties:{},additionalProperties:false}}]}));
instance.server.setRequestHandler(CallToolRequestSchema,async(request,extra)=>{
 observed.push({kind:'mcp_request',params:request.params,extraKeys:Object.keys(extra),requestId:extra.requestId,meta:extra._meta});
 return {content:[{type:'text',text:'IDENTITY_DONE'}]};
});
const api=createServer(async(req,res)=>{
 let raw='';for await(const chunk of req){raw+=chunk;if(Buffer.byteLength(raw)>4*1024*1024){res.writeHead(413).end();return;}}
 if(req.url?.includes('count_tokens')){res.writeHead(200,{'content-type':'application/json'}).end('{"input_tokens":100}');return;}
 const body=JSON.parse(raw||'{}');const done=JSON.stringify(body.messages??[]).includes('IDENTITY_DONE');
 const tool=body.tools?.find(t=>t.name==='mcp__identity__lookup');
 const use=tool&&!done;observed.push({kind:'api',hasTool:!!tool,done});
 const message={id:'msg_identity',type:'message',role:'assistant',content:[],model:body.model,stop_reason:null,stop_sequence:null,usage:{input_tokens:100,output_tokens:0}};
 const block=use?{type:'tool_use',id:'toolu_identity_stable_1',name:tool.name,input:{}}:{type:'text',text:''};
 const delta=use?{type:'input_json_delta',partial_json:'{}'}:{type:'text_delta',text:'IDENTITY_PROBE_COMPLETE'};
 const events=[['message_start',{type:'message_start',message}],['content_block_start',{type:'content_block_start',index:0,content_block:block}],['content_block_delta',{type:'content_block_delta',index:0,delta}],['content_block_stop',{type:'content_block_stop',index:0}],['message_delta',{type:'message_delta',delta:{stop_reason:use?'tool_use':'end_turn',stop_sequence:null},usage:{output_tokens:20}}],['message_stop',{type:'message_stop'}]];
 res.writeHead(200,{'content-type':'text/event-stream'}).end(events.map(([event,payload])=>`event: ${event}\ndata: ${JSON.stringify(payload)}\n\n`).join(''));
});api.listen(0,'127.0.0.1');await once(api,'listening');
const home=path.join(root,'home');await mkdir(home);const controller=new AbortController();const timer=setTimeout(()=>controller.abort(),45000);
let failure;
try{
 const session=query({prompt:(async function*(){yield {type:'user',uuid:promptUuid,session_id:'',parent_tool_use_id:null,message:{role:'user',content:'Call the identity lookup once.'}};await inputDone;})(),options:{cwd:root,model:'claude-sonnet-4-6',abortController:controller,permissionMode:'bypassPermissions',allowDangerouslySkipPermissions:true,settingSources:[],persistSession:false,
 env:{HOME:home,CLAUDE_CONFIG_DIR:path.join(home,'.claude'),PATH:process.env.PATH,ANTHROPIC_API_KEY:'sk-ant-openbot-disposable-fixture-only',ANTHROPIC_BASE_URL:`http://127.0.0.1:${api.address().port}`,CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC:'1',DISABLE_TELEMETRY:'1',DISABLE_ERROR_REPORTING:'1'},
 hooks:{UserPromptSubmit:[{hooks:[async(input)=>{observed.push({kind:'user_prompt_submit',promptId:input.prompt_id,matchesSubmittedText:input.prompt==='Call the identity lookup once.'});return {continue:true};}]}],PreToolUse:[{hooks:[async(input)=>{observed.push({kind:'pre_tool_use',toolUseId:input.tool_use_id,promptId:input.prompt_id,expectedPrompt:promptUuid,toolName:input.tool_name});return {continue:true};}]}]},mcpServers:{identity:{type:'sdk',name:'identity',instance}},allowedTools:['mcp__identity__lookup'],tools:[]}});
 for await(const item of session){if(item.type==='result'){observed.push({kind:'result',subtype:item.subtype,isError:item.is_error});releaseInput();}}
}catch(error){failure=String(error);}
finally{releaseInput();clearTimeout(timer);api.closeAllConnections();await new Promise(resolve=>api.close(resolve));await rm(root,{recursive:true,force:true});}
const result={observed,failure,limitations:'Owned isolated native SDK/MCP metadata probe with scripted local Messages, no remote credentials or paid inference.'};
await writeFile('/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree/plan/cloudroom-harness/phases/p01/docs/logs/B3-native-identity-probe-R3.json',JSON.stringify(result,null,2)+'\n');
console.log(JSON.stringify(result,null,2));
if(failure||!observed.some(x=>x.kind==='mcp_request'))process.exitCode=1;
