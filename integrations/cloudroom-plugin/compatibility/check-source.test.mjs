import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync, readFileSync, symlinkSync, chmodSync } from "node:fs";
import { createHash } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { verifySource, prepareSource, SOURCE_PATCH_LIMITS } from "./check-source.mjs";

test('bundled public SDK declarations match the pinned source and all approved patches', () => {
  const root = fileURLToPath(new URL('../', import.meta.url));
  const declaration = JSON.parse(readFileSync(join(root, 'types/manifest.json'), 'utf8'));
  const manifest = JSON.parse(readFileSync(join(root, 'compatibility/manifest.json'), 'utf8'));
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  assert.equal(hash(readFileSync(join(root, 'types/bb-plugin-sdk.d.ts'))), declaration.declarationSha256);
  assert.equal(declaration.revision, manifest.cloudroom.revision);
  assert.equal(declaration.sdk, manifest.sdk.version);
  assert.deepEqual(declaration.patches, manifest.sourcePatches.map(patch => ({ sha256: patch.sha256,
    targets: patch.targets.map(target => ({ sourcePath: target.sourcePath, sourceSha256: target.afterSha256 })) })));
  for (const patch of manifest.sourcePatches) assert.equal(hash(readFileSync(join(root, 'compatibility', patch.path))), patch.sha256);
});

