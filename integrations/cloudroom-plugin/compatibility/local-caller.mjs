// Test fixture for the local caller proof performed by Cloudroom's desktop wrapper.
// No provider credentials are read or exported. Keep the issued token in memory only.
import assert from "node:assert/strict";
import { realpath, stat, writeFile, unlink } from "node:fs/promises";
import { join } from "node:path";

export async function localCallerHeaders(origin, profile) {
  const url = new URL(origin);
  assert.equal(url.protocol, "http:");
  assert.equal(url.hostname, "127.0.0.1");
  const post = async (path, body) => {
    const response = await fetch(new URL(path, url), {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify(body), redirect: "error", signal: AbortSignal.timeout(5000),
    });
    assert(response.ok, "local caller proof refused");
    return response.json();
  };
  const started = await post("/api/v1/caller-proofs", {});
  assert.match(started.id, /^[0-9a-f]{12}-[0-9a-f]{32}$/);
  assert.equal(await realpath(started.directory), join(await realpath(profile), "caller-proofs"));
  const directory = await stat(started.directory);
  assert(directory.isDirectory() && directory.uid === process.getuid());
  const path = join(started.directory, started.id);
  await writeFile(path, started.id, { flag: "wx", mode: 0o600 });
  try {
    const { token } = await post(`/api/v1/caller-proofs/${started.id}`, { claim: started.claim });
    assert.match(token, /^[A-Za-z0-9_-]{32,128}$/);
    return { "x-cloudroom-caller": token };
  } finally {
    try { await unlink(path); } catch (error) { if (error.code !== "ENOENT") throw error; }
  }
}
