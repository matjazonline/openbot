import test from 'node:test';
import assert from 'node:assert/strict';
import { mcpArguments } from '../src/mcp-arguments.js';
import { RecoveryRequired } from '../src/journal.js';

test('native MCP arguments bound hostile objects before cloning or durable intent and preserve valid Unicode',()=>{
  let invoked=false;const getter=Object.defineProperty({},'value',{enumerable:true,get(){invoked=true;return 'hidden';}});
  const sparse:unknown[]=[];sparse.length=2**32-1;
  for(const input of [null,[],getter,new Date(),{value:Infinity},{value:()=>0},{value:sparse},{value:'x'.repeat(65536)},
    {value:'embedded\0nul'},{'nul\0key':true},{value:'\ud800'},{'\udfff':true}]) {
    assert.throws(()=>mcpArguments(input),RecoveryRequired);
  }
  assert.equal(invoked,false,'validation must never execute a model-controlled getter');
  const input=JSON.parse('{"__proto__":{"polluted":true},"unicode":"🌱 é","literal":"\\\\u0000"}');
  const result=mcpArguments(input);
  assert.deepEqual(result.arguments,input);assert.equal(Object.hasOwn(result.arguments,'__proto__'),true);
  assert.equal(({} as {polluted?:boolean}).polluted,undefined);
  assert.equal(mcpArguments({value:'x'.repeat(65000)}).arguments.value,'x'.repeat(65000));
});
