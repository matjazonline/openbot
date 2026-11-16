"""Package the owned pinned checkout; never stage or commit its files."""
from pathlib import Path
import hashlib, json, subprocess
root=Path('/private/tmp/openbot-cloudroom-harness-t1gfnhz1/cloudroom')
main=Path('/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree')
compat=main/'integrations/cloudroom-plugin/compatibility'
def git(*args):return subprocess.check_output(['git','-C',str(root),*args])
def sha(data):return hashlib.sha256(data).hexdigest()
manifest=json.loads((compat/'manifest.json').read_text())
assert git('rev-parse','HEAD').decode().strip()==manifest['cloudroom']['revision']
oldpatches=manifest['sourcePatches'][:2]
preexisting=set()
for patch in oldpatches:
 for target in patch['targets']:
  preexisting.add(target['sourcePath'])
  assert sha((root/target['sourcePath']).read_bytes())==target['afterSha256']
changed=git('diff','--name-only','HEAD').decode().splitlines()
added=git('ls-files','--others','--exclude-standard','--','gui/').decode().splitlines()
targets=sorted(set(changed+added)-preexisting)
assert len(targets)+len(preexisting)<=60 and all(path.startswith('gui/') for path in targets)
patch=git('diff','--full-index','--binary','HEAD','--',*(path for path in targets if path in changed))
for path in targets:
 if path not in added:continue
 result=subprocess.run(['git','diff','--no-index','--full-index','--binary','--','/dev/null',path],cwd=root,capture_output=True)
 assert result.returncode==1,result.stderr
 patch+=result.stdout
patchpath='patches/required-native-configuration.patch'
(compat/patchpath).write_bytes(patch)
descriptors=[{'sourcePath':path,'beforeSha256':None if path in added else sha(git('show','HEAD:'+path)),
 'afterSha256':sha((root/path).read_bytes())} for path in targets]
manifest['sourcePatches']=oldpatches+[{'path':patchpath,'sha256':sha(patch),'targets':descriptors}]
for source in manifest['sourceFiles']:source['sha256']=sha((root/source['path']).read_bytes())
(compat/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
# This is the actual pinned public SDK generator's output, not a hand-written subset.
declaration=main/'integrations/cloudroom-plugin/types/bb-plugin-sdk.d.ts'
declaration.write_bytes((root/'gui/packages/plugin-sdk/bundled-types/bb-plugin-sdk.d.ts').read_bytes())
identity={'revision':manifest['cloudroom']['revision'],'patches':[{'sha256':patch['sha256'],'targets':[{'sourcePath':target['sourcePath'],
 'sourceSha256':target['afterSha256']} for target in patch['targets']]} for patch in manifest['sourcePatches']]}
types=main/'integrations/cloudroom-plugin/types/manifest.json'
typemanifest=json.loads(types.read_text());typemanifest.update(identity)
typemanifest['artifactDigest']=sha(json.dumps(identity,separators=(',',':')).encode())
typemanifest['declarationSha256']=sha(declaration.read_bytes())
types.write_text(json.dumps(typemanifest,indent=2)+'\n')
print(json.dumps({'targets':len(targets)+len(preexisting),'new_files':len(added),'patch_bytes':len(patch),
 'patch_sha256':sha(patch),'public_sdk_declaration_sha256':sha(declaration.read_bytes())}))
