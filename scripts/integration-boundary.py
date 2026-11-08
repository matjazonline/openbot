#!/usr/bin/env python3
"""Integration custody/source gate; supplements actual authority and redaction tests.

Walk production Rust modules from crate roots (rather than guessing test filenames).
Ignore cfg(test) items, retain raw/ordinary SQL strings, and expand checked type
dependencies so aliasing a credential-bearing result cannot hide it in a reply.
This is a source policy, not a general Rust type checker; compilation remains required.
"""
from pathlib import Path
import re
import sys

TOKEN = re.compile(r'''//[^\n]*|/\*[\s\S]*?\*/|(?:br|r)(?P<h>\#*)"[\s\S]*?"(?P=h)|b?"(?:\\[\s\S]|[^"\\])*"|b?'(?:\\[^\n]|[^'\\\n])'|\b[A-Za-z_][A-Za-z_0-9]*\b|::|[^\s]''')
PRIVATE = {'CredentialRecord', 'TypedInjection', 'ResolvedCredential', 'SecretString',
           'VerifiedOwner', 'OwnerConfirmation', 'ExecutionContext', 'OwnerDiscoveryContext',
           'ConnectWork', 'ConnectBrowser', 'DispatchAuthority', 'CredentialDispatchGate'}
TABLE = re.compile(r'\bintegration_(?:accounts|use_grants|owner_states|connect_attempts|teardown_intents)\b', re.I)
HOST = re.compile(r'\b(?:sqlx|PostgresPersistence|PgPool|PgConnection|Session|SessionId|WorkflowFence|ClaimedWorkflow)\b|(?:application|domain)::workflow\b|(?:adapters::persistence|entities::session)\b')


def tokens(source):
    # Rust permits nested block comments. Remove them before the token regex.
    cursor = 0
    result = []
    while cursor < len(source):
        match = TOKEN.match(source, cursor)
        if not match:
            cursor += 1
            continue
        value = match.group()
        if value.startswith('/*'):
            depth, end = 1, match.start() + 2
            while depth and end < len(source):
                if source.startswith('/*', end): depth += 1; end += 2
                elif source.startswith('*/', end): depth -= 1; end += 2
                else: end += 1
            if depth: raise ValueError('unterminated Rust comment')
            cursor = end
            continue
        cursor = match.end()
        if not value.startswith('//'): result.append(value)
    return result


def balanced_end(ts, start, opening, closing):
    depth = 0
    for i in range(start, len(ts)):
        if ts[i] == opening: depth += 1
        elif ts[i] == closing:
            depth -= 1
            if not depth: return i + 1
    raise ValueError('unbalanced Rust item')


def field_end(ts, start):
    depth = 0
    for i in range(start, len(ts)):
        if ts[i] in ('(', '[', '<', '{'): depth += 1
        elif ts[i] in (')', ']', '>', '}'):
            if depth == 0: return i
            depth -= 1
        elif ts[i] == ',' and depth == 0: return i+1
    raise ValueError('unterminated Rust field')


def production(ts):
    result, i = [], 0
    while i < len(ts):
        if ts[i:i+2] == ['#', '[']:
            end = balanced_end(ts, i+1, '[', ']')
            attribute = ''.join(ts[i:end])
            if attribute == '#[cfg(test)]':
                i = end
                while ts[i:i+2] == ['#', '[']: i = balanced_end(ts, i+1, '[', ']')
                field = i
                if ts[field:field+1] == ['pub']:
                    field += 1
                    if ts[field:field+1] == ['(']: field = balanced_end(ts, field, '(', ')')
                if ts[field+1:field+2] == [':']:
                    i = field_end(ts, field+2)
                    continue
                while i < len(ts) and ts[i] not in (';', '{'): i += 1
                i = balanced_end(ts, i, '{', '}') if i < len(ts) and ts[i] == '{' else i+1
                continue
        result.append(ts[i]); i += 1
    return result


def modules(root):
    todo = [p for p in (root/'src/lib.rs', root/'src/main.rs') if p.is_file()]
    seen = set()
    while todo:
        file = todo.pop().resolve()
        if file in seen: continue
        seen.add(file)
        try: ts = production(tokens(file.read_text()))
        except ValueError as error: raise ValueError(f'{file}: {error}') from error
        yield file, ts
        directory = file.parent if file.name in ('lib.rs', 'main.rs', 'mod.rs') else file.with_suffix('')
        # Inline modules may refer to path files relative to the containing file.
        for i in range(len(ts)-2):
            if ts[i] != 'mod' or ts[i+2] != ';': continue
            name = ts[i+1]
            before = ''.join(ts[max(0, i-16):i])
            path = re.search(r'#\[path="([^"\n]+)"\](?:pub(?:\([^)]*\))?)?$', before)
            candidates = ([file.parent/path.group(1)] if path else
                          [directory/(name+'.rs'), directory/name/'mod.rs'])
            candidate = next((p for p in candidates if p.is_file()), None)
            if candidate is None: raise ValueError(f'{file}: unresolved production module {name}')
            todo.append(candidate)


