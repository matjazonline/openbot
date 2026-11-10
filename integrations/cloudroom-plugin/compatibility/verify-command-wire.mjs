// Cross-language contract proof. Input comes from actual Rust typed serialization.
import assert from "node:assert/strict";
import { parseOperation } from "../src/protocol.ts";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const digest = (command, deadline) => createHash("sha256")
  .update(JSON.stringify([command, deadline]), "utf8").digest("hex");
const fixtures = JSON.parse(readFileSync(0, "utf8"));
assert.equal(fixtures.length, 6);
assert.deepEqual(new Set(fixtures.map(({ command }) => command.kind)),
  new Set(["create", "send", "inspect", "stop_thread", "archive_thread", "refresh_catalog"]));
for (const operation of fixtures) {
  const { command, deadline, payload_digest } = operation;
  assert.deepEqual(parseOperation(operation), operation);
  const reordered = { ...operation, command: Object.fromEntries(Object.entries(command).reverse()) };
  assert.deepEqual(parseOperation(reordered), operation, "actual peer parser restores Rust typed command order");
  assert.throws(() => parseOperation({ ...operation, command: { ...command, kind: "changed" } }));
  assert.throws(() => parseOperation({ ...operation, deadline: new Date(deadline).toISOString() }));
  assert.throws(() => parseOperation({ ...operation, extra: true }));
  assert.match(deadline, /\.123456Z$/);
  assert.equal(digest(command, deadline), payload_digest, `${command.kind}: preserve exact deadline string`);
  assert.notEqual(digest(command, new Date(deadline).toISOString()), payload_digest,
    `${command.kind}: Date round trip must demonstrate microsecond loss`);
  assert.notEqual(digest({ ...command, kind: "changed" }, deadline), payload_digest,
    `${command.kind}: changed command is rejected`);
  assert.notEqual(digest(command, deadline.replace("123456", "123457")), payload_digest,
    `${command.kind}: changed deadline is rejected`);
  if (command.prompt !== undefined) {
    assert(command.prompt.includes("é😀\n\t\r\"\\\u0001"));
    assert.notEqual(digest({ ...command, prompt: `${command.prompt}!` }, deadline), payload_digest);
  }
}
process.stdout.write("PASS Rust→Node command hashes: all 6 kinds, exact microseconds, escaping/Unicode, tampering and lossy Date negatives\n");
