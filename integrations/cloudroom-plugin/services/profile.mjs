import assert from 'node:assert/strict';
import { constants, openSync, closeSync, mkdirSync, lstatSync } from 'node:fs';
import { resolve } from 'node:path';
import { flockSync } from 'fs-ext';

export function acquireProfile(profile) {
  mkdirSync(profile, { recursive: true, mode: 0o700 });
  const info = lstatSync(profile);
  assert(info.isDirectory() && info.uid === process.getuid() && (info.mode & 0o077) === 0,
    'profile must be owned, private and not a symlink');
  const fd = openSync(resolve(profile, 'openbot-launcher.lock'), constants.O_RDWR | constants.O_CREAT | constants.O_NOFOLLOW, 0o600);
  try { flockSync(fd, 'exnb'); } catch (error) { closeSync(fd); throw error; }
  return () => closeSync(fd);
}
