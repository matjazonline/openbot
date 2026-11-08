#!/usr/bin/env python3
"""Local links, reset inventory and actual integration configuration/CI entry points."""
from pathlib import Path
import re
import sys

root = Path(__file__).resolve().parents[1]
errors = []
documents = [root/'docs/vault-integrations.md', root/'docs/gmail-workflows.md',
             root/'plan/.archive/vault-google/07-integration-verification-and-operations.md']
for file in documents:
    text = file.read_text()
    for target in re.findall(r'\[[^\]]+\]\(([^)]+)\)', text):
        target = target.split('#')[0]
        if not target or '://' in target or target.startswith('mailto:'): continue
        if not (file.parent/target).exists(): errors.append(f'{file.relative_to(root)}: broken link {target}')

main = (root/'src/main.rs').read_text()
inventory = main.split('const NO_BACKWARD_COMPATIBILITY_NOTES:',1)[1].split('];',1)[0]
carriers = []
for file in sorted((root/'plan').rglob('*.md')):
    if any(part in ('phases','integration') for part in file.relative_to(root/'plan').parts): continue
    if file.name == 'COORDINATION.md': continue
    text = file.read_text()
    if '> **No backward compatibility.**' in text:
        carriers.append(file)
        if f'"{file.relative_to(root).as_posix()}"' not in inventory:
            errors.append(f'{file.relative_to(root)}: reset notice missing from startup inventory')
for file in (root/'plan/.archive/vault-google').glob('[0-9][0-9]-*.md'):
    if '> **No backward compatibility.**' not in file.read_text(): errors.append(f'{file}: missing reset notice')

docs = documents[0].read_text()
sources = ''.join((root/path).read_text() for path in [
    'src/infra/config.rs', 'src/infra/setup.rs', 'src/adapters/persistence/credentials/mod.rs'])
for key in ['GOOGLE_OAUTH_CLIENT_ID', 'GOOGLE_OAUTH_CLIENT_SECRET', 'APP_DOMAIN_NAME',
            'CREDENTIAL_ENCRYPTION_KEYS', 'CREDENTIAL_ENCRYPTION_ACTIVE_VERSION', 'AUTH_ATTEMPT_SECRET']:
    if key not in docs or f'"{key}"' not in sources: errors.append(f'configuration claim has no source: {key}')
if 'RUST_LOG' not in docs or 'EnvFilter::try_from_default_env' not in sources:
    errors.append('RUST_LOG claim requires the tracing default environment filter')
ci = (root/'.github/workflows/ci.yml').read_text()
for path in ['scripts/tests/integration-boundary.py', 'scripts/integration-boundary.py',
             'scripts/tests/vault-authoring-browser.mjs']:
    if path not in ci or not (root/path).is_file(): errors.append(f'CI entry point missing: {path}')
for family in ['integration_portability_', 'integration_service_contract_', 'vault_google_e2e_']:
    if family not in ci: errors.append(f'CI conformance inventory missing: {family}')
extract = docs.split('## Service extraction gate',1)[1].split('## Implemented verification',1)[0]
if re.findall(r'^([1-6])\. ', extract, re.M) != list('123456'):
    errors.append('extraction checklist must retain all six gates')
for error in errors: print(error, file=sys.stderr)
if errors: sys.exit(1)
print(f'integration docs: {len(documents)} link sets, {len(carriers)} startup reset notices, configuration and CI inventory PASS')
