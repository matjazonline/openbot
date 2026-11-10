// Operates only on explicitly supplied disposable native-smoke threads.
import assert from "node:assert/strict";
import { chromium, expect } from "@playwright/test";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { localCallerHeaders } from "./local-caller.mjs";
import { restrictToProbeServers } from "./prove-ui.mjs";

assert.equal(process.env.OPENBOT_CLOUDROOM_NATIVE_SMOKE, "1", "native smoke requires explicit opt-in");
const [origin, backendOrigin, profile, projectId, threadId, action, evidence] = process.argv.slice(2);
assert.equal(process.argv.length, 9);
assert.match(projectId, /^proj_[a-z0-9]+$/);
assert.match(threadId, /^thr_[a-z0-9]+$/);
assert(["answer", "deny"].includes(action));
for (const value of [origin, backendOrigin]) {
  const url = new URL(value);
  assert.equal(url.protocol, "http:");
  assert.equal(url.hostname, "127.0.0.1");
}
const browser = await chromium.launch({ headless: true });
let page;
const refused = [];
try {
  const context = await browser.newContext({ extraHTTPHeaders: await localCallerHeaders(backendOrigin, profile) });
  await restrictToProbeServers(context, [origin, backendOrigin]);
  page = await context.newPage();
  page.on("response", (response) => {
    if (response.status() >= 400 && refused.length < 16) refused.push({
      path: new URL(response.url()).pathname, status: response.status(),
    });
  });
  await page.goto(`${origin}/projects/${projectId}/threads/${threadId}`, { waitUntil: "domcontentloaded" });
  await expect(page.getByRole("button", { name: "Set up Cloudroom", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Skip setup", exact: true })).toHaveCount(0);
  const button = action === "answer" ? "Submit answer" : "Deny";
  if (action === "deny") {
    const deny = page.getByRole("button", { name: "Deny", exact: true });
    const approval = page.getByRole("button", { name: "Approval needed", exact: true });
    await expect(deny.or(approval).first()).toBeVisible({ timeout: 30_000 });
    if (!(await deny.isVisible())) await approval.click();
  }
  await expect(page.getByRole("button", { name: button, exact: true })).toBeVisible({ timeout: 30_000 });
  await page.screenshot({ path: resolve(evidence, `${action}-before.png`), fullPage: true });
  if (action === "answer") await page.getByRole("button", { name: /^Continue smoke/ }).click();
  const response = page.waitForResponse((res) => {
    const url = new URL(res.url());
    return url.pathname.startsWith(`/api/v1/threads/${threadId}/interactions/`)
      && res.request().method() === "POST";
  });
  await page.getByRole("button", { name: button, exact: true }).click();
  assert((await response).ok(), "real interaction submission failed");
  await expect(page.getByRole("button", { name: button, exact: true })).toHaveCount(0, { timeout: 30_000 });
  await page.screenshot({ path: resolve(evidence, `${action}-after.png`), fullPage: true });
  await writeFile(resolve(evidence, `${action}-ui.json`), JSON.stringify({
    threadId, projectId, action, actualUi: true, hostedAccount: false, apiStandIns: false,
    realInteractionSubmitted: true,
  }, null, 2) + "\n");
} catch (error) {
  if (page) {
    await page.screenshot({ path: resolve(evidence, `${action}-failure.png`), fullPage: true });
    await writeFile(resolve(evidence, `${action}-failure.json`), JSON.stringify({
      threadId, action, refused,
      message: String(error.message).slice(0, 2000),
      buttons: (await page.getByRole("button").allTextContents()).slice(0, 80).map(text => text.slice(0, 160)),
    }, null, 2) + "\n");
  }
  throw error;
} finally { await browser.close(); }
