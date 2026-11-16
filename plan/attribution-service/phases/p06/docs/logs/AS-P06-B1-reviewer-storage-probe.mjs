import vm from 'node:vm';
import fs from 'node:fs';
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
const source=fs.readFileSync('services/attribution/assets/sdk-1.0.0.js','utf8');
const values=new Map(), installation=randomUUID(), clickA=randomUUID(), clickB=randomUUID();
const prefix=`ob-attribution:1:${installation}:click:`;
let armed=false, readA=false, fired=false, tabB;
function tab(click,withHook=false) {
 const localStorage={
  get length(){if(withHook && armed && readA && !fired){fired=true;tabB.setConsent(true);assert.ok(values.has(prefix+clickB));console.log('Tab B completed consent/write after tab A read and before tab A cleanup.');}return values.size;},
  key(i){return [...values.keys()][i]??null;},
  getItem(k){if(withHook && armed && k===prefix+clickA)readA=true;return values.get(k)??null;},
  setItem(k,v){values.set(k,String(v));},removeItem(k){values.delete(k);}
 };
 const window={addEventListener(){}};
 const context=vm.createContext({window,document:{currentScript:{src:'https://attribution.example/sdk/1.0.0.js'}},location:{href:'https://shop.example/'+(click?'?ob_click='+click:'')},history:{state:null,replaceState(){}},localStorage,URL,AbortController,crypto:{randomUUID},setTimeout,clearTimeout,fetch:async()=>({status:204})});
 vm.runInContext(source,context);return window.OpenbotAttribution.create(installation);
}
const tabA=tab(clickA,true);tabA.setConsent(true);tabB=tab(clickB);armed=true;
const a=tabA.handoff();
console.log('A handoff candidates:',a.candidate_click_ids.length);
console.log('B candidate persisted after A cleanup:',values.has(prefix+clickB));
assert.ok(fired);assert.equal(values.has(prefix+clickB),false);
const returned=tab(null);returned.setConsent(true);
console.log('Fresh return-visit handoff includes B:',returned.handoff().candidate_click_ids.includes(clickB));
assert.equal(returned.handoff().candidate_click_ids.includes(clickB),false);
console.log('REPRODUCED: concurrent new candidate is deleted despite only 2 candidates and no expiry. VM uses actual SDK; storage calls inject a legal cross-tab interleaving, not a real browser scheduling test.');
