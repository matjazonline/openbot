import { createHash, randomUUID } from 'node:crypto';
import { constants } from 'node:fs';
import * as fs from 'node:fs/promises';
import path from 'node:path';
import { parse as parseYaml, stringify as stringifyYaml } from 'yaml';
import { z } from 'zod';
import { Journal, JournalCapacity, RecoveryRequired } from './journal.js';
import { permissions, type Harness } from './protocol.js';

export const NATIVE_CACHE_LIMITS = Object.freeze({ bytes: 64 * 1024 * 1024, bundles: 128, concurrent: 8 });
export const MAX_NATIVE_BUNDLE_BYTES = 2 * 1024 * 1024;
const bytes = (limit: number) => (value: string) => Buffer.byteLength(value) <= limit;
const fileSchema = z.object({ path: z.string().max(240), content_base64: z.string().max(4 * Math.ceil(256 * 1024 / 3)), executable: z.boolean() }).strict();
const toolSchema=z.object({binding_id:z.string().uuid(),alias:z.string().regex(/^[A-Za-z0-9_.:-]{1,128}$/),
  contract_base64:z.string().max(4*Math.ceil(65536/3))}).strict();
export const nativeBundleSchema = z.object({ format: z.literal('openbot_native_v2'), instructions: z.string().max(16384).refine(bytes(16384)), permissions,
  skills: z.array(z.object({ name: z.string().regex(/^[A-Za-z0-9_.:-]{1,128}$/), entry: z.string().max(20*1024).refine(bytes(20*1024)), files: z.array(fileSchema).max(64) }).strict()).max(16),
  tools:z.array(toolSchema).max(32) }).strict();
export type NativeBundle = z.infer<typeof nativeBundleSchema>;
export type NativeReference = Extract<Harness,{kind:'agent'}>;
export type BundleReader = (operationId: string, signal: AbortSignal) => Promise<Buffer>;
type Entry = { path: string; bytes: Buffer; mode: number };
type SkillTree = { name: string; hash: string; files: Entry[] };
type Plan = { bundle: NativeBundle; instructions: string; trees: SkillTree[]; files: Map<string,Entry>; directories: Set<string>; cost: number };
export type RequiredConfiguration = { instructions: string; skillDirectories: Array<{rootPath:string;expectedTreeHash:string}> };
const hex = (bytes: Buffer | string) => createHash('sha256').update(bytes).digest('hex');

export function nativeMcpContract(tool:NativeBundle['tools'][number]):string {
  const raw=Buffer.from(tool.contract_base64,'base64');
  if(raw.length>65536 || raw.toString('base64')!==tool.contract_base64) throw new RecoveryRequired();
  const text=new TextDecoder('utf-8',{fatal:true}).decode(raw);
  const contract:unknown=JSON.parse(text);
  if(!contract || typeof contract!=='object' || !('binding' in contract) || !contract.binding || typeof contract.binding!=='object') throw new RecoveryRequired();
  const binding=contract.binding;
  if(!('id' in binding) || binding.id!==tool.binding_id || !('alias' in binding) || binding.alias!==tool.alias || !('source' in binding)
      || !binding.source || typeof binding.source!=='object' || !('kind' in binding.source) || binding.source.kind!=='mcp') throw new RecoveryRequired();
  // Return original canonical JSON as text: parsing a schema into JS and
  // serializing it again can change large numeric schema literals.
  return text;
}

function validateTools(bundle:NativeBundle):void {
  const bindings=new Set<string>();const aliases=new Set<string>();
  for(const tool of bundle.tools) {
    if(bindings.has(tool.binding_id) || aliases.has(tool.alias)) throw new RecoveryRequired();
    bindings.add(tool.binding_id);aliases.add(tool.alias);nativeMcpContract(tool);
  }
}

