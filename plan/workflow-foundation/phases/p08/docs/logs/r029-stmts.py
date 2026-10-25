import re,sys,json
def statements(path):
    t=open(path).read()
    out=[];buf=[];i=0;n=len(t);dollar=None
    cur=''
    while i<n:
        c=t[i]
        if dollar:
            if t.startswith(dollar,i):
                cur+=dollar;i+=len(dollar);dollar=None;continue
            cur+=c;i+=1;continue
        if c=='-' and t.startswith('--',i):
            j=t.find('\n',i); j=n if j<0 else j
            i=j+1; cur+='\n'; continue
        if c=="'":
            j=i+1
            while True:
                k=t.index("'",j)
                if k+1<n and t[k+1]=="'": j=k+2; continue
                break
            cur+=t[i:k+1]; i=k+1; continue
        if c=='$':
            m=re.match(r'\$[A-Za-z_]*\$',t[i:])
            if m: dollar=m.group(0); cur+=dollar; i+=len(dollar); continue
        if c==';':
            s=' '.join(cur.split())
            if s: out.append(s)
            cur='';i+=1;continue
        cur+=c;i+=1
    s=' '.join(cur.split())
    if s: out.append(s)
    return out
if __name__=='__main__':
    a=statements(sys.argv[1]); b=statements(sys.argv[2])
    sa=set(a); sb=set(b)
    print('before',len(a),'after',len(b))
    removed=[s for s in a if s not in sb]
    added=[s for s in b if s not in sa]
    json.dump({'removed':removed,'added':added},open(sys.argv[3],'w'),indent=0)
    print('removed',len(removed),'added',len(added))
