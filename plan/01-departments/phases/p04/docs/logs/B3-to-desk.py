import re,sys
word=re.compile(r"\b([Cc])hannel(s?)\b")
def conv(text):
    def rep(m):
        d='D' if m.group(1)=='C' else 'd'
        return d+'esk'+m.group(2)
    return word.sub(rep,text)
def convert_line(line):
    # split into code spans and link targets; convert only prose parts
    out=[];i=0
    tokens=re.split(r'(`[^`]*`|\]\([^)]*\)|https?://\S+)',line)
    for t in tokens:
        if t.startswith('`') or t.startswith('](') or t.startswith('http'):
            out.append(t)
        else:
            out.append(conv(t))
    return ''.join(out)
for path in sys.argv[1:]:
    lines=open(path).read().split('\n')
    fence=False; new=[]
    for line in lines:
        if line.lstrip().startswith('```'):
            fence=not fence; new.append(line); continue
        new.append(line if fence else convert_line(line))
    open(path,'w').write('\n'.join(new))
