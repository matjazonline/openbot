import { constants, openSync, closeSync } from 'node:fs';
import { dirname, join, isAbsolute } from 'node:path';
import { flockSync } from 'fs-ext';
export function acquireProfileLock(databaseName: string): () => void {
  if (!isAbsolute(databaseName)) throw new Error('persistent plugin database required');
  const fd = openSync(join(dirname(databaseName), 'openbot-connector.lock'),
    constants.O_RDWR | constants.O_CREAT | constants.O_NOFOLLOW, 0o600);
  try { flockSync(fd, 'exnb'); } catch { closeSync(fd); throw new Error('another connector owns this profile'); }
  let held = true;
  return () => { if (held) { held = false; closeSync(fd); } };
}
