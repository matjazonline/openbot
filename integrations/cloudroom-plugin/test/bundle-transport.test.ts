import test from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { fetchNativeBundle } from '../src/bundle-transport.js';
import { MAX_NATIVE_BUNDLE_BYTES } from '../src/native-bundle.js';
import { fixture } from './support.js';
import { server, waitFor } from './connection-support.js';
import type { Identity } from '../src/protocol.js';

const operation='00000000-0000-4000-8000-000000000001';
const etag=(bytes:Buffer)=>`"${createHash('sha256').update(bytes).digest('hex')}"`;

test('bundle transport uses trusted TLS and live identity with exact maximum streaming bytes',async()=>{
  const s=await server();const f=fixture();f.binding.origin=s.origin;
  const paired={state:'paired' as const,version:1 as const,binding:f.binding,credential:'a'.repeat(64)};
  const identity:Identity={installation_id:f.binding.installation_id,credential_generation:2,connection_generation:9};
  let bytes=Buffer.from('exact bytes');const failures:unknown[]=[];
  s.https.on('request',(request,response)=>{
    try{
      assert.equal(request.method,'GET');assert.equal(request.url,`/v1/local-machines/operations/${operation}/bundle`);
      assert.equal(request.headers.authorization,`Bearer ${paired.credential}`);
      assert.equal(request.headers['x-openbot-installation-id'],identity.installation_id);
      assert.equal(request.headers['x-openbot-protocol-version'],'2');
      assert.equal(request.headers['x-openbot-credential-generation'],'2');assert.equal(request.headers['x-openbot-connection-generation'],'9');
    }catch(error){failures.push(error);}
    response.setHeader('ETag',etag(bytes));response.write(bytes.subarray(0,5));response.end(bytes.subarray(5));
  });
  try{
    assert.deepEqual(await fetchNativeBundle(paired,identity,operation,new AbortController().signal),bytes);
    bytes=Buffer.alloc(MAX_NATIVE_BUNDLE_BYTES,255);
    assert.deepEqual(await fetchNativeBundle(paired,identity,operation,new AbortController().signal),bytes);
    bytes=Buffer.alloc(MAX_NATIVE_BUNDLE_BYTES+1);
    await assert.rejects(fetchNativeBundle(paired,identity,operation,new AbortController().signal),/recovery/i);
    assert.equal(failures.length,0,String(failures[0]));
    await assert.rejects(fetchNativeBundle({...paired,binding:{...paired.binding,origin:s.origin.replace('localhost','127.0.0.1')}},identity,operation,new AbortController().signal));
  }finally{s.https.closeAllConnections();await s.close();f.close();}
});

test('bundle transport refuses redirects, revoked authority, corrupt digests and oversized declared bodies',async()=>{
  const s=await server();const f=fixture();f.binding.origin=s.origin;
  const paired={state:'paired' as const,version:1 as const,binding:f.binding,credential:'a'.repeat(64)};
  const identity:Identity={installation_id:f.binding.installation_id,credential_generation:1,connection_generation:1};
  let mode='redirect';let redirected=0;
  s.https.on('request',(request,response)=>{
    if(request.url==='/redirected'){redirected++;response.end('forbidden redirect');return;}
    if(mode==='redirect'){response.writeHead(302,{location:'/redirected'});response.end();}
    else if(mode==='revoked'){response.writeHead(403);response.end();}
    else if(mode==='304'){response.writeHead(304);response.end();}
    else if(mode==='digest'){response.setHeader('etag',`"${'0'.repeat(64)}"`);response.end('bytes');}
    else{response.setHeader('content-length',String(MAX_NATIVE_BUNDLE_BYTES+1));response.end();}
  });
  try{
    for(mode of ['redirect','revoked','304','digest','length'])await assert.rejects(fetchNativeBundle(paired,identity,operation,new AbortController().signal));
    assert.equal(redirected,0);
    await assert.rejects(fetchNativeBundle({...paired,binding:{...paired.binding,origin:'http://localhost'}},identity,operation,new AbortController().signal));
  }finally{s.https.closeAllConnections();await s.close();f.close();}
});

test('bundle body cancellation closes the active request and joins promptly',async()=>{
  const s=await server();const f=fixture();f.binding.origin=s.origin;
  const paired={state:'paired' as const,version:1 as const,binding:f.binding,credential:'a'.repeat(64)};
  const identity:Identity={installation_id:f.binding.installation_id,credential_generation:1,connection_generation:1};
  let started=false;let closed=false;
  s.https.on('request',(_request,response)=>{started=true;response.writeHead(200,{etag:etag(Buffer.from('unfinished'))});response.write('un');response.on('close',()=>{closed=true;});});
  const stop=new AbortController();const pending=fetchNativeBundle(paired,identity,operation,stop.signal);const refused=assert.rejects(pending);
  try{await waitFor(()=>started);stop.abort();await refused;await waitFor(()=>closed);}
  finally{stop.abort();s.https.closeAllConnections();await s.close();f.close();}
});
