// Explicitly opted-in tests only. Fixed prompts and new disposable projects; no policy bypass.
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { BbPluginApi, PluginCliResult } from "@get-bb/plugin-sdk";

type Mapping = { providerId: string; model: string; projectId: string; markers: string[] };
const providers = ["codex", "claude-code", "acp-opencode"];

export async function nativeCommand(bb: BbPluginApi, argv: string[], allowed: Set<string>): Promise<PluginCliResult | undefined> {
  if (!argv[0]?.startsWith("native-")) return;
  assert.equal(process.env.OPENBOT_CLOUDROOM_NATIVE_SMOKE, "1", "native smoke requires explicit opt-in");
  let result: unknown;
  if (argv[0] === "native-start") {
    assert(providers.includes(argv[1]), "unsupported native provider");
    assert(argv[3] === undefined || argv[3] === "low", "this bounded smoke supports only explicit low reasoning");
    result = await start(bb, argv[1], allowed, argv[2], argv[3]);
  } else if (argv[0] === "native-catalog") {
    assert(providers.includes(argv[1]));
    const host = (await bb.sdk.hosts.list())[0];
    const catalog = await bb.sdk.providers.models({ providerId: argv[1], hostId: host.id, signal: AbortSignal.timeout(20_000) });
    result = { models: catalog.models.map(({ model }) => model), error: catalog.modelLoadError?.code ?? null };
  } else if (argv[0] === "native-refresh-catalog") {
    assert(providers.includes(argv[1]));
    const host = (await bb.sdk.hosts.list())[0];
    const catalog = await bb.sdk.providers.restartModelDiscovery({ providerId: argv[1], hostId: host.id, signal: AbortSignal.timeout(45_000) });
    result = { models: catalog.models.map(({ model }) => model), error: catalog.modelLoadError?.code ?? null };
  } else {
    const threadId = argv[1];
    assert(threadId && /^thr_[a-z0-9]+$/.test(threadId));
    const mapping = await bb.storage.kv.get<Mapping>(`native:${threadId}`);
    assert(mapping, "only this disposable probe's native threads may be controlled");
    if (argv[0] === "native-state") result = await state(bb, threadId, mapping);
    else if (["native-send", "native-permission", "native-question", "native-outside-write", "native-stop-task"].includes(argv[0])) {
      const marker = `OPENBOT_SMOKE_${randomBytes(12).toString("hex")}`;
      mapping.markers.push(marker);
      await bb.storage.kv.set(`native:${threadId}`, mapping);
      const permissionTarget = argv[0] === "native-outside-write" ? join(await mkdtemp(join(tmpdir(), "openbot-permission-smoke-")), "smoke.txt") : undefined;
      const text = permissionTarget
        ? `Disposable permission test: use your shell tool to write the single word smoke to exactly ${permissionTarget}. This target is a fresh directory owned by this test, outside the project to exercise permission handling. If permission is required, ask; do not work around the boundary or use another path. Then reply exactly ${marker}. Do not access any other files, network, or subagents.`
        : argv[0] === "native-question"
          ? `Disposable interaction test: invoke your AskUserQuestion tool to ask 'Continue the disposable Openbot smoke test?' with choices 'Continue smoke' and 'Stop smoke'. Wait for the answer, then reply exactly ${marker}. Do not use other tools, change files, access network, or start subagents.`
          : argv[0] === "native-permission"
        ? `Disposable local permission smoke: use your shell tool once to run exactly curl --max-time 5 --silent http://127.0.0.1:46986/api/v1/system/version then reply exactly ${marker}. Do not read or change files, use other URLs, or start subagents.`
        : argv[0] === "native-stop-task"
          ? `Disposable cancellation smoke: use your shell tool once to run exactly sleep 20 then reply exactly ${marker}. Do not read or change files, use network, or start subagents.`
          : prompt(marker);
      const receipt = await bb.sdk.threads.send({ threadId, model: mapping.model, mode: "auto",
        ...(["native-permission", "native-outside-write"].includes(argv[0]) ? { permissionMode: "accept-edits" as const } : {}),
        input: [{ type: "text", text, mentions: [] }] });
      result = { marker, delivery: receipt.delivery, permissionTarget };
    } else if (argv[0] === "native-stop") {
      result = await bb.sdk.threads.stop({ threadId });
    } else if (argv[0] === "native-archive") {
      await bb.sdk.threads.stop({ threadId });
      result = await bb.sdk.threads.archive({ threadId });
    } else throw new Error("unknown native smoke command");
  }
  return { exitCode: 0, stdout: JSON.stringify(result) };
}

function prompt(marker: string) {
  return `This is an authorized disposable integration smoke test. Reply with exactly ${marker}. Do not use tools, change files, or start subagents.`;
}

