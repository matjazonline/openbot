"""Report class tokens used in the given Rust page sources that neither the compiled
assets/app.css nor an inline <style> const of the pages defines. Stand-in for
`npm run check:css` while node_modules is absent (p09 TASK "Tooling")."""
import glob, re, sys

css = open('assets/app.css').read()
inline = ''.join(open(path).read() for path in glob.glob('src/adapters/http/pages/*.rs'))

def escaped(token):
    return re.sub(r'([:/\[\]\.\(\)%#,>+~=!&@])', r'\\\1', token)

def defined(token):
    for text in (css, inline):
        pattern = '.' + escaped(token)
        start = text.find(pattern)
        while start != -1:
            after = text[start + len(pattern):start + len(pattern) + 1]
            if not re.match(r'[A-Za-z0-9_\-\\]', after):
                return True
            start = text.find(pattern, start + 1)
        if text is inline and re.search(r'\.' + re.escape(token) + r'[\s,{:.\[]', inline):
            return True
    return False

# Styled by htmx itself (its injected indicator styles), not by Tailwind.
PROVIDED = {'htmx-indicator', 'htmx-request'}
missing = {}
for path in sys.argv[1:]:
    for number, line in enumerate(open(path), 1):
        for value in re.findall(r'class=\\?"([^"\\]*)', line):
            for token in value.split():
                if '{' in token or '}' in token:
                    continue
                if token not in PROVIDED and not defined(token):
                    missing.setdefault(token, []).append(f'{path}:{number}')
for token, places in sorted(missing.items()):
    print(f'MISSING {token}  ({places[0]}{" +" + str(len(places) - 1) if len(places) > 1 else ""})')
print(f'checked {len(sys.argv) - 1} files; {len(missing)} missing tokens')
