const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync('src/adapters/http/pages/mailbox.rs','utf8');
const script=source.match(/const BUTTON_BUSY_SCRIPT: &str = r##"([\s\S]*?)"##;/)[1];
const listeners=new Map();let spinner=null;const attributes=new Map();
const button={disabled:false,textContent:'Save credential',matches:()=>true,
 querySelector:selector=>selector==='[data-request-spinner]'?spinner:null,
 prepend:element=>{spinner=element;},setAttribute:(k,v)=>attributes.set(k,v),removeAttribute:k=>attributes.delete(k)};
const form={matches:selector=>selector==='form',contains:elt=>elt===button,querySelector:()=>button};
const document={body:{addEventListener:(name,fn)=>listeners.set(name,fn)},createElement:()=>({className:'',setAttribute(){},remove(){spinner=null;}})};
vm.runInNewContext(script,{document,Map});
const event={detail:{elt:form,requestConfig:{triggeringEvent:{submitter:button}},successful:false}};
for(let attempt=0;attempt<2;attempt++) {
 listeners.get('htmx:beforeRequest')(event);
 assert.equal(button.disabled,true);assert.ok(spinner);assert.equal(attributes.get('aria-busy'),'true');
 listeners.get('htmx:afterRequest')(event);
 assert.equal(button.disabled,false);assert.equal(spinner,null);assert.equal(attributes.has('aria-busy'),false);
}
console.log('PASS: actual shared HTMX button script shows progress, restores failed requests, and accepts retry');
