import re,sys
pat=re.compile(r'\b[Cc]hannel(s|\'s|s\')?\b')
for path in sys.argv[1:]:
    fence=False
    for n,line in enumerate(open(path),1):
        if line.lstrip().startswith('```'):
            fence=not fence; continue
        if fence: continue
        # drop inline code spans and link targets
        stripped=re.sub(r'`[^`]*`','',line)
        stripped=re.sub(r'\]\([^)]*\)','](...)',stripped)
        hits=[m.group(0) for m in pat.finditer(stripped)]
        if hits:
            print(f"{path}:{n}: {line.rstrip()[:220]}")
