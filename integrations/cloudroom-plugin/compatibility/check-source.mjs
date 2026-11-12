import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { realpathSync,readFileSync,lstatSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname,resolve,sep } from 'node:path';
import { fileURLToPath } from 'node:url';

export const CLOUDROOM_REVISION='d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815';
export const SOURCE_PATCH_LIMITS=Object.freeze({patches:8,targets:80,fileBytes:2*1024*1024,patchBytes:8*1024*1024});
const compatibilityRoot=dirname(fileURLToPath(import.meta.url));
const hash=value=>createHash('sha256').update(value).digest('hex');
const digest=value=>typeof value==='string' && /^[0-9a-f]{64}$/.test(value);
const lines=value=>value.split('\n').filter(Boolean).sort();
function checkedGit(checkout,revision) {
  const root=realpathSync(checkout);
  const git=args=>execFileSync('git',['-C',root,...args],{encoding:'utf8',timeout:10000,maxBuffer:64*1024,stdio:['ignore','pipe','pipe']}).trim();
  assert.equal(realpathSync(git(['rev-parse','--show-toplevel'])),root,'pass the checkout root');
  assert.equal(git(['rev-parse','HEAD']),revision,'wrong Cloudroom revision');
  return {root,git};
}
function sourceHash(root,sourcePath) {
  const target=resolve(root,sourcePath);let stat;
  try{stat=lstatSync(target);}catch(error){if(error.code!=='ENOENT')throw error;}
  if(!stat){let parent=dirname(target);for(;;){try{assert(realpathSync(parent).startsWith(root+sep),'source path escapes checkout');break;}catch(error){if(error.code!=='ENOENT')throw error;const next=dirname(parent);assert(next!==parent,'missing checkout ancestor');parent=next;}}return null;}
  assert(stat.isFile() && !stat.isSymbolicLink() && (stat.mode&0o111)===0,'unexpected source file mode');
  assert(realpathSync(target).startsWith(root+sep),'source path escapes checkout');
  assert(stat.size<=SOURCE_PATCH_LIMITS.fileBytes,'source file capacity exceeded');
  return hash(readFileSync(target));
}
function headHash(root,git,sourcePath) {
  const entry=git(['ls-tree','HEAD','--',sourcePath]);if(!entry)return null;
  assert(entry.startsWith('100644 blob '),'unexpected pinned source mode');
  const size=Number(git(['cat-file','-s',`HEAD:${sourcePath}`]));assert(Number.isSafeInteger(size)&&size<=SOURCE_PATCH_LIMITS.fileBytes,'pinned source capacity exceeded');
  return hash(execFileSync('git',['-C',root,'show',`HEAD:${sourcePath}`],{timeout:10000,maxBuffer:SOURCE_PATCH_LIMITS.fileBytes+1,stdio:['ignore','pipe','pipe']}));
}
function declaredTargets(patches) {
  assert(Array.isArray(patches)&&patches.length>0&&patches.length<=SOURCE_PATCH_LIMITS.patches,'bounded source patches required');
  const targets=[];
  for(const patch of patches){
    assert(patch && Object.keys(patch).sort().join(',')==='path,sha256,targets' && typeof patch.path==='string' && digest(patch.sha256),'invalid source patch declaration');
    assert(Array.isArray(patch.targets)&&patch.targets.length>0&&patch.targets.length<=SOURCE_PATCH_LIMITS.targets,'bounded source patch targets required');
    for(const target of patch.targets){
      assert(target && Object.keys(target).sort().join(',')==='afterSha256,beforeSha256,sourcePath','invalid source target declaration');
      assert(typeof target.sourcePath==='string' && target.sourcePath.length<=512 && /^gui\/(?:[A-Za-z0-9._-]+\/)*[A-Za-z0-9._-]+$/.test(target.sourcePath) && !target.sourcePath.split('/').some(part=>part==='.'||part==='..'),'unsafe source path');
      assert((target.beforeSha256===null || digest(target.beforeSha256)) && digest(target.afterSha256) && target.beforeSha256!==target.afterSha256,'invalid source fingerprints');targets.push(target);
    }
  }
  assert(targets.length<=SOURCE_PATCH_LIMITS.targets,'source target capacity exceeded');
  const paths=targets.map(target=>target.sourcePath).sort();
  assert(new Set(paths).size===targets.length,'overlapping source patches');
  assert(paths.every((path,index)=>!index || !path.startsWith(paths[index-1]+'/')),'source target prefix collision');return targets;
}
function inspectPatch(root,patchRoot,patch,git) {
  const patchPath=realpathSync(resolve(patchRoot,patch.path));assert(patchPath.startsWith(realpathSync(patchRoot)+sep),'patch escapes fixture root');
  const stat=lstatSync(patchPath);assert(stat.isFile()&&stat.size<=SOURCE_PATCH_LIMITS.patchBytes,'source patch capacity exceeded');
  assert.equal(hash(readFileSync(patchPath)),patch.sha256,'tampered source patch');
  assert.deepEqual(lines(git(['apply','--numstat',patchPath])).map(line=>line.split('\t')[2]).sort(),patch.targets.map(target=>target.sourcePath).sort(),'patch changes unapproved paths');
  const states=patch.targets.map(target=>{
    assert.equal(headHash(root,git,target.sourcePath),target.beforeSha256,'source pin differs from declared before fingerprint');
    const current=sourceHash(root,target.sourcePath);assert(current===target.beforeSha256 || current===target.afterSha256,'source differs from approved patch');return current===target.beforeSha256;
  });
  assert(states.every(state=>state===states[0]),'partially applied cohesive source patch');return {patch,patchPath,needed:states[0]};
}
function changedInputs(git) {
  return [...new Set([...lines(git(['diff','--name-only','HEAD'])),...lines(git(['ls-files','--others','--exclude-standard','--','gui/']))])].sort();
}
function guardInputs(git,targets) {
  const allowed=new Set(targets.map(target=>target.sourcePath));
  assert(lines(git(['diff','--name-only','HEAD'])).every(path=>allowed.has(path)),'unapproved source edits');
  const additions=new Set(targets.filter(target=>target.beforeSha256===null).map(target=>target.sourcePath));
  assert(lines(git(['ls-files','--others','--exclude-standard','--','gui/'])).every(path=>additions.has(path)),'unexpected untracked GUI inputs');
}
export function verifySource(checkout,expectedRevision=CLOUDROOM_REVISION) {
  const {git}=checkedGit(checkout,expectedRevision);git(['diff','--quiet','HEAD','--','gui/']);
  assert.equal(git(['ls-files','--others','--exclude-standard','--','gui/']),'','unexpected untracked GUI inputs');return expectedRevision;
}
export function prepareSource(checkout,{revision=CLOUDROOM_REVISION,patches=JSON.parse(readFileSync(resolve(compatibilityRoot,'manifest.json'),'utf8')).sourcePatches,patchRoot=compatibilityRoot}={}) {
  const {root,git}=checkedGit(checkout,revision);const targets=declaredTargets(patches);guardInputs(git,targets);
  const prepared=patches.map(patch=>inspectPatch(root,patchRoot,patch,git));
  for(const {needed,patchPath}of prepared)if(needed)git(['apply','--check',patchPath]);
  for(const {needed,patchPath}of prepared)if(needed)git(['apply','--whitespace=error',patchPath]);
  for(const target of targets)assert.equal(sourceHash(root,target.sourcePath),target.afterSha256,'source differs from approved patch');
  guardInputs(git,targets);assert.deepEqual(changedInputs(git),targets.map(target=>target.sourcePath).sort(),'unapproved source edits');
  const identity={revision,patches:prepared.map(({patch})=>({sha256:patch.sha256,targets:patch.targets.map(target=>({sourcePath:target.sourcePath,sourceSha256:target.afterSha256}))}))};
  return {...identity,artifactDigest:hash(JSON.stringify(identity))};
}
