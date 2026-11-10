from pathlib import Path
import hashlib,json,stat,subprocess

BASE='9c205daad5c029229f0547dc137a9936c6b2b49c'
EXCLUDED='plan/fix-app/phases/p06/docs/'

def git(*args):
    return subprocess.check_output(['git',*args])

def snapshot():
    names=sorted(set(git('ls-files','-z','--cached','--others','--exclude-standard').decode().split('\0'))- {''})
    untracked=[p for p in git('ls-files','-z','--others','--exclude-standard').decode().split('\0') if p and not p.startswith(EXCLUDED)]
    assert not untracked, ('Unstaged new source files',untracked)
    manifest=[]
    for name in names:
        if name.startswith(EXCLUDED):continue
        p=Path(name)
        if not p.exists() and not p.is_symlink():continue
        content=p.readlink().as_posix().encode() if p.is_symlink() else p.read_bytes()
        manifest.append({'path':name,'mode':oct(stat.S_IMODE(p.lstat().st_mode)), 'sha256':hashlib.sha256(content).hexdigest()})
    raw=json.dumps(manifest,sort_keys=True,separators=(',',':')).encode()
    diff=git('diff','--binary',BASE,'--','.',f':(exclude){EXCLUDED}**')
    return {'base':BASE,'head':git('rev-parse','HEAD').decode().strip(),'exclusions':[EXCLUDED],
            'diff_sha256':hashlib.sha256(diff).hexdigest(),'tree_sha256':hashlib.sha256(raw).hexdigest(),
            'files_count':len(manifest),'manifest':manifest}

if __name__=='__main__':print(json.dumps(snapshot(),indent=2))
