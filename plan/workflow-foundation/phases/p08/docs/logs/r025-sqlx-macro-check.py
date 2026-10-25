import os,re,glob,hashlib,json,sys
def parse_lit(t,i):
    while t[i].isspace(): i+=1
    if t[i]=='r':
        j=i+1; n=0
        while t[j]=='#': n+=1; j+=1
        assert t[j]=='"'; j+=1
        end=t.index('"'+'#'*n,j)
        return t[j:end]
    assert t[i]=='"',t[i:i+20]
    j=i+1; out=[]
    while True:
        c=t[j]
        if c=='\\':
            n=t[j+1]
            if n=='\n':
                j+=2
                while t[j].isspace(): j+=1
                continue
            out.append({'n':'\n','t':'\t','"':'"','\\':'\\','r':'\r','0':'\0',"'":"'"}[n]); j+=2; continue
        if c=='"': return ''.join(out)
        out.append(c); j+=1
used=set(); sites=0
for d,_,fs in os.walk('src'):
    for f in fs:
        if not f.endswith('.rs'): continue
        t=open(os.path.join(d,f)).read()
        for m in re.finditer(r'query(_as|_scalar)?(_unchecked)?!\s*\(',t):
            i=m.end()
            if m.group(1)=='_as':
                i=t.index(',',i)+1
            try: lit=parse_lit(t,i)
            except Exception as e: print('PARSE FAIL',f,t[m.start():m.start()+80]); continue
            sites+=1; used.add(hashlib.sha256(lit.encode()).hexdigest())
files={os.path.basename(p)[6:-5] for p in glob.glob('.sqlx/*.json')}
print('macro sites',sites,'distinct',len(used),'files',len(files))
print('stale (file, no macro):',sorted(files-used))
print('missing (macro, no file):',sorted(used-files))
