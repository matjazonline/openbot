import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { realpathSync, readFileSync, lstatSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

export const CLOUDROOM_REVISION = "d8b3cc3a16bb4b06fc99d8e543d40f1087d1e815";
const compatibilityRoot = dirname(fileURLToPath(import.meta.url));

function checkedGit(checkout, revision) {
  const root = realpathSync(checkout);
  const git = (args) => execFileSync("git", ["-C", root, ...args], {
    encoding: "utf8", timeout: 10_000, maxBuffer: 64 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
  assert.equal(realpathSync(git(["rev-parse", "--show-toplevel"])), root, "pass the checkout root");
  assert.equal(git(["rev-parse", "HEAD"]), revision, "wrong Cloudroom revision");
  return { root, git };
}

export function verifySource(checkout, expectedRevision = CLOUDROOM_REVISION) {
  const { git } = checkedGit(checkout, expectedRevision);
  // Explicit repository root: callers may run from gui/. Quiet avoids logging a private diff.
  git(["diff", "--quiet", "HEAD", "--", "gui/"]);
  return expectedRevision;
}

function inspectPatch(root, patchRoot, patch, git) {
  const hash = (value) => createHash("sha256").update(value).digest("hex");
  assert(patch && patch.sourcePath.startsWith("gui/") && !patch.sourcePath.split("/").includes(".."));
  const patchPath = realpathSync(resolve(patchRoot, patch.path));
  assert(patchPath.startsWith(realpathSync(patchRoot) + sep), "patch escapes fixture root");
  assert.equal(hash(readFileSync(patchPath)), patch.sha256, "tampered source patch");
  const target = resolve(root, patch.sourcePath);
  assert(realpathSync(target).startsWith(root + sep), "source path escapes checkout");
  assert(lstatSync(target).isFile() && (lstatSync(target).mode & 0o111) === 0, "unexpected source file mode");
  assert.deepEqual(git(["apply", "--numstat", patchPath]).split("\n").map((line) => line.split("\t")[2]),
    [patch.sourcePath], "patch changes unapproved paths");
  const current = hash(readFileSync(target));
  assert(current === patch.beforeSha256 || current === patch.afterSha256, "source differs from approved patch");
  return { patch, patchPath, target, needed: current === patch.beforeSha256 };
}

export function prepareSource(checkout, {
  revision = CLOUDROOM_REVISION,
  patches = JSON.parse(readFileSync(resolve(compatibilityRoot, "manifest.json"), "utf8")).sourcePatches,
  patchRoot = compatibilityRoot,
} = {}) {
  const { root, git } = checkedGit(checkout, revision);
  assert(Array.isArray(patches) && patches.length > 0 && patches.length <= 8, "bounded source patches required");
  const changed = () => git(["diff", "--name-only", "HEAD"]).split("\n").filter(Boolean).sort();
  const untracked = () => git(["ls-files", "--others", "--exclude-standard", "--", "gui/"]);
  assert.equal(untracked(), "", "unexpected untracked GUI inputs");
  const allowed = patches.map(({ sourcePath }) => sourcePath).sort();
  assert.equal(new Set(allowed).size, allowed.length, "overlapping source patches");
  assert(changed().every((path) => allowed.includes(path)), "unapproved source edits");
  // Validate every input before applying any dependency patch.
  const prepared = patches.map((patch) => inspectPatch(root, patchRoot, patch, git));
  for (const { needed, patchPath } of prepared) if (needed) git(["apply", "--check", patchPath]);
  for (const { needed, patchPath } of prepared) if (needed) git(["apply", "--whitespace=error", patchPath]);
  const hash = (value) => createHash("sha256").update(value).digest("hex");
  for (const { patch, target } of prepared) assert.equal(hash(readFileSync(target)), patch.afterSha256, "source differs from approved patch");
  assert.deepEqual(changed(), allowed, "unapproved source edits");
  assert.equal(untracked(), "", "unexpected untracked GUI inputs");
  const identity = { revision, patches: prepared.map(({ patch }) => ({
    sha256: patch.sha256, sourcePath: patch.sourcePath, sourceSha256: patch.afterSha256,
  })) };
  return { ...identity, artifactDigest: hash(JSON.stringify(identity)) };
}
