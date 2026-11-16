import assert from "node:assert/strict";
import {pathToFileURL} from "node:url";
const modulePath = process.env.ATTRIBUTION_PLAYWRIGHT_MODULE;
const {chromium} = modulePath ? await import(pathToFileURL(modulePath).href) : await import("playwright");
const openbot = process.env.ATTRIBUTION_FIXTURE_OPENBOT_ORIGIN;
const attribution = process.env.ATTRIBUTION_PUBLIC_ORIGIN;
let stage = "start browser";
let browser;
try {
  browser = await chromium.launch({headless: true});
  const context = await browser.newContext();
  const page = await context.newPage();
  stage = "new user login redirect";
  await page.goto(`${attribution}/auth/login`);
  await page.waitForURL(`${openbot}/login`);
  await page.locator('input[name="email_or_username"]').fill(process.env.ATTRIBUTION_FIXTURE_USERNAME);
  await page.locator('input[name="password"]').fill(process.env.ATTRIBUTION_FIXTURE_PASSWORD);
  stage = "existing Openbot password form";
  const form = page.locator("form").filter({has: page.locator('input[name="password"]')});
  await form.locator('button[type="submit"]').click();
  await page.waitForURL(`${attribution}/`);
  const readSession = () => page.evaluate(async () => {
    const response = await fetch("/v1/session");
    return {status: response.status, value: response.ok ? await response.json() : null};
  });
  const session = await readSession();
  assert.equal(session.status, 200);
  assert.equal(session.value.user_id, process.env.ATTRIBUTION_FIXTURE_USER_ID);
  assert.ok(session.value.csrf_token);
  stage = "independent secure cookies";
  const local = (await context.cookies(attribution)).find(cookie => cookie.name === "__Host-attribution");
  assert.ok(local && local.secure && local.httpOnly && local.sameSite === "Lax");
  assert.equal(local.domain, "127.0.0.1");
  assert.ok(!(await context.cookies(attribution)).some(cookie => cookie.name === "session"));
  assert.ok((await context.cookies(openbot)).some(cookie => cookie.name === "session"));
  assert.ok(!(await context.cookies(openbot)).some(cookie => cookie.name === "__Host-attribution"));
  stage = "company navigation";
  await page.locator('ul[aria-label="Companies"] a').first().click();
  await page.waitForURL(url => url.origin === attribution && url.searchParams.has("company_id"));
  assert.equal(await page.locator("h1").innerText(), "Browser fixture");
  stage = "local logout preserves Openbot login";
  await page.getByRole("button", {name: "Sign out"}).click();
  await page.getByRole("link", {name: "Sign in with Openbot"}).waitFor();
  stage = "already authenticated redirect and session rotation";
  await page.getByRole("link", {name: "Sign in with Openbot"}).click();
  await page.getByRole("button", {name: "Sign out"}).waitFor();
  const next = (await context.cookies(attribution)).find(cookie => cookie.name === "__Host-attribution");
  assert.ok(next && next.value !== local.value);
  assert.equal((await readSession()).status, 200);
  stage = "real upstream logout revokes attribution access";
  const logout = await context.request.post(`${openbot}/logout`, {headers: {Origin: openbot}, maxRedirects: 0});
  assert.equal(logout.status(), 303);
  assert.equal((await readSession()).status, 401);
  stage = "revoked session returns to sign-in";
  await page.reload();
  await page.getByRole("link", {name: "Sign in with Openbot"}).waitFor();
  assert.ok(!(await context.cookies(attribution)).some(cookie => cookie.name === "__Host-attribution"));
  process.stdout.write(JSON.stringify({checks: ["new-password-login", "already-authenticated-login", "independent-secure-cookies", "current-company-access", "local-logout", "session-rotation", "upstream-revocation", "session-recovery"], result: "PASS"}) + "\n");
} catch {
  // Playwright errors can include a callback URI with its code/state. Never retain those.
  process.stderr.write(`Attribution browser fixture failed at: ${stage}\n`);
  process.exitCode = 1;
} finally {
  await browser?.close();
}
