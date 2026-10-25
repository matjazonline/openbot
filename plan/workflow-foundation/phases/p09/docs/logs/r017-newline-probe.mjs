import { readFileSync } from 'node:fs';
import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';
const source = readFileSync('/Users/mac02/dev/openbot/src/adapters/http/pages/workflow_authoring/sample.rs','utf8');
const template = source.match(/<textarea name="\{name\}"[^>]*>\{raw\}<\/textarea>/)[0];
const raw = '\n{invalid JSON';
const html = template.replace('{name}','input').replace('{raw}',raw);
const browser = await chromium.launch({executablePath:process.env.CHROME_PATH});
try {
 const page = await browser.newPage();
 await page.setContent(html);
 const observed = await page.locator('textarea').inputValue();
 assert.equal(observed,raw.slice(1));
 console.log(JSON.stringify({expected:raw,observed,defect:'leading newline stripped from returned raw sample input'}));
} finally {await browser.close();}
