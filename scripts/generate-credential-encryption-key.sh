#!/usr/bin/env bash
#
# Generate a development credential-encryption key suitable for .env.

set -euo pipefail

key="$(openssl rand -base64 32 | tr -d '=\n')"

printf 'CREDENTIAL_ENCRYPTION_KEYS=1:%s\n' "$key"
