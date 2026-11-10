// Install only in a disposable Cloudroom profile. No provider inference is requested.
import { randomBytes } from "node:crypto";
import assert from "node:assert/strict";
import type { BbPluginApi } from "@get-bb/plugin-sdk";
import { nativeCommand } from "./native.js";

export default async function probe(bb: BbPluginApi) {
  const settings = bb.settings.define({
    proofSecret: { type: "string", label: "Disposable proof secret", secret: true },
  });
  let secret = (await settings.get()).proofSecret;
  if (!secret) {
    secret = randomBytes(32).toString("hex");
    await settings.experimental_set({ proofSecret: secret });
  }
  const previousSecret = await bb.storage.kv.get<string>("proof-secret");
  assert(previousSecret === undefined || previousSecret === secret);
  await bb.storage.kv.set("proof-secret", secret);
  const db = bb.storage.database();
  const nativeProjects = new Set(await bb.storage.kv.get<string[]>("native-projects") ?? []);
  bb.storage.migrate(db, [
    "CREATE TABLE probe (id INTEGER PRIMARY KEY CHECK(id=1), loads INTEGER NOT NULL, aborts INTEGER NOT NULL)",
    "INSERT INTO probe VALUES (1,0,0)",
    "CREATE TABLE intent (operation TEXT PRIMARY KEY, project TEXT NOT NULL, thread TEXT, state TEXT NOT NULL, attempts INTEGER NOT NULL)",
  ]);
  bb.experimental_hooks.on("message.dispatch", ({ thread }) => thread.originPluginId === bb.pluginId
    && !(process.env.OPENBOT_CLOUDROOM_NATIVE_SMOKE === "1" && nativeProjects.has(thread.projectId))
    ? { action: "wait", reason: "Disposable compatibility probe never runs inference" }
    : { action: "proceed" });
  db.prepare("UPDATE probe SET loads=loads+1 WHERE id=1").run();
  bb.background.service("openbot-proof", {
    async start(signal) {
      if (!signal.aborted) {
        await new Promise<void>((resolve) => signal.addEventListener("abort", () => resolve(), { once: true }));
      }
      db.prepare("UPDATE probe SET aborts=aborts+1 WHERE id=1").run();
    },
  });
  bb.cli.register({
    name: "openbot-proof",
    summary: "Disposable, non-inference public SDK compatibility checks",
    async run(argv) {
      const native = await nativeCommand(bb, argv, nativeProjects);
      if (native) return native;
      if (argv[0] === "crash-create") {
        const project = await createProject(bb);
        const operation = randomBytes(16).toString("hex");
        db.prepare("INSERT INTO intent VALUES (?,?,NULL,'dispatching',1)").run(operation, project.id);
        await bb.sdk.threads.spawn(scheduledInput(project.id, operation));
        // Deliberate cut: SDK accepted, but no thread identity was saved in our journal.
        process.kill(process.pid, "SIGKILL");
        return { exitCode: 1, stderr: "server should have crashed" };
      }
      if (argv[0] === "recover") {
        const intent = db.prepare("SELECT * FROM intent WHERE state='dispatching'").get() as
          { operation: string; project: string; thread: string | null; attempts: number };
        assert(intent && intent.thread === null && intent.attempts === 1);
        const threads = await bb.sdk.threads.list({ projectId: intent.project, originPluginId: bb.pluginId, limit: 100 });
        const matches = [];
        for (const thread of threads) {
          if ((await bb.sdk.threads.getPluginMetadata({ threadId: thread.id })).operation === intent.operation) matches.push(thread.id);
        }
        assert.equal(matches.length, 1, "recover from positive metadata evidence only");
        db.prepare("UPDATE intent SET thread=?,state='mapped' WHERE operation=?").run(matches[0], intent.operation);
        await bb.sdk.threads.delete({ threadId: matches[0], childThreadsConfirmed: true });
        return { exitCode: 0, stdout: JSON.stringify({ reconciled: true, attempts: intent.attempts, newSpawnCalls: 0, source: "pluginMetadata" }) };
      }
      if (argv[0] === "prepare-shared") {
        const project = await createProject(bb);
        const thread = await bb.sdk.threads.spawn(scheduledInput(project.id, randomBytes(16).toString("hex")));
        await bb.storage.kv.set("shared-thread", thread.id);
        return { exitCode: 0, stdout: JSON.stringify({ threadId: thread.id, projectId: project.id }) };
      }
      if (argv[0] === "check-shared") {
        return { exitCode: 0, stdout: JSON.stringify(await checkShared(bb)) };
      }
      if (argv[0] === "catalog") {
        const host = (await bb.sdk.hosts.list())[0];
        const discoveryDeadline = Date.now() + 15_000;
        while (Date.now() < discoveryDeadline) {
          if ((await bb.sdk.providers.list({ hostId: host.id })).some(({ id }) => id === "acp-opencode")) break;
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
        const catalog = [];
        for (const providerId of ["codex", "claude-code", "acp-opencode"]) {
          try {
            const models = await bb.sdk.providers.models({ providerId, hostId: host.id, signal: AbortSignal.timeout(15_000) });
            catalog.push({ providerId, models: models.models.length, error: models.modelLoadError?.code ?? null });
          } catch { catalog.push({ providerId, models: null, error: "unavailable" }); }
        }
        return { exitCode: 0, stdout: JSON.stringify({ catalog, inference: "NOT EXERCISED" }) };
      }
      if (argv[0] === "queued") {
        return { exitCode: 0, stdout: JSON.stringify(await queuedProof(bb)) };
      }
      if (argv[0] === "report") {
        const hosts = await bb.sdk.hosts.list();
        const providers = await bb.sdk.providers.list();
        const projects = await bb.sdk.projects.list();
        return { exitCode: 0, stdout: JSON.stringify({
          lifecycle: db.prepare("SELECT loads,aborts FROM probe WHERE id=1").get(),
          secretPersisted: (await settings.get()).proofSecret === secret,
          secretSameAcrossReload: previousSecret === secret,
          hosts: hosts.map(({ id, name, status }) => ({ id, name, status })),
          providers: providers.map(({ id }) => id),
          projects: projects.map(({ id }) => id),
        }) };
      }
      return { exitCode: 1, stderr: "Use report; further probes require an explicitly selected disposable project." };
    },
  });
}

async function createProject(bb: BbPluginApi) {
  const hosts = await bb.sdk.hosts.list();
  assert.equal(hosts.length, 1, "run only in a disposable single-host profile");
  assert.equal(hosts[0].name, "openbot-p01-disposable");
  return bb.sdk.projects.create({ name: "Openbot public SDK proof",
    source: { type: "local_path", hostId: hosts[0].id, path: "/tmp/openbot-cloudroom-p01-workspace" } });
}

function scheduledInput(projectId: string, operation: string) {
  return { projectId, requestId: operation, executionTarget: "local" as const,
    providerId: "codex", model: "gpt-5.1-codex", environment: { type: "project-default" as const },
    prompt: "Disposable scheduled SDK probe; this must never execute.",
    pluginMetadata: { operation }, sendAt: Date.now() + 24 * 60 * 60 * 1000 };
}

async function checkShared(bb: BbPluginApi) {
  const threadId = await bb.storage.kv.get<string>("shared-thread");
  assert(threadId);
  try {
    const before = await bb.sdk.threads.queuedMessages.list({ threadId });
    assert.equal(before.length, 2, "SDK first prompt plus independent CLI message");
    await bb.sdk.threads.send({ threadId, model: "gpt-5.1-codex", mode: "auto",
      sendAt: Date.now() + 24 * 60 * 60 * 1000,
      input: [{ type: "text", text: "SDK after independent CLI", mentions: [] }] });
    const after = await bb.sdk.threads.queuedMessages.list({ threadId });
    assert.equal(after.length, 3);
    assert.equal(after[1].content[0].type, "text");
    assert.equal(after[1].content[0].text, "Independent CLI message");
    assert.equal(after[2].content[0].type, "text");
    assert.equal(after[2].content[0].text, "SDK after independent CLI");
    return { clients: ["public plugin SDK", "Cloudroom CLI"], queueOrder: "PASS", activeTurnCorrelation: "NOT EXERCISED" };
  } finally { await bb.sdk.threads.delete({ threadId, childThreadsConfirmed: true }); }
}

async function queuedProof(bb: BbPluginApi) {
  const project = await createProject(bb);
  const operation = randomBytes(16).toString("hex");
  const created: string[] = [];
  const sendAt = Date.now() + 24 * 60 * 60 * 1000;
  try {
    const input = {
      projectId: project.id, requestId: operation, executionTarget: "local" as const,
      providerId: "codex", model: "gpt-5.1-codex",
      environment: { type: "project-default" as const },
      prompt: "Disposable scheduled SDK probe; this must never execute.",
      pluginMetadata: { operation }, sendAt,
    };
    const first = await bb.sdk.threads.spawn(input);
    created.push(first.id);
    // Deliberately retry the public API without our mapping to characterize its guarantee.
    const second = await bb.sdk.threads.spawn(input);
    created.push(second.id);
    assert.notEqual(first.id, second.id, "pinned local spawn is NOT idempotent by requestId");
    assert.equal(first.status, "pending");
    const listed = await bb.sdk.threads.list({ projectId: project.id, limit: 100 });
    assert.equal(listed.filter(({ id }) => created.includes(id)).length, 2);
    const metadata = await bb.sdk.threads.getPluginMetadata({ threadId: first.id });
    assert.equal(metadata.operation, operation);
    const sent = await bb.sdk.threads.send({
      threadId: first.id, requestId: `${operation}_send`, model: input.model, mode: "auto", sendAt,
      input: [{ type: "text", text: "Disposable queued follow-up", mentions: [] }],
    });
    assert.equal(sent.delivery, "queued");
    const resent = await bb.sdk.threads.send({
      threadId: first.id, requestId: `${operation}_send`, model: input.model, mode: "auto", sendAt,
      input: [{ type: "text", text: "Disposable queued follow-up", mentions: [] }],
    });
    const queue = await bb.sdk.threads.queuedMessages.list({ threadId: first.id });
    const events = await bb.sdk.threads.events.list({ threadId: first.id, limit: "1", order: "asc" });
    const replay = await bb.sdk.threads.events.list({ threadId: first.id, limit: "1", order: "asc" });
    assert.deepEqual(events, replay);
    assert.equal(queue.length, 3, "pinned queued send does NOT deduplicate requestId");
    await bb.sdk.threads.stop({ threadId: first.id });
    await bb.sdk.threads.archive({ threadId: first.id });
    return { createDeduplicates: false, metadata, sendDelivery: sent.delivery,
      resendDelivery: resent.delivery, queueLength: queue.length, eventsReplayed: events.length,
      threadIds: created, nativeInference: "NOT EXERCISED", targetedStop: "UNSUPPORTED" };
  } finally {
    for (const threadId of created) {
      await bb.sdk.threads.delete({ threadId, childThreadsConfirmed: true });
    }
  }
}
