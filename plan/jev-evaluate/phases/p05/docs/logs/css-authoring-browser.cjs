// Full p09 admin journey against an explicitly selected disposable application server.
const { chromium } = require('playwright-core');
const { expect } = require('@playwright/test');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const BASE = process.env.WORKFLOW_TEST_URL || 'http://localhost:3001';
const OUT = process.env.OUT || fs.mkdtempSync(path.join(os.tmpdir(), 'workflow-authoring-'));
fs.mkdirSync(OUT, { recursive: true });
(async () => {
    const browser = await chromium.launch({ channel: 'chrome', headless: true });
    const page = await browser.newPage({viewport:{width:1400,height:1000}});
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    let dialogAnswer='accept';const dialogs=[];
    page.on('dialog',dialog=>{dialogs.push(dialog.message());return dialogAnswer==='accept'?dialog.accept():dialog.dismiss();});
    try {
        const user='p09sample'+crypto.randomBytes(4).toString('hex');
        const password=crypto.randomBytes(20).toString('base64url');
        await page.goto(BASE+'/register');
        await page.locator('[name=username]').fill(user);
        await page.locator('[name=email]').fill(user+'@example.test');
        await page.locator('[name=password]').fill(password);
        if(await page.locator('[name=confirm_password]').count()) await page.locator('[name=confirm_password]').fill(password);
        await page.locator('form [type=submit]').first().click();await page.waitForLoadState('networkidle');
        if(page.url().includes('/login')||page.url().endsWith('/register')) {
            await page.goto(BASE+'/login');await page.locator('[name=email_or_username]').fill(user);
            await page.locator('[name=password]').fill(password);await page.locator('form [type=submit]').first().click();await page.waitForLoadState('networkidle');
        }
        await page.goto(BASE+'/ui/companies');
        await page.evaluate(async slug=>{const response=await fetch('/ui/companies',{method:'POST',body:new URLSearchParams({name:'Sample browser',slug,connection_0_provider:'openai',connection_0_models:'gpt-test',connection_0_api_key:'fixture-only-not-a-key',default_model_provider:'0'}),headers:{'HX-Request':'true'}});if(!response.ok)throw Error('company creation failed');},user);
        await page.goto(BASE+'/ui/companies');
        const company=(await page.content()).match(/company_id=([0-9a-f-]{36})/)[1];
        await page.goto(`${BASE}/ui/companies?company_id=${company}`);
        const connections=page.locator('#typesafe-connections');
        await expect(connections.getByRole('heading',{name:'New connection'})).toBeVisible();
        const typesafeComputed=[];
        for(const theme of ['dark','light']) {
            await page.evaluate(theme=>{const toggle=document.getElementById('theme-toggle');toggle.checked=theme==='light';toggle.dispatchEvent(new Event('change',{bubbles:true}));},theme);
            await connections.locator('[name=models]').focus();
            const styles=await connections.locator('[name=models]').evaluate(element=>{const c=getComputedStyle(element);return {theme:document.documentElement.dataset.theme,radius:c.borderRadius,outline:c.outlineOffset,color:c.color,display:c.display};});
            expect(styles.theme).toBe(theme);expect(styles.radius).not.toBe('0px');expect(styles.outline).toBe('-1px');expect(styles.display).not.toBe('none');typesafeComputed.push(styles);
        }
        fs.writeFileSync(path.join(OUT,'typesafe-computed.json'),JSON.stringify(typesafeComputed,null,2));
        const createConnection=connections.locator('form').last();
        await createConnection.locator('[name=api_key]').fill('browser-typesafe-secret');
        await createConnection.getByRole('button',{name:'Create connection'}).click();
        await expect(connections.getByText(/ready · revision 1/)).toBeVisible();
        expect(await connections.innerHTML()).not.toContain('browser-typesafe-secret');
        const rotateConnection=connections.locator('form[hx-post$="/rotate"]');
        await rotateConnection.locator('[name=api_key]').fill('browser-typesafe-rotated');
        await rotateConnection.getByRole('button',{name:'Rotate API key'}).click();
        await expect(connections.getByText(/ready · revision 2/)).toBeVisible();
        expect(await connections.innerHTML()).not.toContain('browser-typesafe-rotated');
        const updateConnection=connections.locator('form').first();
        await updateConnection.locator('[name=enabled]').uncheck();
        await updateConnection.getByRole('button',{name:'Save models and availability'}).click();
        await expect(connections.getByText(/not ready · revision 3/)).toBeVisible();
        await page.evaluate(async company=>{
            const response=await fetch(`/ui/agents?company_id=${company}`,{method:'POST',body:new URLSearchParams({name:'Assistant',slug:'assistant',provider:'openai',model:'gpt-test',system_prompt:'Draft a helpful response.'})});
            if(!response.ok)throw Error('agent setup failed');
        },company);
        await page.goto(`${BASE}/ui/workflows?company_id=${company}&new=1`);
        await page.locator('form[hx-post*="templates/reviewed-support/copy"] button').focus();
        await page.keyboard.press('Enter');await expect(page.locator('#workflow-source')).toBeVisible();
        const route=await page.locator('form[data-workflow-editor]').getAttribute('hx-post');
        const workflow=route.match(/workflows\/([0-9a-f-]{36})/)[1];
        const source=(await page.locator('#workflow-source').inputValue())+'\n# Browser authoring journey\n';
        const sourceMessage=crypto.randomUUID();
        const sampleInput=JSON.stringify({message:'Please help',source_message:sourceMessage});
        const sampleParams=JSON.stringify({reviewer:crypto.randomUUID()});
        const scripts=JSON.stringify([
            {workflow_id:workflow,step:'draft',responses:[{output:{body:'First draft'}}]},
            {workflow_id:workflow,step:'review',responses:[{output:{choice:'approve',data:{body:'Browser sample passed'},feedback:''}}]},
            {workflow_id:workflow,step:'reply',responses:[{output:{status:'accepted',messages:[{destination:{kind:'reply',id:sourceMessage},message_id:crypto.randomUUID(),status:'accepted'}]}}]}
        ]);
        // Keyboard-only editor diagnostics, disclosure and dirty confirmation in each theme.
        for(const theme of ['dark','light']) {
            await page.evaluate(theme=>{const toggle=document.getElementById('theme-toggle');toggle.checked=theme==='light';toggle.dispatchEvent(new Event('change',{bubbles:true}));},theme);
            await page.locator('#workflow-source').fill(source.replace('type: agent.run','type: missing.step'));
            await page.getByRole('button',{name:'Save and validate',exact:true}).focus();await page.keyboard.press('Enter');
            const go=page.getByRole('button',{name:/Go to line/});await expect(go).toBeVisible();
            await go.focus();await page.keyboard.press('Enter');await expect(page.locator('#workflow-source')).toBeFocused();
            const selected=await page.locator('#workflow-source').evaluate(field=>field.selectionEnd>field.selectionStart);
            expect(selected).toBe(true);
            const reference=page.locator('#workflow-reference > summary');await reference.focus();await page.keyboard.press('Enter');
            await expect(page.locator('#workflow-reference')).toHaveAttribute('open','');
            await expect(page.locator('#workflow-reference-body')).toContainText('data.map');
            await page.locator('#workflow-source').fill(source);
            const beforeDialogs=dialogs.length;dialogAnswer='dismiss';
            await page.getByRole('tab',{name:'Sample',exact:true}).focus();await page.keyboard.press('Enter');
            await expect.poll(()=>dialogs.length).toBe(beforeDialogs+1);
            await expect(page.locator('#workflow-source')).toHaveValue(source);
            await expect(page.getByRole('tab',{name:'Editor',exact:true})).toHaveAttribute('aria-selected','true');
            dialogAnswer='accept';
            await page.getByRole('button',{name:'Save and validate',exact:true}).focus();await page.keyboard.press('Enter');
            await expect(page.getByText(/Revision \d+ is valid/)).toBeVisible();
        }
        await page.locator('[name=title]').fill('Sample journey');
        await page.locator('#workflow-source').fill(source);
        await page.getByRole('button',{name:'Save and validate',exact:true}).focus();await page.keyboard.press('Enter');
        await expect(page.getByText(/Revision \d+ is valid/)).toBeVisible();
        await page.getByRole('tab',{name:'Sample',exact:true}).focus();await page.keyboard.press('Enter');
        await expect(page.locator('#workflow-sample-form')).toBeVisible();
        await page.locator('#workflow-sample-form [name=params]').fill(sampleParams);
        await page.locator('#workflow-sample-form [name=scripts]').fill(scripts);
        const colors=[];
        for(const theme of ['dark','light']) {
            await page.evaluate(theme=>{const toggle=document.getElementById('theme-toggle');toggle.checked=theme==='light';toggle.dispatchEvent(new Event('change',{bubbles:true}));},theme);
            const input=page.locator('#workflow-sample-form [name=input]');await input.focus();await page.keyboard.press('Tab');
            await expect(page.locator('#workflow-sample-form [name=params]')).toBeFocused();
            colors.push(await page.locator('#workflow-sample-form [name=params]').evaluate(element=>({theme:document.documentElement.dataset.theme,color:getComputedStyle(element).color,radius:getComputedStyle(element).borderRadius,outline:getComputedStyle(element).outlineOffset})));
            await input.fill('\n</textarea><img src=x onerror=alert(1)>');
            await page.getByRole('button',{name:'Run sample',exact:true}).click();
            await expect(page.getByText(/Invalid sample JSON/)).toBeVisible();
            await expect(input).toHaveValue('\n</textarea><img src=x onerror=alert(1)>');
            await expect(page.locator('#workflow-pane img')).toHaveCount(0);
            await input.fill(sampleInput);
            // Hold a real response to inspect progress, then allow the real server report through.
            let release;
            await page.route('**/ui/workflows/*/sample?*',async route=>{const response=await route.fetch();await new Promise(resolve=>release=resolve);await route.fulfill({response});},{times:1});
            await page.getByRole('button',{name:'Run sample',exact:true}).focus();await page.keyboard.press('Enter');
            await expect(page.locator('#workflow-sample-form')).toHaveAttribute('aria-busy','true');
            await expect(page.locator('#workflow-sample-form button[type=submit]')).toBeDisabled();
            await expect.poll(()=>typeof release).toBe('function');release();
            await expect(page.getByRole('status').filter({hasText:'Sample completed'})).toBeVisible();
            await expect(page.locator('#sample-report')).toContainText('Browser sample passed');
            await page.screenshot({path:path.join(OUT,`sample-${theme}.png`)});
        }
        expect(colors[0].color).not.toBe(colors[1].color);
        expect(colors.every(style=>style.outline==='-1px')).toBe(true);
        // A delayed sample must not undo a later tab selection.
        let release;
        await page.route('**/ui/workflows/*/sample?*',async route=>{const response=await route.fetch();await new Promise(resolve=>release=resolve);await route.fulfill({response});},{times:1});
        await page.getByRole('button',{name:'Run sample',exact:true}).click();await expect.poll(()=>typeof release).toBe('function');
        await page.getByRole('tab',{name:'Versions and usage',exact:true}).click();
        await expect(page.locator('#workflow-sample-form')).toHaveCount(0);release();await page.waitForTimeout(200);
        await expect(page.getByRole('tab',{name:'Versions and usage'})).toHaveAttribute('aria-selected','true');
        await page.getByRole('tab',{name:'Editor',exact:true}).click();
        await page.getByRole('button',{name:'Save and validate',exact:true}).click();
        await expect(page.getByText(/Revision \d+ is valid/)).toBeVisible();
        await page.getByRole('button',{name:/Publish/}).focus();await page.keyboard.press('Enter');
        await expect(page.getByText(/Published version/).first()).toBeVisible();
        await page.getByRole('tab',{name:'Bindings',exact:true}).click();
        await page.waitForTimeout(100); // Let HTMX finish processing the newly swapped form.
        const reviewer=await page.locator('#binding-people option').first().getAttribute('value');
        await page.locator('[name="param:reviewer"]').fill(reviewer);
        await page.getByRole('button',{name:'Save binding',exact:true}).focus();await page.keyboard.press('Enter');
        await expect(page.getByRole('button',{name:'Activate',exact:true})).toBeVisible();
        await page.getByRole('button',{name:'Activate',exact:true}).focus();await page.keyboard.press('Enter');
        await expect(page.getByRole('button',{name:'Deactivate',exact:true})).toBeVisible();
        expect(errors).toEqual([]);
        fs.writeFileSync(path.join(OUT,'result.json'),JSON.stringify({ok:true,company,workflow,colors},null,2));
        console.log('PASS copy/edit/validate/sample/publish/activate; keyboard, both themes, progress, escaping, stale response');
    } catch(error) {
        console.error('Page errors:',errors);
        await page.screenshot({path:path.join(OUT,'failure.png')});fs.writeFileSync(path.join(OUT,'failure.html'),await page.content());throw error;
    } finally {await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
