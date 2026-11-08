import assert from 'node:assert/strict';
import { createServer } from 'node:https';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import WebSocket, { WebSocketServer } from 'ws';
import { encodeFrames } from '../src/protocol.js';

export async function waitFor(assertion: () => boolean, milliseconds = 8_000): Promise<void> {
  const until = Date.now() + milliseconds;
  while (!assertion()) { if (Date.now() >= until) throw new Error('fixture condition timed out'); await delay(10); }
}
export async function server() {
  const directory = process.env.OPENBOT_TEST_TLS!;
  assert(directory, 'test runner must install its trusted TLS CA');
  const https = createServer({ key: readFileSync(join(directory, 'key.pem')), cert: readFileSync(join(directory, 'cert.pem')) });
  const wss = new WebSocketServer({ server: https, maxPayload: 64 * 1024 });
  await new Promise<void>(resolve => https.listen(0, '127.0.0.1', resolve));
  const address = https.address(); assert(address && typeof address !== 'string');
  return { https, wss, origin: `https://localhost:${address.port}`,
    close: async () => { for (const client of wss.clients) client.terminate(); await new Promise<void>(resolve => wss.close(() => resolve())); await new Promise<void>(resolve => https.close(() => resolve())); } };
}
export function write(socket: WebSocket, message: unknown): void { for (const frame of encodeFrames(message)) socket.send(frame); }
