import { chromium, expect } from "@playwright/test";
import { writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { localCallerHeaders } from "./local-caller.mjs";

export async function restrictToProbeServers(context, origins) {
  const allowed = new Set(origins.filter(Boolean).map((value) => new URL(value).origin));
  const permitted = (value) => {
    const url = new URL(value);
    if (url.protocol === "ws:") url.protocol = "http:";
    if (url.protocol === "wss:") url.protocol = "https:";
    return allowed.has(url.origin);
  };
  await context.route("**/*", (route) => permitted(route.request().url()) ? route.continue() : route.abort());
  await context.routeWebSocket("**/*", (socket) => {
    if (permitted(socket.url())) socket.connectToServer();
    else socket.close();
  });
}

export async function proveUi({ origin, projectId, threadId, evidence,
  expectedText = "Disposable scheduled SDK probe; this must never execute.", backendOrigin, profile }) {
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({ extraHTTPHeaders: backendOrigin && profile
      ? await localCallerHeaders(backendOrigin, profile) : {} });
    await restrictToProbeServers(context, [origin, backendOrigin]);
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message.slice(0, 1000)));
    try {
    const threadRead = page.waitForResponse((response) =>
      new URL(response.url()).pathname === `/api/v1/threads/${threadId}` && response.ok(), { timeout: 45_000 });
    void threadRead.catch(() => {});
    await page.goto(`${origin}/projects/${projectId}/threads/${threadId}`, { waitUntil: "domcontentloaded", timeout: 45_000 });
    const rendered = await (await threadRead).json();
    expect(rendered.id).toBe(threadId);
    await expect(page.getByText(expectedText, { exact: true }).first()).toBeVisible({ timeout: 45_000 });
    await page.screenshot({ path: resolve(evidence, "ui-thread.png"), fullPage: true });
    await writeFile(resolve(evidence, "ui-thread.json"), JSON.stringify({
      threadId, projectId, actualThreadRead: rendered.id, promptVisible: true,
      assertedTextVisible: true, apiStandIns: false,
      localCallerProofFixture: Boolean(backendOrigin && profile),
    }, null, 2) + "\n");
    } catch (error) {
      await page.screenshot({ path: resolve(evidence, "ui-failure.png"), fullPage: true });
      await writeFile(resolve(evidence, "ui-failure.json"), JSON.stringify({ errors,
        location: page.url(), body: (await page.locator("body").innerText()).slice(0, 3000),
      }, null, 2) + "\n");
      throw error;
    }
  } finally { await browser.close(); }
}