export function hashNativeTree(files: readonly Entry[]): string {
  const hash=createHash('sha256').update('bb-skill-tree-v1');
  for(const file of [...files].sort((a,b)=>a.path<b.path?-1:a.path>b.path?1:0)) hash.update('\0file\0').update(file.path).update('\0').update(file.mode.toString(8)).update('\0').update(String(file.bytes.length)).update('\0').update(file.bytes);
  return hash.digest('hex');
}
function assetPath(value:string):void {
  if(!value || !/^[A-Za-z0-9._/ -]+$/.test(value) || Buffer.byteLength(value)>240 || value.split('/').some(part=>!part || part==='.' || part==='..' || part.startsWith(' ') || /[. ]$/.test(part)) || value.split('/')[0]!.toLowerCase()==='skill.md') throw new RecoveryRequired();
}
function renamedEntry(skill: NativeBundle['skills'][number], name:string):Buffer {
  const match=/^\uFEFF?---\r?\n([\s\S]*?)\r?\n---(?=\r?\n|$)/.exec(skill.entry);
  if(!match) throw new RecoveryRequired();
  const metadata:unknown=parseYaml(match[1]!,{uniqueKeys:true,maxAliasCount:100});
  if(!metadata || typeof metadata!=='object' || Array.isArray(metadata) || !('name' in metadata) || metadata.name!==skill.name || !('description' in metadata) || typeof metadata.description!=='string') throw new RecoveryRequired();
  const result=Buffer.from(`---\n${stringifyYaml({...metadata,name})}---${skill.entry.slice(match[0].length)}`);
  if(result.length>20*1024) throw new RecoveryRequired();
  return result;
}
function plan(raw:Buffer, digest:string):Plan {
  if(raw.length>MAX_NATIVE_BUNDLE_BYTES || hex(raw)!==digest) throw new RecoveryRequired();
  const bundle=nativeBundleSchema.parse(JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(raw)));
  if(JSON.stringify(bundle)!==raw.toString('utf8')) throw new RecoveryRequired();
  validateTools(bundle);
  const names=new Set<string>(); const trees:SkillTree[]=[];
  for(const skill of bundle.skills) {
    const slug=skill.name.toLowerCase().replace(/[^a-z0-9]+/g,'-').replace(/^-|-$/g,'').slice(0,28).replace(/-$/,'') || 'skill';
    const name=`ob-${slug}-${hex(JSON.stringify(skill)).slice(0,32)}`;
    if(names.has(name)) throw new RecoveryRequired(); names.add(name);
    const files:Entry[]=[{path:'SKILL.md',bytes:renamedEntry(skill,name),mode:0o644}]; let total=0;
    for(const file of skill.files) {
      assetPath(file.path); const buffer=Buffer.from(file.content_base64,'base64'); total+=buffer.length;
      if(buffer.toString('base64')!==file.content_base64 || buffer.length>256*1024 || total>512*1024) throw new RecoveryRequired();
      files.push({path:file.path,bytes:buffer,mode:file.executable?0o755:0o644});
    }
    const paths=files.map(file=>file.path.toLowerCase()).sort();
    if(paths.some((value,index)=>index>0 && (value===paths[index-1] || value.startsWith(`${paths[index-1]}/`)))) throw new RecoveryRequired();
    const spellings=new Map<string,string>();
    for(const file of files) {
      let prefix='';
      for(const component of file.path.split('/')) {
        prefix=prefix?`${prefix}/${component}`:component;const folded=prefix.toLowerCase();
        if(spellings.has(folded) && spellings.get(folded)!==prefix) throw new RecoveryRequired();
        spellings.set(folded,prefix);
      }
    }
    trees.push({name,hash:hashNativeTree(files),files});
  }
  const files=new Map<string,Entry>([['bundle.json',{path:'bundle.json',bytes:raw,mode:0o644}]]); const directories=new Set(['','skills']);
  for(const tree of trees) for(const file of tree.files) {
    const relative=`skills/${tree.name}/${file.path}`; files.set(relative,{...file,path:relative});
    for(let parent=path.posix.dirname(relative);parent!=='.';parent=path.posix.dirname(parent)) directories.add(parent);
  }
  const instructions=bundle.instructions+(trees.length?'\n\nOpenbot skill names:\n'+trees.map((tree,index)=>`${bundle.skills[index]!.name} => ${tree.name}`).join('\n'):'');
  if(Buffer.byteLength(instructions)>32768) throw new RecoveryRequired();
  // Include allocation and metadata overhead, so empty deep paths cannot bypass the byte ceiling.
  const cost=4096*(files.size+directories.size)+[...files.values()].reduce((sum,file)=>sum+Math.ceil(file.bytes.length/4096)*4096,0);
  return {bundle,instructions,trees,files,directories,cost};
}
async function privateDirectory(directory:string):Promise<void> {
  const stat=await fs.lstat(directory);
  if(!stat.isDirectory() || stat.isSymbolicLink() || stat.uid!==process.getuid!() || (stat.mode&0o777)!==0o700) throw new RecoveryRequired();
}
async function exactFile(file:string, expected:Entry, signal:AbortSignal):Promise<void> {
  signal.throwIfAborted(); const handle=await fs.open(file,constants.O_RDONLY|constants.O_NOFOLLOW);
  try {
    const stat=await handle.stat();
    if(!stat.isFile() || stat.uid!==process.getuid!() || stat.size!==expected.bytes.length || (stat.mode&0o777)!==expected.mode) throw new RecoveryRequired();
    const buffer=Buffer.alloc(stat.size); let offset=0;
    while(offset<buffer.length) {signal.throwIfAborted();const {bytesRead}=await handle.read(buffer,offset,buffer.length-offset,null);if(!bytesRead) throw new RecoveryRequired();offset+=bytesRead;}
    if((await handle.read(Buffer.alloc(1),0,1,null)).bytesRead || !buffer.equals(expected.bytes)) throw new RecoveryRequired();
    const after=await handle.stat();if(after.size!==stat.size || after.mode!==stat.mode || after.ctimeMs!==stat.ctimeMs || after.mtimeMs!==stat.mtimeMs) throw new RecoveryRequired();
  } finally {await handle.close();}
}
async function validateDirectory(root:string, expected:Plan, signal:AbortSignal):Promise<void> {
  const pending=['']; const seen=new Set<string>();let count=0;
  while(pending.length) {
    const relative=pending.pop()!;const directory=path.join(root,relative);await privateDirectory(directory);
    const entries=await fs.opendir(directory);
    for await(const entry of entries) {
      signal.throwIfAborted();if(++count>expected.files.size+expected.directories.size) throw new RecoveryRequired();
      const child=relative?`${relative}/${entry.name}`:entry.name;
      if(expected.directories.has(child)) {pending.push(child);continue;}
      const file=expected.files.get(child);if(!file) throw new RecoveryRequired();
      await exactFile(path.join(root,child),file,signal);seen.add(child);
    }
  }
  if(seen.size!==expected.files.size) throw new RecoveryRequired();
}
async function boundedRaw(file:string):Promise<Buffer> {
  const handle=await fs.open(file,constants.O_RDONLY|constants.O_NOFOLLOW);
  try {const stat=await handle.stat();if(!stat.isFile() || stat.size>MAX_NATIVE_BUNDLE_BYTES) throw new RecoveryRequired();
    const buffer=Buffer.alloc(stat.size);let offset=0;
    while(offset<buffer.length){const {bytesRead}=await handle.read(buffer,offset,buffer.length-offset,null);if(!bytesRead) throw new RecoveryRequired();offset+=bytesRead;}
    if((await handle.read(Buffer.alloc(1),0,1,null)).bytesRead) throw new RecoveryRequired();return buffer;
  }finally{await handle.close();}
}

