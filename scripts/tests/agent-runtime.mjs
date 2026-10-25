import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { test } from 'node:test';
const source = readFileSync(new URL('../../src/adapters/http/pages/agent_library_multi_select.rs', import.meta.url), 'utf8');
const script = source.split('AGENT_LIBRARY_SCRIPT: &str = r#"')[1].split('"#;')[0];
function context() {
    const state = { FormData: class { constructor(values) { this.values=values; } get(key) { return this.values[key]??null; } }, alert() {} };
    vm.createContext(state); vm.runInContext(script, state); return state;
}
test('library payload carries the agent\'s own fields and no retired runtime fields', () => {
    const ctx=context();
    const draft={name:'Agent',slug:'agent',description:'Answers billing',skill_ids:'a, b',harness_kind:'rig',granted_tool_ids:'calculator',response_format:'json_schema',response_schema:'{}',config_json:'{"version":1}',run_timeout_secs:'90'};
    const result=ctx.libraryPayload(draft);
    for (const retired of ['harness_kind','granted_tool_ids','native_tool_policy','config_json','run_timeout_secs','response_contract','sub_agent_ids']) assert.equal(retired in result,false,retired);
    assert.equal(result.name,'Agent');
    assert.equal(result.description,'Answers billing');
    assert.deepEqual(Array.from(result.skill_ids),['a','b']);
});
test('accepted library writes show progress, prevent duplicate submits, and restore on failure', async () => {
    const ctx=context(); let finish; let requests=0;
    ctx.fetch=()=>{requests++;return new Promise(resolve=>{finish=resolve;});};
    const button={textContent:'Save',dataset:{},disabled:false}; const attrs={};
    const form={name:'Agent',response_format:'text',getAttribute(key){return attrs[key]??null;},setAttribute(key,value){attrs[key]=value;},removeAttribute(key){delete attrs[key];},querySelectorAll(){return [button];}};
    const pending=ctx.submitLibraryAgent(form,'/api/agent-library','POST');
    assert.equal(attrs['aria-busy'],'true');assert.equal(button.disabled,true);assert.equal(button.textContent,'Saving…');
    await ctx.submitLibraryAgent(form,'/api/agent-library','POST'); assert.equal(requests,1);
    finish({ok:false,text:async()=> 'Invalid config'});await pending;
    assert.equal(attrs['aria-busy'],undefined);assert.equal(button.disabled,false);assert.equal(button.textContent,'Save');
});