test('multiple source patches validate all inputs before mutation and accept partial preparation', () => {
  const fixture = mkdtempSync(join(tmpdir(), 'openbot-multi-patch-'));
  const patches = mkdtempSync(join(tmpdir(), 'openbot-multi-input-'));
  const git = args => execFileSync('git', ['-C', fixture, ...args], { encoding: 'utf8', stdio: 'pipe' }).trim();
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  try {
    git(['init', '--quiet']); mkdirSync(join(fixture, 'gui'));
    for (const name of ['one', 'two']) writeFileSync(join(fixture, `gui/${name}.ts`), `export const ${name} = false;\n`);
    git(['add', '.']); git(['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'core.hooksPath=/dev/null', 'commit', '--quiet', '--no-gpg-sign', '-m', 'fixture']);
    const revision = git(['rev-parse', 'HEAD']);
    const descriptors = ['one', 'two'].map(name => {
      const before = `export const ${name} = false;\n`; const after = `export const ${name} = true;\n`;
      writeFileSync(join(fixture, `gui/${name}.ts`), after);
      const patch = `${git(['diff', '--full-index', '--binary', '--', `gui/${name}.ts`])}\n`;
      writeFileSync(join(fixture, `gui/${name}.ts`), before); writeFileSync(join(patches, `${name}.patch`), patch);
      return { path: `${name}.patch`, sha256: hash(patch), targets: [{sourcePath: `gui/${name}.ts`, beforeSha256: hash(before), afterSha256: hash(after)}] };
    });
    const options = { revision, patches: descriptors, patchRoot: patches };
    writeFileSync(join(patches, 'two.patch'), 'tampered');
    assert.throws(() => prepareSource(fixture, options), /tampered/);
    assert.equal(readFileSync(join(fixture, 'gui/one.ts'), 'utf8'), 'export const one = false;\n');
    // Regenerate the second exact patch, leave the first already applied.
    writeFileSync(join(fixture, 'gui/two.ts'), 'export const two = true;\n');
    writeFileSync(join(patches, 'two.patch'), `${git(['diff', '--full-index', '--binary', '--', 'gui/two.ts'])}\n`);
    writeFileSync(join(fixture, 'gui/two.ts'), 'export const two = false;\n');
    writeFileSync(join(fixture, 'gui/one.ts'), 'export const one = true;\n');
    const identity = prepareSource(fixture, options); assert.equal(identity.patches.length, 2);
    assert.deepEqual(prepareSource(fixture, options), identity);
    assert.throws(() => prepareSource(fixture, { ...options, patches: [descriptors[0], descriptors[0]] }), /overlapping/);
  } finally { rmSync(fixture, { recursive: true, force: true }); rmSync(patches, { recursive: true, force: true }); }
});

test("source verification rejects changed tracked GUI source before any build, from any cwd", () => {
  const fixture = mkdtempSync(join(tmpdir(), "openbot-source-guard-"));
  const git = (args) => execFileSync("git", ["-C", fixture, ...args], { encoding: "utf8", stdio: "pipe" }).trim();
  try {
    git(["init", "--quiet"]);
    mkdirSync(join(fixture, "gui"));
    writeFileSync(join(fixture, "gui/package.json"), "{}\n");
    git(["add", "gui/package.json"]);
    git(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "core.hooksPath=/dev/null", "commit", "--quiet", "--no-gpg-sign", "-m", "fixture"]);
    const head = git(["rev-parse", "HEAD"]);
    assert.equal(verifySource(fixture, head), head);
    assert.throws(() => verifySource(join(fixture, "gui"), head), /checkout root/);
    assert.throws(() => verifySource(fixture, "0".repeat(40)), /wrong Cloudroom revision/);
    writeFileSync(join(fixture, "gui/package.json"), '{"modified":true}\n');
    // The old guard falsely passed this exact change when invoked from gui/.
    assert.doesNotThrow(() => execFileSync("git", ["diff", "--exit-code", "HEAD", "--", "gui"], { cwd: join(fixture, "gui"), stdio: "pipe" }));
    assert.throws(() => verifySource(fixture, head));
  } finally { rmSync(fixture, { recursive: true, force: true }); }
});

test("prepared source accepts only the exact approved patch and rejects tampering", () => {
  const fixture = mkdtempSync(join(tmpdir(), "openbot-patched-source-"));
  const patches = mkdtempSync(join(tmpdir(), "openbot-patch-input-"));
  const git = (args) => execFileSync("git", ["-C", fixture, ...args], { encoding: "utf8", stdio: "pipe" }).trim();
  const hash = (value) => createHash("sha256").update(value).digest("hex");
  try {
    git(["init", "--quiet"]);
    mkdirSync(join(fixture, "gui"));
    writeFileSync(join(fixture, "gui/package.json"), "{}\n");
    writeFileSync(join(fixture, "gui/other.ts"), "export const base = true;\n");
    git(["add", "."]);
    git(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "core.hooksPath=/dev/null", "commit", "--quiet", "--no-gpg-sign", "-m", "fixture"]);
    const revision = git(["rev-parse", "HEAD"]);
    const after = '{"approved":true}\n';
    writeFileSync(join(fixture, "gui/package.json"), after);
    const patchText = git(["diff", "--full-index", "--binary"]) + "\n";
    writeFileSync(join(fixture, "gui/package.json"), "{}\n");
    writeFileSync(join(patches, "approved.patch"), patchText);
    const patch = { path: "approved.patch", sha256: hash(patchText), targets: [{sourcePath: "gui/package.json", beforeSha256: hash("{}\n"), afterSha256: hash(after)}] };
    const options = { revision, patchRoot: patches, patches: [patch] };
    assert.throws(() => prepareSource(fixture, { ...options, revision: "0".repeat(40) }), /wrong Cloudroom revision/);
    assert.equal(prepareSource(fixture, options).patches[0].sha256, patch.sha256);
    assert.equal(readFileSync(join(fixture, "gui/package.json"), "utf8"), after);
    assert.deepEqual(prepareSource(fixture, options), prepareSource(fixture, options));
    writeFileSync(join(fixture, "gui/package.json"), '{"unapproved":true}\n');
    assert.throws(() => prepareSource(fixture, options), /source differs/);
    writeFileSync(join(fixture, "gui/package.json"), after);
    writeFileSync(join(fixture, "gui/other.ts"), "export const base = false;\n");
    assert.throws(() => prepareSource(fixture, options), /unapproved source edits/);
    writeFileSync(join(fixture, "gui/other.ts"), "export const base = true;\n");
    writeFileSync(join(fixture, "gui/untracked.ts"), "export const unapproved = true;\n");
    assert.throws(() => prepareSource(fixture, options), /untracked/);
    rmSync(join(fixture, "gui/untracked.ts"));
    writeFileSync(join(patches, "approved.patch"), patchText + "tampered");
    assert.throws(() => prepareSource(fixture, options), /tampered/);
  } finally { rmSync(fixture, { recursive: true, force: true }); rmSync(patches, { recursive: true, force: true }); }
});