export class NativeBundleCache {
  private tail:Promise<void>=Promise.resolve(); private active=0;
  private root:string|undefined;
  constructor(readonly journal:Journal, readonly limits:{bytes:number;bundles:number;concurrent:number}=NATIVE_CACHE_LIMITS) {}
  private async directory():Promise<string> {
    if(this.root) return this.root;
    if(!path.isAbsolute(this.journal.db.name)) throw new RecoveryRequired();
    const parent=path.dirname(await fs.realpath(this.journal.db.name)); const root=path.join(parent,'openbot-native-bundles');
    try {await fs.mkdir(root,{mode:0o700});}catch(error){if((error as NodeJS.ErrnoException).code!=='EEXIST')throw error;}
    await privateDirectory(root);this.root=root;return root;
  }
  private serial<T>(work:()=>Promise<T>):Promise<T> {
    const result=this.tail.then(work);this.tail=result.then(()=>{},()=>{});return result;
  }
  private async inventory(signal:AbortSignal):Promise<Map<string,Plan>> {
    const root=await this.directory();const plans=new Map<string,Plan>();const entries=await fs.opendir(root);let total=0;
    for await(const entry of entries) {
      signal.throwIfAborted();if(!/^[0-9a-f]{64}$/.test(entry.name) || plans.size>=this.limits.bundles) throw new RecoveryRequired();
      const target=path.join(root,entry.name);await privateDirectory(target);
      const raw=await boundedRaw(path.join(target,'bundle.json'));
      const expected=plan(raw,entry.name);
      total+=expected.cost;if(total>this.limits.bytes) throw new JournalCapacity();
      await validateDirectory(target,expected,signal);
      plans.set(entry.name,expected);
    }
    return plans;
  }
  async load(reader:BundleReader,operation:string,reference:NativeReference,purpose:'admission'|'required_configuration',signal:AbortSignal):Promise<{configuration:RequiredConfiguration;bundle:NativeBundle}> {
    if(this.active>=this.limits.concurrent) throw new JournalCapacity();this.active++;
    try {
      const raw=await reader(operation,signal);signal.throwIfAborted();const expected=plan(raw,reference.digest);
      return await this.serial(async()=>{
        signal.throwIfAborted();const root=await this.directory();const inventory=await this.inventory(signal);const target=path.join(root,reference.digest);
        if(!inventory.has(reference.digest)) {
          if(purpose==='required_configuration' || this.journal.hasMappedBundle(reference.digest) || this.journal.hasDispatchedBundle(reference.digest)) throw new RecoveryRequired();
          if(inventory.size>=this.limits.bundles || expected.cost+[...inventory.values()].reduce((sum,item)=>sum+item.cost,0)>this.limits.bytes) throw new JournalCapacity();
          await this.publish(target,expected,signal);
        }
        await validateDirectory(target,expected,signal);
        return {bundle:expected.bundle,configuration:{instructions:expected.instructions,skillDirectories:expected.trees.map(tree=>({rootPath:path.join(target,'skills',tree.name),expectedTreeHash:tree.hash}))}};
      });
    }catch(error){if(error instanceof JournalCapacity || error instanceof RecoveryRequired || signal.aborted)throw error;throw new RecoveryRequired();}finally{this.active--;}
  }
  private async publish(target:string,expected:Plan,signal:AbortSignal):Promise<void> {
    const temporary=path.join(path.dirname(target),`tmp-${randomUUID()}`);let created=false;
    try {
      await fs.mkdir(temporary,{mode:0o700});created=true;
      for(const directory of [...expected.directories].filter(Boolean).sort((a,b)=>a.length-b.length)) {signal.throwIfAborted();await fs.mkdir(path.join(temporary,directory),{mode:0o700});}
      for(const file of expected.files.values()) {
        signal.throwIfAborted();const handle=await fs.open(path.join(temporary,file.path),'wx',file.mode);
        try{await handle.writeFile(file.bytes,{signal});await handle.chmod(file.mode);await handle.sync();}finally{await handle.close();}
      }
      await validateDirectory(temporary,expected,signal);signal.throwIfAborted();await fs.rename(temporary,target);created=false;
    }finally{if(created)await fs.rm(temporary,{recursive:true,force:true});}
  }
  async cleanup(signal:AbortSignal):Promise<void> {
    await this.serial(async()=>{
      const inventory=await this.inventory(signal);const references=this.journal.bundleReferences();const root=await this.directory();
      for(const [digest,expected]of inventory) if(!references.has(digest)) {signal.throwIfAborted();const target=path.join(root,digest);await validateDirectory(target,expected,signal);await fs.rm(target,{recursive:true});}
    });
  }
}
