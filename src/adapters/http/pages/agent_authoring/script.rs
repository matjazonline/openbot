/// Delegated listeners survive partial navigation. Values enter text/attribute APIs, never HTML.
pub const AGENT_AUTHORING_SCRIPT: &str = r#"
function syncAgentToolRows(root) {
    var rows = Array.from(root.querySelectorAll('[data-agent-tool-row]'));
    var assignments = rows.map(function (row) {
        var assignment = JSON.parse(row.dataset.assignment);
        assignment.alias = row.querySelector('[data-agent-tool-alias]').value;
        return assignment;
    });
    root.querySelector('[name=tool_assignments]').value = JSON.stringify(assignments);
}
function addAgentToolRow(root, button) {
    var error = root.querySelector('[data-agent-tool-error]');
    if (root.querySelectorAll('[data-agent-tool-row]').length >= 32) {
        error.textContent = 'An agent can have at most 32 tools.';
        return;
    }
    var assignment = { binding: null, alias: button.dataset.alias, choice: JSON.parse(button.dataset.choice) };
    var row = document.createElement('div');
    row.className = 'my-2 rounded border border-base-300 p-3';
    row.dataset.agentToolRow = '';
    row.dataset.assignment = JSON.stringify(assignment);
    var label = document.createElement('label');
    label.className = 'form-control';
    label.textContent = 'Agent-visible alias';
    var input = document.createElement('input');
    input.className = 'input input-sm';
    input.dataset.agentToolAlias = '';
    input.value = assignment.alias;
    input.maxLength = 64;
    input.required = true;
    label.append(input);
    var target = document.createElement('p');
    target.className = 'break-all text-xs opacity-70';
    target.textContent = button.closest('div').querySelector('p').textContent;
    var identity = document.createElement('p');
    identity.className = 'text-xs';
    identity.textContent = 'Binding: assigned on save';
    var remove = document.createElement('button');
    remove.type = 'button';
    remove.className = 'btn btn-ghost btn-xs';
    remove.dataset.agentToolRemove = '';
    remove.textContent = 'Remove from draft';
    row.append(label, target, identity, remove);
    root.querySelector('[data-agent-tool-rows]').append(row);
    syncAgentToolRows(root);
    error.textContent = '';
    input.focus();
}
document.addEventListener('click', function (event) {
    var add = event.target.closest('[data-agent-tool-add]');
    var remove = event.target.closest('[data-agent-tool-remove]');
    if (add && !add.disabled) addAgentToolRow(add.closest('[data-agent-tools-editor]'), add);
    if (remove) {
        var root = remove.closest('[data-agent-tools-editor]');
        var removed = remove.closest('[data-agent-tool-row]');
        var next = removed.nextElementSibling || removed.previousElementSibling;
        removed.remove();
        syncAgentToolRows(root);
        var focus = next ? next.querySelector('[data-agent-tool-alias]') : root.querySelector('[name=kind]');
        if (focus) focus.focus();
    }
    var move = event.target.closest('[data-agent-skill-move]');
    if (move) {
        var row = move.closest('[data-agent-skill-row]');
        var peer = move.dataset.agentSkillMove === 'up' ? row.previousElementSibling : row.nextElementSibling;
        if (peer) {
            if (move.dataset.agentSkillMove === 'up') row.parentNode.insertBefore(row, peer);
            else row.parentNode.insertBefore(peer, row);
            row.querySelector('input[type=checkbox]').dispatchEvent(new Event('change', { bubbles: true }));
            move.focus();
        }
    }
});
document.addEventListener('input', function (event) {
    if (event.target.matches('[data-agent-tool-alias]')) syncAgentToolRows(event.target.closest('[data-agent-tools-editor]'));
});
// A completed write keeps running on the server. Its detached response must not replace an agent
// selected after that write began, nor apply its out-of-band list or push a stale navigation URL.
document.addEventListener('htmx:beforeRequest', function (event) {
    var element = event.detail.elt;
    if (!element.closest('#agent-pane')) return;
    var target = event.detail.target;
    element.addEventListener('htmx:beforeOnLoad', function (answer) {
        if (target && !target.isConnected) answer.preventDefault();
    }, { once: true });
}, true); // Capture before the shared loading placeholder detaches the triggering form.
"#;
