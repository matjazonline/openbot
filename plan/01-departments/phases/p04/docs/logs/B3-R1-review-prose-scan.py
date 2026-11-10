import re, sys
# Independent reviewer scan: any case-insensitive "channel" substring outside fenced blocks,
# inline code spans (any backtick run), link targets and bare URLs. Link text is kept.
pat = re.compile(r'channel', re.I)
def strip(line):
    out = []
    i = 0
    while i < len(line):
        if line[i] == '`':
            j = i
            while j < len(line) and line[j] == '`':
                j += 1
            run = line[i:j]
            k = line.find(run, j)
            if k == -1:
                out.append(line[i:]); break
            i = k + len(run); continue
        out.append(line[i]); i += 1
    s = ''.join(out)
    s = re.sub(r'\]\([^)]*\)', '](…)', s)
    s = re.sub(r'https?://\S+', 'URL', s)
    return s
total = 0
for path in sys.argv[1:]:
    fence = None
    for n, line in enumerate(open(path, encoding='utf-8'), 1):
        st = line.lstrip()
        m = re.match(r'(```+|~~~+)', st)
        if m:
            if fence is None: fence = m.group(1)[:3]
            elif st.startswith(fence): fence = None
            continue
        if fence: continue
        if line.startswith('    ') and not st.startswith(('-', '*', '|')) and st.strip():
            pass  # indented continuation lines in lists are prose here; keep scanning
        s = strip(line)
        hits = pat.findall(s)
        if hits:
            total += len(hits)
            print(f"{path}:{n}: [{len(hits)}] {line.rstrip()[:200]}")
print(f"TOTAL {total}", file=sys.stderr)
