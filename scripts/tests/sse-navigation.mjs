import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import vm from 'node:vm';
import { test } from 'node:test';

const source = readFileSync(new URL('../../src/adapters/http/pages/mailbox.rs', import.meta.url), 'utf8');
const script = source.split('const EVENT_DELEGATION_SCRIPT: &str = r##"')[1].split('document.addEventListener')[0];

function navigate(href, event = {}, attributes = {}) {
    const owners = [{ closed: false }, { closed: false }];
    let click;
    const link = {
        href,
        hasAttribute: name => Object.hasOwn(attributes, name),
        getAttribute: name => name === 'href' ? href : attributes[name] ?? null,
    };
    vm.runInNewContext(script, {
        URL,
        document: {
            baseURI: 'https://example.test/ui/tasks?view=board',
            querySelector: () => null,
            querySelectorAll: () => owners,
        },
        window: {
            location: { href: 'https://example.test/ui/tasks?view=board' },
            addEventListener: (name, handler) => { if (name === 'click') click = handler; },
            htmx: { trigger: (owner, name) => {
                assert.equal(name, 'htmx:beforeCleanupElement');
                owner.closed = true;
            } },
        },
    });
    click({ button: 0, target: { closest: () => link }, ...event });
    return owners.every(owner => owner.closed);
}

test('ordinary task links release every stream before navigation', () => {
    for (const href of ['/ui/tasks?view=list', '/ui/tasks?view=board', '/ui?thread_id=123']) {
        assert.equal(navigate(href), true, href);
    }
});
test('HTMX/prevented clicks, modified clicks, downloads and new tabs keep streams', () => {
    for (const event of [{ defaultPrevented: true }, { button: 1 }, { ctrlKey: true },
        { metaKey: true }, { shiftKey: true }, { altKey: true }]) {
        assert.equal(navigate('/ui/tasks?view=list', event), false);
    }
    for (const attributes of [{ download: '' }, { target: '_blank' }]) {
        assert.equal(navigate('/ui/tasks?view=list', {}, attributes), false);
    }
});
test('anchors and non-document protocols keep streams', () => {
    assert.equal(navigate('#details'), false);
    assert.equal(navigate('#'), false);
    assert.equal(navigate('mailto:hello@example.test'), false);
    assert.equal(navigate('/ui/tasks?view=list#details'), true);
});

function submitForm({ prevented = false, attributes = {}, submitter = {}, baseTarget = null } = {}) {
    const owners = [{ closed: false }, { closed: false }];
    let submit;
    vm.runInNewContext(script, {
        document: {
            querySelector: () => baseTarget === null ? null : { getAttribute: () => baseTarget },
            querySelectorAll: () => owners,
        },
        window: {
            addEventListener: (name, handler) => { if (name === 'submit') submit = handler; },
            htmx: { trigger: (owner, name) => {
                assert.equal(name, 'htmx:beforeCleanupElement');
                owner.closed = true;
            } },
        },
    });
    submit({
        defaultPrevented: prevented,
        target: { getAttribute: name => attributes[name] ?? null },
        submitter: { getAttribute: name => submitter[name] ?? null },
    });
    return owners.every(owner => owner.closed);
}

test('native owned-task filter submissions release all streams on every navigation', () => {
    for (let i = 0; i < 20; i++) {
        assert.equal(submitForm({ attributes: { method: 'get', action: '/ui/work' } }), true);
    }
});
test('cancelled, HTMX, dialog and other-window submissions retain streams', () => {
    assert.equal(submitForm({ prevented: true }), false);
    assert.equal(submitForm({ attributes: { method: 'dialog' } }), false);
    assert.equal(submitForm({ attributes: { target: '_blank' } }), false);
    assert.equal(submitForm({ submitter: { formtarget: '_blank' } }), false);
    assert.equal(submitForm({ submitter: { formmethod: 'dialog' } }), false);
    assert.equal(submitForm({ baseTarget: '_blank' }), false);
    assert.equal(submitForm({ attributes: { target: '_blank' }, submitter: { formtarget: '_self' } }), true);
});
