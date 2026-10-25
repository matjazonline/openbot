import json,re,sys
D='/Users/mac02/dev/openbot/plan/workflow-foundation/phases/p08/docs/logs/b4c3a-scripts/'
tables=set(open(D+'tables.txt').read().split())
funcs=set(open(D+'funcs.txt').read().split())
d=json.load(open(sys.argv[1]))
added=d['added']
def added_names():
    names={}
    for s in added:
        m=re.match(r'CREATE (?:OR REPLACE )?(FUNCTION|TABLE|TRIGGER|INDEX|UNIQUE INDEX) (?:public\.)?(\w+)',s)
        if m: names[m.group(2)]=m.group(1)
    return names
an=added_names()
cats={}
other=[]
for s in d['removed']:
    k=None
    m=re.match(r'CREATE TABLE public\.(\w+)',s)
    if m: k=('table', m.group(1), m.group(1) in tables or m.group(1) in an)
    m=m or None
    if not k:
        m=re.match(r'CREATE (?:OR REPLACE )?FUNCTION public\.(\w+)',s)
        if m: k=('function',m.group(1), m.group(1) in funcs or m.group(1) in an)
    if not k:
        m=re.match(r'ALTER TABLE (?:ONLY )?public\.(\w+) ADD CONSTRAINT (\w+)',s)
        if m: k=('constraint',m.group(1)+'.'+m.group(2), m.group(1) in tables)
    if not k:
        m=re.match(r'CREATE (?:UNIQUE )?INDEX (\w+) ON (?:ONLY )?public\.(\w+)',s)
        if m: k=('index',m.group(2)+'.'+m.group(1), m.group(2) in tables)
    if not k:
        m=re.match(r'CREATE (?:CONSTRAINT )?TRIGGER (\w+) .*? ON public\.(\w+) .*EXECUTE FUNCTION public\.(\w+)',s)
        if m: k=('trigger',m.group(2)+'.'+m.group(1)+' -> '+m.group(3), m.group(2) in tables or m.group(3) in funcs)
    if not k:
        m=re.match(r'COMMENT ON (\w+) (?:public\.)?([\w.]+)',s)
        if m:
            tgt=m.group(2).split('.')[0]
            k=('comment',m.group(1)+' '+m.group(2), tgt in tables or tgt in funcs)
    if not k:
        m=re.match(r'ALTER TABLE (?:ONLY )?public\.(\w+) (.*)',s)
        if m: k=('alter',m.group(1)+': '+m.group(2)[:80], m.group(1) in tables)
    if not k: k=('other',s[:120],False)
    cats.setdefault((k[0],k[2]),[]).append(k[1])
for (c,ok),items in sorted(cats.items()):
    print(f'== {c} dropped-object={ok}: {len(items)}')
    if not ok:
        for it in items: print('   ',it)
print('ADDED (modified/new) statements:')
for s in added: print('  +',s[:160])
