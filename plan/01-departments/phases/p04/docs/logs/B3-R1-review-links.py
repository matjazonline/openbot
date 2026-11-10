import re, os, sys
def slug(h):
    h = h.strip().lower()
    h = re.sub(r'[`*_~]', '', h)
    h = re.sub(r'[^\w\- ]', '', h)
    return h.replace(' ', '-')
def anchors(path):
    out=set(); fence=False
    for line in open(path, encoding='utf-8'):
        if line.lstrip().startswith('```'): fence = not fence; continue
        if not fence and re.match(r'#{1,6} ', line):
            out.add(slug(re.sub(r'^#{1,6} ', '', line)))
    return out
bad=0; n=0
for path in sys.argv[1:]:
    fence=False
    for ln, line in enumerate(open(path, encoding='utf-8'), 1):
        if line.lstrip().startswith('```'): fence = not fence; continue
        if fence: continue
        code_free = re.sub(r'`[^`]*`', '', line)
        for m in re.finditer(r'\]\(([^)\s]+)\)', code_free):
            t = m.group(1)
            if re.match(r'(https?|mailto):', t): continue
            n += 1
            file, _, frag = t.partition('#')
            target = os.path.normpath(os.path.join(os.path.dirname(path), file)) if file else path
            if not os.path.exists(target):
                print(f"MISSING {path}:{ln} -> {t}"); bad+=1; continue
            if frag and target.endswith('.md') and frag not in anchors(target):
                print(f"ANCHOR {path}:{ln} -> {t}"); bad+=1
print(f"checked {n} relative links, {bad} problems")
