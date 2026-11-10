// Validate the retained real-provider smoke captures; never manufactures execution evidence.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

assert.equal(process.argv.length, 3);
const root = resolve(process.argv[2]);
const read = (path) => JSON.parse(readFileSync(resolve(root, path), "utf8"));
for (const [name, stateFile, uiFile] of [
  ["codex", "codex-post-restart-state.txt", "codex-ui/ui-thread.json"],
  ["claude-code", "claude-after-answer-state.txt", "claude-ui-after-answer/ui-thread.json"],
  ["acp-opencode", "opencode-openai-state.txt", "opencode-ui/ui-thread.json"],
]) {
  const state = read(stateFile);
  assert.equal(state.providerId, name);
  assert.equal(state.status, "idle");
  assert(state.eventPrefixReplay && state.pagination);
  assert(state.events.length > 10, "replay must contain execution events");
  for (const turn of state.turns) {
    assert.equal(turn.requestIds.length, 1);
    assert.equal(turn.turnIds.length, 1);
    assert.deepEqual(turn.terminal, ["completed"]);
    assert(state.events.some((event) => event.type === "turn/input/accepted"
      && event.clientRequestId === turn.requestIds[0] && event.scope.turnId === turn.turnIds[0]));
  }
  assert.equal(state.output, state.turns.at(-1).marker);
  const ui = read(uiFile);
  assert.equal(ui.threadId, state.threadId);
  assert.equal(ui.actualThreadRead, state.threadId);
  assert(ui.assertedTextVisible && !ui.apiStandIns);
}
const blocked = read("codex-pre-ui-recovery-state.txt");
assert(blocked.pending.some((interaction) => interaction.kind === "approval" && interaction.status === "pending"));
const recovered = read("active-stop-active.json");
assert.equal(recovered.pending.length, 0);
assert.deepEqual(recovered.turns.find((turn) => turn.marker === blocked.turns.at(-1).marker).terminal, ["completed"]);
assert.equal(read("codex-ui-denial.json").decision, "deny");
const question = read("claude-interaction-state.txt");
assert(question.pending.some((interaction) => interaction.kind === "user_question" && interaction.status === "pending"));
assert.equal(read("answer-ui.json").realInteractionSubmitted, true);
assert.equal(read("claude-after-answer-state.txt").pending.length, 0);
const active = read("active-stop-active.json").turns.at(-1);
const stopped = read("active-stop-stopped.json").turns.at(-1);
assert.equal(active.terminal.length, 0);
assert.equal(active.turnIds[0], stopped.turnIds[0]);
assert.deepEqual(stopped.terminal, ["interrupted"]);
assert(read("manual-setup.json").skipDismissed);
console.log("PASS: 3 real native providers/UI, correlated terminal turns, nonempty replay/pagination, approval denial, answered question, active thread-wide stop, optional manual setup");
