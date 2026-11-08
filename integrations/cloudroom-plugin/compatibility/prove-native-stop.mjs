// Usage: OPENBOT_CLOUDROOM_NATIVE_SMOKE=1 node prove-native-stop.mjs <checkout> <profile> <server-origin> <owned-thread> <evidence>
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";

assert.equal(process.env.OPENBOT_CLOUDROOM_NATIVE_SMOKE, "1");
assert.equal(process.argv.length, 7);
const [checkout, profile, origin, threadId, evidence] = process.argv.slice(2);
assert.match(threadId, /^thr_[a-z0-9]+$/);
assert.equal(new URL(origin).hostname, "127.0.0.1");
const env = { ...process.env, BB_DATA_DIR: profile, ROOM_DATA_DIR: profile,
  BB_SERVER_URL: origin, ROOM_SERVER_URL: origin };
async function cli(command) {
  const child = spawn(process.execPath, ["--conditions=source", "--import", "tsx", "apps/cli/src/index.ts",
    "plugin", "run", "openbot-compatibility-probe", command, threadId], {
    cwd: resolve(checkout, "gui"), env, stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "";
  child.stdout.on("data", (data) => { output += data; });
  child.stderr.on("data", (data) => { output += data; });
  const timer = setTimeout(() => child.kill("SIGKILL"), 10_000);
  try {
    const code = await new Promise((done, reject) => { child.once("error", reject); child.once("exit", done); });
    assert.equal(code, 0, `native command failed: ${output.slice(0, 1000)}`);
    return JSON.parse(output);
  } finally { clearTimeout(timer); }
}
async function until(predicate) {
  const deadline = Date.now() + 35_000;
  while (Date.now() < deadline) {
    const state = await cli("native-state");
    if (predicate(state)) return state;
    await new Promise((done) => setTimeout(done, 100));
  }
  throw new Error("native state did not reach the asserted condition");
}
const send = await cli("native-stop-task");
const active = await until((state) => {
  const turn = state.turns.find((turn) => turn.marker === send.marker);
  return state.status === "active" && turn?.turnIds.length === 1 && turn.terminal.length === 0
    && state.events.some((event) => event.type === "item/started" && event.itemType === "commandExecution"
      && event.scope.turnId === turn.turnIds[0]);
});
const receipt = await cli("native-stop");
const stopped = await until((state) => state.turns.find((turn) => turn.marker === send.marker)?.terminal.length > 0);
for (const [name, data] of Object.entries({ send, active, receipt, stopped })) {
  await writeFile(resolve(evidence, `active-stop-${name}.json`), JSON.stringify(data, null, 2) + "\n");
}
const turn = stopped.turns.find((turn) => turn.marker === send.marker);
assert.equal(turn.turnIds[0], active.turns.at(-1).turnIds[0]);
assert(!stopped.events.some((event) => event.type === "item/completed" && event.itemType === "agentMessage"
  && event.scope.turnId === turn.turnIds[0] && event.agentText?.includes(send.marker)), "turn completed before cancellation");
assert(turn.terminal.includes("interrupted"), `unexpected stop terminal state: ${turn.terminal}`);
console.log(JSON.stringify({ threadId, turnId: turn.turnIds[0], terminal: turn.terminal, threadWideStop: true }));
