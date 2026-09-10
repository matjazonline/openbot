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
test('library payload preserves explicit Rig and shared JSON schema without a default ai-agents fallback', () => {
    const ctx=context();
    const draft={name:'Agent',slug:'agent',harness_kind:'rig',response_format:'json_schema',response_schema:'{"const":"</textarea>"}',config_json:'{"version":1,"max_turns":4}'};
    const result=ctx.libraryPayload(draft);
    assert.equal(result.harness_kind,'rig');
    assert.equal(result.config_json.max_turns,4);
    assert.equal(result.response_contract.schema.const,'</textarea>');
    assert.equal(draft.response_schema,'{"const":"</textarea>"}');
    assert.equal(ctx.libraryPayload({}).harness_kind,null);
    assert.equal(ctx.libraryPayload({response_format:'text',response_schema:'{bad'}).response_contract,null);
});
test('invalid replacement schema never reaches the library API and leaves the form intact', async () => {
    const ctx=context(); let requests=0;
    ctx.fetch=async()=>{requests++;return {ok:true,status:204}};
    const form={response_format:'json_schema',response_schema:'{bad',getAttribute(){return null;}};
    await ctx.submitLibraryAgent(form,'/api/agent-library','POST');
    assert.equal(requests,0); assert.equal(form.response_schema,'{bad');
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
