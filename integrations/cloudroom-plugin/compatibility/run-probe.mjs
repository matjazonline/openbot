// Usage: node run-probe.mjs <pinned-cloudroom-checkout> <evidence-directory>
// Builds source with its lockfile and uses only a newly created local profile.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createWriteStream } from "node:fs";
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { CLOUDROOM_REVISION, prepareSource } from "./check-source.mjs";
import { proveUi } from "./prove-ui.mjs";

const sourceRevision = CLOUDROOM_REVISION;
assert.equal(process.version, "v24.21.0", "use the manifest's pinned tested Node runtime");
assert.equal(process.argv.length, 4, "pass source checkout and evidence directory");
const checkout = resolve(process.argv[2]);
const evidence = resolve(process.argv[3]);
const cwd = resolve(checkout, "gui");
const probe = resolve(dirname(fileURLToPath(import.meta.url)), "probe");
await mkdir(evidence, { recursive: true });
const resources = await mkdtemp(resolve(tmpdir(), "openbot-cloudroom-proof-"));
const profile = resolve(resources, "profile");
await mkdir(profile);
await mkdir("/tmp/openbot-cloudroom-p01-workspace", { recursive: true });
const children = [];
let env;

async function run(command, args, name, secret = false, expectFailure = false) {
  const child = spawn(command, args, { cwd, env: env ?? process.env, detached: true, stdio: ["ignore", "pipe", "pipe"] });
  const parts = [];
  let size = 0;
  const stream = secret ? null : createWriteStream(resolve(evidence, name));
  let stopping;
  const terminate = () => (stopping ??= stop(child));
  const capture = (part) => {
    size += part.length;
    if (size > 16 * 1024 * 1024) { void terminate(); return; }
    parts.push(part);
    stream?.write(part);
  };
  child.stdout.on("data", capture);
  child.stderr.on("data", capture);
  const timer = setTimeout(() => void terminate(), 5 * 60 * 1000);
  try {
    const [code] = await once(child, "exit");
    if (expectFailure) assert.notEqual(code, 0, "crash-cut command unexpectedly succeeded");
    else assert.equal(code, 0, `${command} failed; see ${name}`);
    return Buffer.concat(parts).toString("utf8").trim();
  } finally { clearTimeout(timer); await stopping; stream?.end(); }
}

async function port() {
  const listener = createServer();
  listener.listen(0, "127.0.0.1");
  await once(listener, "listening");
  const value = listener.address().port;
  await new Promise((done) => listener.close(done));
  return value;
}

function service(entry, name, extra = {}, args = [], workingDirectory = cwd) {
  const log = createWriteStream(resolve(evidence, name));
  const child = spawn(process.execPath, ["--conditions=source", "--import", "tsx", entry, ...args], {
    cwd: workingDirectory, env: { ...env, ...extra }, detached: true, stdio: ["ignore", "pipe", "pipe"],
  });
  child.stdout.pipe(log, { end: false });
  child.stderr.pipe(log, { end: false });
  child.once("exit", () => log.end());
  children.push(child);
  return child;
}

async function stop(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const finished = once(child, "exit");
  process.kill(-child.pid, "SIGTERM");
  const timer = setTimeout(() => { try { process.kill(-child.pid, "SIGKILL"); } catch {} }, 5000);
  try { await finished; } finally { clearTimeout(timer); }
}

async function ready(check, child) {
  const expires = Date.now() + 30_000;
  while (Date.now() < expires && child.exitCode === null && child.signalCode === null) {
    try { if (await check()) return; } catch {}
    await new Promise((done) => setTimeout(done, 200));
  }
  throw new Error("disposable service did not become ready; inspect logs");
}

const cli = (args, name, secret = false, expectFailure = false) => run(process.execPath,
  ["--conditions=source", "--import", "tsx", "apps/cli/src/index.ts", ...args], name, secret, expectFailure);