def declarations(ts):
    result = {}
    for i in range(len(ts)-2):
        if ts[i] not in ('struct', 'enum', 'type'): continue
        name = ts[i+1]
        end = i+2
        while end < len(ts) and ts[end] not in ('{', ';'): end += 1
        if end == len(ts): continue
        end = balanced_end(ts, end, '{', '}') if ts[end] == '{' else end+1
        result[name] = set(ts[i+2:end])
    return result


def imports(ts):
    bindings = []
    for i, token in enumerate(ts):
        if token != 'use': continue
        end = i+1
        while end < len(ts) and ts[end] != ';': end += 1
        body = ts[i+1:end]
        def group(index, prefix):
            while index < len(body) and body[index] != '}':
                path = list(prefix)
                alias = None
                while index < len(body) and body[index] not in ('{', ',', '}'):
                    if body[index] == 'as':
                        alias = body[index+1]
                        index += 2
                        continue
                    if body[index] != '::': path.append(body[index])
                    index += 1
                if index < len(body) and body[index] == '{': index = group(index+1, path)
                elif path: bindings.append(('::'.join(path), alias or path[-1]))
                if index < len(body) and body[index] == ',': index += 1
            return index+1
        group(0, [])
    return bindings


def serializable_types(sources, bindings):
    traits = {'Serialize'}
    while True:
        added = {alias for path, alias in bindings if path.split('::')[-1] in traits} - traits
        if not added: break
        traits.update(added)
    trait = '(?:' + '|'.join(re.escape(name) for name in sorted(traits)) + ')'
    result = set()
    for _, ts in sources:
        text = ''.join(ts)
        for derive, name in re.findall(r'#\[derive\(([^]]*)\)\](?:#\[[^]]*\])*(?:pub(?:\([^)]*\))?)?(?:struct|enum)(\w+)', text):
            if set(re.findall(r'\w+', derive)) & traits: result.add(name)
        result.update(re.findall(r'impl(?:<[^>]+>)?(?:\w+::)*' + trait + r'for(\w+)', text))
    return result


def violations(root):
    failures, definitions, bindings = [], {}, []
    sources = list(modules(root))
    dto_sources = []
    for file, ts in sources:
        rel = file.relative_to(root.resolve()).as_posix()
        text = ''.join(ts)
        imported = imports(ts)
        # Follow import/re-export aliases through helper modules as well as DTOs.
        bindings.extend(imported)
        for path, alias in imported:
            definitions.setdefault(alias, set()).add(path.split('::')[-1])
        dependencies = text + '\n' + '\n'.join(path for path, _ in imported)
        module_owned = rel.startswith('src/adapters/persistence/integrations/') or rel == 'src/adapters/persistence/integrations.rs'
        if not module_owned and TABLE.search(text): failures.append(f'{rel}: integration table outside custody module')
        if not module_owned and re.search(r'integrations::(?:private|renewal::CredentialResolver)|integrations::resolve\b', dependencies):
            # Private authenticated transports are part of the integration module.
            if not rel.startswith(('src/application/integrations/', 'src/adapters/integrations/')):
                failures.append(f'{rel}: host imports private credential port')
        dto = rel.startswith(('src/application/integrations/', 'src/domain/integrations/'))
        if dto:
            if HOST.search(dependencies): failures.append(f'{rel}: public module imports host/database type')
            dto_sources.append((file, ts))
            for name, dependencies in declarations(ts).items():
                definitions.setdefault(name, set()).update(dependencies)
    serializable = serializable_types(dto_sources, bindings)
    tainted = set(PRIVATE)
    while True:
        added = {name for name, dependencies in definitions.items() if dependencies & tainted} - tainted
        if not added: break
        tainted.update(added)
    # These four reviewed wire types carry command-only secret submissions.
    command_types = {'SecretSubmission', 'StaticInput', 'Command', 'Request'}
    for name in sorted((serializable & tainted) - command_types):
        failures.append(f'{name}: serializable runtime proof or credential-bearing type')
    if definitions.get('Reply', set()) & tainted:
        failures.append('Reply: credential/runtime proof reachable from gateway result')
    return failures


if __name__ == '__main__':
    root = Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parents[1]
    try: errors = violations(root)
    except (ValueError, OSError) as error: errors = [str(error)]
    for error in errors: print(error, file=sys.stderr)
    if errors: sys.exit(1)
    print('integration boundary: production custody, DTO dependencies, nonserializable private results PASS')