test('cohesive source patch adds absent inputs and refuses partial or unsafe preparation', () => {
  const fixture = mkdtempSync(join(tmpdir(), 'openbot-cohesive-'));
  const patches = mkdtempSync(join(tmpdir(), 'openbot-cohesive-input-'));
  const git = args => execFileSync('git', ['-C', fixture, ...args], {encoding: 'utf8', stdio: 'pipe'}).trim();
  const hash = bytes => createHash('sha256').update(bytes).digest('hex');
  try {
    git(['init', '--quiet']); mkdirSync(join(fixture, 'gui'));
    writeFileSync(join(fixture, 'gui/base.ts'), 'before\n'); git(['add', '.']);
    git(['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'core.hooksPath=/dev/null', 'commit', '--quiet', '--no-gpg-sign', '-m', 'fixture']);
    const revision = git(['rev-parse', 'HEAD']);
    writeFileSync(join(fixture, 'gui/base.ts'), 'after\n');
    writeFileSync(join(fixture, 'gui/added.sql'), 'SELECT 1;\n'); git(['add', '-N', 'gui/added.sql']);
    const patchText = git(['diff', '--full-index', '--binary']) + '\n';
    git(['reset', '--mixed', '--quiet']); git(['checkout', '--', 'gui/base.ts']); rmSync(join(fixture, 'gui/added.sql'));
    writeFileSync(join(patches, 'cohesive.patch'), patchText);
    const patch = {path: 'cohesive.patch', sha256: hash(patchText), targets: [
      {sourcePath: 'gui/base.ts', beforeSha256: hash('before\n'), afterSha256: hash('after\n')},
      {sourcePath: 'gui/added.sql', beforeSha256: null, afterSha256: hash('SELECT 1;\n')},
    ]};
    const options = {revision, patches: [patch], patchRoot: patches};
    writeFileSync(join(fixture, 'gui/base.ts'), 'after\n');
    assert.throws(() => prepareSource(fixture, options), /partially applied/);
    assert.throws(() => readFileSync(join(fixture, 'gui/added.sql')), /ENOENT/);
    git(['checkout', '--', 'gui/base.ts']);
    const identity = prepareSource(fixture, options);
    assert.equal(readFileSync(join(fixture, 'gui/added.sql'), 'utf8'), 'SELECT 1;\n');
    assert.deepEqual(prepareSource(fixture, options), identity);
    chmodSync(join(fixture, 'gui/added.sql'), 0o755);
    assert.throws(() => prepareSource(fixture, options), /mode/); chmodSync(join(fixture, 'gui/added.sql'), 0o644);
    rmSync(join(fixture, 'gui/added.sql')); symlinkSync('base.ts', join(fixture, 'gui/added.sql'));
    assert.throws(() => prepareSource(fixture, options), /mode/);
    const oversized = {...patch, targets: Array.from({length: SOURCE_PATCH_LIMITS.targets + 1}, (_, index) =>
      ({sourcePath: `gui/file${index}.ts`, beforeSha256: null, afterSha256: hash('after')}))};
    assert.throws(() => prepareSource(fixture, {...options, patches: [oversized]}), /bounded source patch targets/);
  } finally { rmSync(fixture, {recursive: true, force: true}); rmSync(patches, {recursive: true, force: true}); }
});

test('pinned compatibility extension retains 75 percent source capacity headroom', () => {
  const root = fileURLToPath(new URL('./', import.meta.url));
  const manifest = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8'));
  assert(manifest.sourcePatches.length <= SOURCE_PATCH_LIMITS.patches * .75);
  assert(manifest.sourcePatches.reduce((sum, patch) => sum + patch.targets.length, 0) <= SOURCE_PATCH_LIMITS.targets * .75,
    'shrink or deliberately recalibrate the compatibility extension before raising its bound');
  for (const patch of manifest.sourcePatches) assert(readFileSync(join(root, patch.path)).length <= SOURCE_PATCH_LIMITS.patchBytes * .75);
});
