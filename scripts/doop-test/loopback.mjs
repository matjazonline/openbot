// Bind the unmodified upstream service to the runner's already reserved socket.
// This file is a test preload, not a production patch or an authentication bypass.
import { Server } from 'node:net';
import { writeFile, rename } from 'node:fs/promises';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const fd = Number(process.env.DOOP_TEST_SOCKET_FD);
const observeOnly = process.env.DOOP_TEST_OBSERVE_ONLY === '1';
if (!observeOnly && (!Number.isInteger(fd) || fd < 3)) throw new Error('Missing inherited test socket');
const original = Server.prototype.listen;
let claimed = false;
Server.prototype.listen = function (...args) {
  if (!claimed && args[0] === Number(process.env.PORT)) {
    claimed = true;
    if (!observeOnly) args[0] = { fd, exclusive: true };
    if (process.env.DOOP_TEST_SOURCE && process.env.DOOP_TEST_SEED_METADATA) {
      this.once('listening', async () => {
        // Observe an actual fresh ownerless fixture; never create/alter auth or data.
        const { store } = await import(pathToFileURL(join(process.env.DOOP_TEST_SOURCE, 'server/store.ts')));
        const ids = store.allCanvases().filter((canvas) => !canvas.ownerId && !canvas.workspaceId).map((canvas) => canvas.id);
        const destination = process.env.DOOP_TEST_SEED_METADATA;
        await writeFile(destination + '.new', JSON.stringify({ ownerlessCanvasIds: ids.slice(0, 128) }), { mode: 0o600 });
        await rename(destination + '.new', destination);
      });
    }
  }
  return original.apply(this, args);
};
