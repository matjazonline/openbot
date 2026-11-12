import type { PluginAgentToolContentPart, PluginAgentToolResult } from '@get-bb/plugin-sdk';
import { RecoveryRequired } from './journal.js';
import type { McpResponse } from './mcp-transport.js';

export function mcpResult(response:McpResponse,origin:string,company:string):PluginAgentToolResult {
  const url=new URL('/ui/workflow-runs',origin);url.searchParams.set('company_id',company);url.searchParams.set('run_id',response.run_id);
  const metadata:Record<string,unknown>={call_id:response.call_id,run_url:url.href,outcome:response.kind};
  const content:PluginAgentToolContentPart[]=[];
  let toolError=false;
  if(response.kind==='completed' || response.kind==='tool_error') {
    const result=response.result;
    if(!result || typeof result!=='object' || !('content' in result) || !Array.isArray(result.content) || result.content.length>256)throw new RecoveryRequired();
    const extra:unknown[]=[];
    if('isError' in result)toolError=result.isError===true;
    for(const [index,part] of result.content.entries()) {
      if(!part || typeof part!=='object')throw new RecoveryRequired();
      if(part.type==='text' && typeof part.text==='string')content.push({type:'text',text:part.text});
      else if(part.type==='image' && typeof part.data==='string' && typeof part.mimeType==='string')content.push({type:'image',data:part.data,mimeType:part.mimeType});
      else {
        const copy={...part};
        // The public SDK supports text/images. Other MCP media stay explicit inert
        // descriptors; typed binary bytes are never put into model text.
        if(copy.type==='audio'){delete copy.data;copy.native_disposition='audio_bytes_omitted';}
        if(copy.type==='resource' && copy.resource && typeof copy.resource==='object' && 'blob' in copy.resource) {
          copy.resource={...copy.resource};delete copy.resource.blob;copy.native_disposition='resource_blob_omitted';
        }
        extra.push(copy);
      }
      if(part.annotations!==undefined || part._meta!==undefined)extra.push({type:'content_metadata',index,annotations:part.annotations,_meta:part._meta});
    }
    if('structuredContent' in result)metadata.structuredContent=result.structuredContent;
    if('_meta' in result)metadata._meta=result._meta;
    if(extra.length)metadata.additional_content=extra;
  }
  if(response.kind==='failed')metadata.reason=response.code;
  content.push({type:'text',text:JSON.stringify(metadata)});
  return {content,isError:toolError || !['completed','pending'].includes(response.kind)};
}