try {
  const sourceIdentity = prepareSource(checkout);
  await writeFile(resolve(evidence, "source-revision.txt"), sourceRevision + "\n");
  await writeFile(resolve(evidence, "source-clean.txt"), "PASS: pinned source plus the exact approved UI and SDK cancellation patches\n");
  await writeFile(resolve(evidence, "source-patch.json"), JSON.stringify(sourceIdentity, null, 2) + "\n");
  await run("pnpm", ["install", "--frozen-lockfile"], "dependencies.txt");
  await run("pnpm", ["--filter", "@get-bb/plugin-sdk", "build"], "sdk-build.txt");
  await run("pnpm", ["--filter", "@get-bb/plugin-sdk", "build:types"], "sdk-types.txt");
  await run("pnpm", ["--filter", "@cloudroom/cli", "build"], "cli-build.txt");
  await run("pnpm", ["--filter", "@cloudroom/app", "build"], "ui-build.txt");
  const serverPort = await port();
  const daemonPort = await port();
  const appPort = await port();
  const origin = `http://127.0.0.1:${serverPort}`;
  const appOrigin = `http://127.0.0.1:${appPort}`;
  env = { ...process.env, NODE_ENV: "production", BB_DATA_DIR: profile,
    ROOM_DATA_DIR: profile, BB_SERVER_BIND_HOST: "127.0.0.1", BB_SERVER_URL: origin,
    ROOM_SERVER_URL: origin, BB_SERVER_PORT: String(serverPort),
    BB_HOST_DAEMON_PORT: String(daemonPort), BB_HOST_DAEMON_AUTO_UPDATE: "false",
    BB_APP_URL: appOrigin, BB_DEV_APP_PORT: String(appPort), BB_DEV_APP_HOST: "127.0.0.1" };
  let server = service("apps/server/src/index.ts", "server.txt");
  await ready(async () => (await fetch(`${origin}/api/v1/system/version`, { signal: AbortSignal.timeout(1000) })).ok, server);
  await cli(["plugin", "install", `path:${probe}`, "--yes"], "install.txt");
  const before = JSON.parse(await cli(["plugin", "run", "openbot-compatibility-probe", "report"], "before.txt"));
  assert.equal(before.lifecycle.loads, 1);
  assert.equal(before.secretPersisted, true);
  await cli(["plugin", "reload", "openbot-compatibility-probe"], "reload.txt");
  const after = JSON.parse(await cli(["plugin", "run", "openbot-compatibility-probe", "report"], "after.txt"));
  assert.equal(after.lifecycle.loads, 2);
  assert.equal(after.lifecycle.aborts, 1);
  assert.equal(after.secretSameAcrossReload, true);
  const claim = JSON.parse(await cli(["machine", "join-code", "--json"], "secret-not-logged", true));
  const host = service("apps/host-daemon/src/index.ts", "host.txt", {
    BB_HOST_ID: claim.hostId, BB_HOST_ENROLL_KEY: claim.joinCode, BB_HOST_NAME: "openbot-p01-disposable",
  });
  await ready(async () => (await fetch(`http://127.0.0.1:${daemonPort}/status`, { signal: AbortSignal.timeout(1000) })).ok, host);
  const queued = JSON.parse(await cli(["plugin", "run", "openbot-compatibility-probe", "queued"], "queued.txt"));
  assert.equal(queued.createDeduplicates, false);
  assert.equal(queued.queueLength, 3);
  await cli(["plugin", "run", "openbot-compatibility-probe", "catalog"], "catalog.txt");
  const shared = JSON.parse(await cli(["plugin", "run", "openbot-compatibility-probe", "prepare-shared"], "shared.txt"));
  const app = service("node_modules/vite/bin/vite.js", "ui-server.txt", {}, [
    "preview", "--config", "vite.dev.config.ts", "--configLoader", "runner",
    "--port", String(appPort), "--host", "127.0.0.1",
  ], resolve(cwd, "apps/app"));
  await ready(async () => (await fetch(appOrigin, { signal: AbortSignal.timeout(1000) })).ok, app);
  await proveUi({ origin: appOrigin, ...shared, evidence, backendOrigin: origin, profile });
  await cli(["thread", "tell", shared.threadId, "Independent CLI message", "--mode", "auto", "--model", "gpt-5.1-codex", "--send-at", new Date(Date.now() + 86_400_000).toISOString(), "--json"], "second-client.txt");
  await cli(["plugin", "run", "openbot-compatibility-probe", "check-shared"], "shared-check.txt");
  await cli(["plugin", "run", "openbot-compatibility-probe", "crash-create"], "crash-cut.txt", false, true);
  assert.equal(server.signalCode, "SIGKILL", "probe must cut the server after SDK acceptance");
  server = service("apps/server/src/index.ts", "server-after-crash.txt");
  await ready(async () => (await fetch(`${origin}/api/v1/system/version`, { signal: AbortSignal.timeout(1000) })).ok, server);
  const recovery = JSON.parse(await cli(["plugin", "run", "openbot-compatibility-probe", "recover"], "recovery.txt"));
  assert.equal(recovery.reconciled, true);
  assert.equal(recovery.attempts, 1);
  assert.equal(recovery.newSpawnCalls, 0);
  const beforeRestart = JSON.parse(await cli(["plugin", "run", "openbot-compatibility-probe", "report"], "before-restart.txt"));
  await stop(server);
  server = service("apps/server/src/index.ts", "server-after-restart.txt");
  await ready(async () => (await fetch(`${origin}/api/v1/system/version`, { signal: AbortSignal.timeout(1000) })).ok, server);
  const afterRestart = JSON.parse(await cli(["plugin", "run", "openbot-compatibility-probe", "report"], "after-restart.txt"));
  assert.equal(afterRestart.lifecycle.loads, beforeRestart.lifecycle.loads + 1);
  assert.equal(afterRestart.lifecycle.aborts, beforeRestart.lifecycle.aborts + 1);
  assert.equal(afterRestart.secretSameAcrossReload, true);
  assert.equal(afterRestart.hosts[0].id, claim.hostId);
  const sdk = JSON.parse(await readFile(resolve(cwd, "packages/plugin-sdk/package.json")));
  await writeFile(resolve(evidence, "result.json"), JSON.stringify({
    sourceRevision, sourcePatches: sourceIdentity.patches, sourceArtifactDigest: sourceIdentity.artifactDigest, sdkVersion: sdk.version, nodeVersion: process.version, profile,
    lifecycle: "PASS", restartIdentity: "PASS", acceptanceMappingCrash: "PASS",
    twoClientsQueuedOrder: "PASS", codexQueuedThreadUi: "PASS", queuedMutations: "PASS", createDeduplicates: false,
    queuedSendDeduplicates: false, nativeProviderInference: "NOT EXERCISED",
    terminalTurnCorrelation: "NOT EXERCISED", eventReplay: "NOT EXERCISED",
    targetedStop: "UNSUPPORTED", platform: process.platform,
  }, null, 2) + "\n");
  console.log("PASS: public SDK lifecycle and queued-mutation characterization; native inference/terminal recovery NOT EXERCISED");
} finally {
  for (const child of children.reverse()) await stop(child);
  // Preserve this explicitly named disposable profile for inspection; never touch a user's profile.
}