async function start(bb: BbPluginApi, providerId: string, allowed: Set<string>, model?: string, reasoningLevel?: "low") {
  const hosts = await bb.sdk.hosts.list();
  assert.equal(hosts.length, 1);
  assert.equal(hosts[0].name, "openbot-p01-disposable");
  const catalog = await bb.sdk.providers.models({ providerId, hostId: hosts[0].id, signal: AbortSignal.timeout(20_000) });
  assert(!catalog.modelLoadError, `provider catalog failed: ${catalog.modelLoadError?.code}`);
  const selected = model ? catalog.models.find((candidate) => candidate.model === model)
    : catalog.models.find(({ model }) => /luna|mini|haiku/i.test(model))
      ?? catalog.models.find(({ isDefault }) => isDefault) ?? catalog.models[0];
  assert(selected, "no advertised model; never fall back during execution");
  const path = await mkdtemp(join(tmpdir(), "openbot-native-smoke-"));
  const project = await bb.sdk.projects.create({ name: `Openbot ${providerId} smoke`,
    source: { type: "local_path", hostId: hosts[0].id, path } });
  allowed.add(project.id);
  await bb.storage.kv.set("native-projects", [...allowed]);
  const marker = `OPENBOT_SMOKE_${randomBytes(12).toString("hex")}`;
  await bb.storage.kv.set(`native-intent:${marker}`, { projectId: project.id, providerId, model: selected.model, state: "dispatching" });
  const thread = await bb.sdk.threads.spawn({ projectId: project.id, executionTarget: "local",
    providerId, model: selected.model, reasoningLevel, environment: { type: "project-default" },
    title: `Openbot ${providerId} smoke`, prompt: prompt(marker), pluginMetadata: { operation: marker } });
  await bb.storage.kv.set(`native:${thread.id}`, { providerId, model: selected.model, projectId: project.id, markers: [marker] } satisfies Mapping);
  return { threadId: thread.id, projectId: project.id, marker, model: selected.model, providerId, status: thread.status };
}

async function state(bb: BbPluginApi, threadId: string, mapping: Mapping) {
  const thread = await bb.sdk.threads.get({ threadId });
  const events = await readEvents(bb, threadId);
  const replay = await readEvents(bb, threadId);
  const stable = events.every((event, index) => JSON.stringify(event) === JSON.stringify(replay[index]));
  const output = await bb.sdk.threads.output({ threadId });
  const interactions = await bb.sdk.threads.interactions.list({ threadId });
  const execution = await bb.sdk.threads.defaultExecutionOptions({ threadId });
  const firstPage = await bb.sdk.threads.events.list({ threadId, afterSeq: "0", limit: "1", order: "asc" });
  const secondPage = firstPage.length ? await bb.sdk.threads.events.list({ threadId, afterSeq: String(firstPage[0].seq), limit: "1", order: "asc" }) : [];
  const turns = mapping.markers.map((marker) => {
    const requests = events.filter((event) => event.type === "client/turn/requested" && JSON.stringify(event.data.input).includes(marker));
    const requestIds = requests.flatMap((event) => "requestId" in event.data ? [event.data.requestId] : []);
    const accepted = events.filter((event) => event.type === "turn/input/accepted" && requestIds.includes(event.data.clientRequestId));
    const turnIds = [...new Set(accepted.flatMap((event) => event.scope.kind === "turn" ? [event.scope.turnId] : []))];
    const completed = events.filter((event) => event.type === "turn/completed" && event.scope.kind === "turn" && turnIds.includes(event.scope.turnId));
    return { marker, requestIds, turnIds, terminal: completed.map((event) => event.type === "turn/completed" ? event.data.status : null) };
  });
  return { threadId, ...mapping, status: thread.status, eventPrefixReplay: stable,
    pagination: firstPage.length === 1 && secondPage.length === 1 && secondPage[0].seq > firstPage[0].seq,
    turns,
    output: output.output?.slice(0, 1024) ?? null, permissionMode: execution?.permissionMode,
    pending: interactions.map(({ id, payload, status }) => ({ id, kind: payload.kind, status })),
    events: events.map((event) => ({ seq: event.seq, type: event.type, scope: event.scope,
      ...("requestId" in event.data ? { requestId: event.data.requestId } : {}),
      ...("clientRequestId" in event.data ? { clientRequestId: event.data.clientRequestId } : {}),
      ...("status" in event.data ? { status: event.data.status } : {}),
      ...("item" in event.data ? { itemType: event.data.item.type,
        ...(event.data.item.type === "agentMessage" ? { agentText: event.data.item.text.slice(0, 1024) } : {}) } : {}),
      ...(event.type === "client/turn/requested" ? { markers: mapping.markers.filter((marker) => JSON.stringify(event.data.input).includes(marker)) } : {}),
    })) };
}

async function readEvents(bb: BbPluginApi, threadId: string) {
  const events = [];
  let afterSeq = 0;
  for (let page = 0; page < 8; page++) {
    const batch = await bb.sdk.threads.events.list({ threadId, afterSeq: String(afterSeq), limit: "100", order: "asc" });
    events.push(...batch);
    if (batch.length < 100) return events;
    assert(batch.at(-1)!.seq > afterSeq, "event cursor must advance");
    afterSeq = batch.at(-1)!.seq;
  }
  throw new Error("native probe event bound exhausted");
}
