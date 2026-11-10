from pathlib import Path
import hashlib,json,stat,subprocess
BASE="9c205daad5c029229f0547dc137a9936c6b2b49c"
EXCLUDES=["plan/fix-app/integration/p06-main/docs/","plan/fix-app/COORDINATION.md"]
def git(*args):return subprocess.check_output(["git",*args])
def excluded(p):return any(p==e or p.startswith(e) for e in EXCLUDES)
def snapshot():
    names=sorted(set(git("ls-files","-z","--cached","--others","--exclude-standard").decode().split("\0"))-{''})
    files=[]
    for n in names:
        if excluded(n):continue
        p=Path(n)
        if not p.exists() and not p.is_symlink():continue
        data=p.readlink().as_posix().encode() if p.is_symlink() else p.read_bytes()
        files.append(dict(path=n,mode=oct(stat.S_IMODE(p.lstat().st_mode)),sha256=hashlib.sha256(data).hexdigest()))
    diff=git("diff","--binary",BASE,"--",".",*[':(exclude)'+e+'**' if e.endswith('/') else ':(exclude)'+e for e in EXCLUDES])
    return dict(base=BASE,head=git("rev-parse","HEAD").decode().strip(),exclusions=EXCLUDES,tree_sha256=hashlib.sha256(json.dumps(files,sort_keys=True,separators=(',',':')).encode()).hexdigest(),diff_sha256=hashlib.sha256(diff).hexdigest(),files_count=len(files),manifest=files)
if __name__=="__main__":print(json.dumps(snapshot(),indent=2))
