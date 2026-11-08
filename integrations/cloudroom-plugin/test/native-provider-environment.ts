import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

// Standard provider configuration directories are scoped to these child CLIs.
// The parent environment and ordinary login/config files are never changed/read.
export function loggedOutProviderEnvironment(provider: string, current: NodeJS.ProcessEnv, directory: string) {
  const isolated = join(directory, 'logged-out-provider'); mkdirSync(isolated, { mode: 0o700 });
  const env = { ...current };
  const credentials = ['OPENAI_API_KEY', 'CODEX_API_KEY', 'OPENAI_ACCESS_TOKEN', 'ANTHROPIC_API_KEY',
    'ANTHROPIC_AUTH_TOKEN', 'CLAUDE_CODE_OAUTH_TOKEN', 'OPENCODE_AUTH_JSON', 'OPENCODE_CONFIG_CONTENT'];
  for (const key of credentials) delete env[key];
  if (provider === 'codex') env.CODEX_HOME = isolated;
  else if (provider === 'claude-code') env.CLAUDE_CONFIG_DIR = isolated;
  else {
    assert.equal(provider, 'acp-opencode');
    env.XDG_CONFIG_HOME = join(isolated, 'config'); env.XDG_DATA_HOME = join(isolated, 'data');
    env.OPENCODE_CONFIG_DIR = join(isolated, 'config'); env.OPENCODE_CONFIG = join(isolated, 'opencode.json');
    writeFileSync(env.OPENCODE_CONFIG, '{}\n', { mode: 0o600 });
  }
  let nativeLoginStatus: number | null = null;
  if (provider !== 'acp-opencode') {
    const args = provider === 'codex' ? ['login', 'status'] : ['auth', 'status'];
    const result = spawnSync(provider === 'codex' ? 'codex' : 'claude', args,
      { env, cwd: directory, encoding: 'utf8', timeout: 15_000, maxBuffer: 16 * 1024 });
    assert(!result.error && result.status !== null && result.status !== 0, 'isolated native CLI must be logged out');
    nativeLoginStatus = result.status;
  } else {
    const result = spawnSync('opencode', ['auth', 'list'],
      { env, cwd: directory, encoding: 'utf8', timeout: 15_000, maxBuffer: 16 * 1024 });
    assert(!result.error && result.status === 0);
    const plain = (result.stdout + result.stderr).replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '');
    assert(/0 credentials|no credentials/i.test(plain), 'isolated OpenCode must have no stored login');
    nativeLoginStatus = 0;
  }
  return { env, proof: { provider, isolatedProviderConfiguration: true, parentLoginUntouched: true,
    credentialEnvironmentRemoved: credentials, nativeLoginStatus } };
}
