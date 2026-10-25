"""Where each top-level item (and each method of an `impl` block) of a split file went, and
whether its text changed. Usage (repository root):
  python3 <this> <old-file>... -- <new-file>...
Prints one line per item: `same`, `same-but-visibility` (only `pub(..)` added or removed),
`changed` (followed by a unified diff of the item), `removed` or `added`, with the files it is in.
Use declarations and module doc comments are compared as one `use`/`//!` group per side."""
import difflib, re, sys

VIS = re.compile(r'\bpub(\([^)]*\))? ')
HEAD = re.compile(r'^(\s*)(?:pub(?:\([^)]*\))? )?(?:async )?(fn|struct|enum|const|static|type|trait|mod) (\w+)')

def items(path):
    lines = open(path).read().split('\n')
    found, pending, i = [], [], 0
    def block(i, indent):
        # from line i to its closing line at `indent`, or a one-line item ending with `;`
        if lines[i].rstrip().endswith(';'):
            return i
        j = i
        while not (lines[j].startswith(indent + '}') and len(lines[j]) - len(lines[j].lstrip()) == len(indent)):
            j += 1
        return j
    while i < len(lines):
        line = lines[i]
        if line.startswith('use ') or line.startswith('//!'):
            found.append(('use' if line.startswith('use') else '//!', [line], path)); i += 1
            while i < len(lines) and lines[i - 1].rstrip()[-1:] not in (';', '') and not lines[i - 1].startswith('//!'):
                found[-1][1].append(lines[i]); i += 1
            continue
        if line.startswith('///') or line.startswith('#['):
            pending.append(line); i += 1; continue
        if line.startswith('impl'):
            target = re.match(r'impl(?:<[^>]*>)? ([\w:<>]+)', line).group(1)
            j, mpending = i + 1, []
            while not lines[j].startswith('}'):
                m = HEAD.match(lines[j])
                if lines[j].strip().startswith(('///', '#[')):
                    mpending.append(lines[j]); j += 1; continue
                if m and m.group(1) == '    ':
                    end = block(j, '    ')
                    found.append((f'impl {target}::{m.group(3)}', mpending + lines[j:end + 1], path))
                    mpending, j = [], end + 1; continue
                j += 1
            pending, i = [], j + 1; continue
        m = HEAD.match(line)
        if m and m.group(1) == '':
            end = block(i, '')
            found.append((f'{m.group(2)} {m.group(3)}', pending + lines[i:end + 1], path))
            pending, i = [], end + 1; continue
        pending = [] if line.strip() == '' else pending
        i += 1
    return found

split = sys.argv.index('--')
sides = [sys.argv[1:split], sys.argv[split + 1:]]
maps = []
for files in sides:
    named = {}
    for path in files:
        for name, text, where in items(path):
            if name in ('use', '//!'):
                entry = named.setdefault(name, ([], set()))
                entry[0].extend(text); entry[1].add(where)
            else:
                assert name not in named, f'{name} twice'
                named[name] = (text, {where})
    maps.append(named)
old, new = maps
counts = {}
for name in list(old) + [n for n in new if n not in old]:
    if name not in new:
        state = 'removed'
    elif name not in old:
        state = 'added'
    elif name in ('use', '//!'):
        state = 'same' if sorted(old[name][0]) == sorted(new[name][0]) else 'changed'
    elif old[name][0] == new[name][0]:
        state = 'same'
    elif [VIS.sub('', l) for l in old[name][0]] == [VIS.sub('', l) for l in new[name][0]]:
        state = 'same-but-visibility'
    else:
        state = 'changed'
    counts[state] = counts.get(state, 0) + 1
    where = ' -> '.join(', '.join(sorted(side[name][1])) for side in (old, new) if name in side)
    print(f'{state:20} {name}  [{where}]')
    if state == 'changed' or state == 'added' and name != 'use':
        a = old[name][0] if name in old else []
        b = new[name][0]
        if name in ('use', '//!'):
            a, b = sorted(a), sorted(b)
        for line in difflib.unified_diff(a, b, 'old', 'new', lineterm='', n=2):
            print('    ' + line)
print(counts)
