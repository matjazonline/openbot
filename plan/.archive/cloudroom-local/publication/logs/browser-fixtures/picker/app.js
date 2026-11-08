var CHIP_SELECTED_MARK = "<svg class=\"inline-block shrink-0 h-4 w-4\" viewBox=\"0 0 16 16\" fill=\"currentColor\" aria-hidden=\"true\"><path d=\"M13.78 4.22a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.22 9.28a.751.751 0 0 1 .018-1.042.751.751 0 0 1 1.042-.018L6 10.94l6.72-6.72a.75.75 0 0 1 1.06 0Z\"/></svg>";
var CHIP_ADD_MARK = "<svg class=\"inline-block shrink-0 h-4 w-4\" viewBox=\"0 0 16 16\" fill=\"currentColor\" aria-hidden=\"true\"><path d=\"M7.75 2a.75.75 0 0 1 .75.75V7h4.25a.75.75 0 0 1 0 1.5H8.5v4.25a.75.75 0 0 1-1.5 0V8.5H2.75a.75.75 0 0 1 0-1.5H7V2.75A.75.75 0 0 1 7.75 2Z\"/></svg>";
var AGENT_REPLIED_MARK = "<svg class=\"inline-block shrink-0 h-4 w-4\" viewBox=\"0 0 16 16\" fill=\"currentColor\" aria-hidden=\"true\"><path d=\"M13.78 4.22a.75.75 0 0 1 0 1.06l-7.25 7.25a.75.75 0 0 1-1.06 0L2.22 9.28a.751.751 0 0 1 .018-1.042.751.751 0 0 1 1.042-.018L6 10.94l6.72-6.72a.75.75 0 0 1 1.06 0Z\"/></svg>";
        function getCachedCompanyId() {
            return localStorage.getItem('cached_company_id');
        }

        function selectCompany(companyId) {
            if (!companyId) return;
            localStorage.setItem('cached_company_id', companyId);
            updateNavChannels();
        }

        function clearCachedCompanyIfMatch(companyId) {
            if (getCachedCompanyId() === companyId) {
                localStorage.removeItem('cached_company_id');
                updateNavChannels();
            }
        }

        function updateNavChannels() {
            const navChannels = document.getElementById('nav-channels');
            const navAgents = document.getElementById('nav-agents');
            const companyId = getCachedCompanyId();
            if (navChannels) {
                if (companyId) {
                    navChannels.href = '/companies/' + companyId + '/channels';
                    navChannels.classList.remove('hidden');
                } else {
                    navChannels.classList.add('hidden');
                }
            }
            if (navAgents) {
                if (companyId) {
                    navAgents.href = '/companies/' + companyId + '/agents';
                    navAgents.classList.remove('hidden');
                } else {
                    navAgents.classList.add('hidden');
                }
            }
            document.querySelectorAll('[id^="selected-badge-"]').forEach(el => {
                if (el.id === 'selected-badge-' + companyId) {
                    el.classList.remove('hidden');
                } else {
                    el.classList.add('hidden');
                }
            });
        }

        function autoDetectAndSyncCompany() {
            const match = window.location.pathname.match(/\/companies\/([a-f0-9\-]{36})/i);
            if (match && match[1]) {
                selectCompany(match[1]);
            } else {
                updateNavChannels();
            }
        }

        document.addEventListener('DOMContentLoaded', autoDetectAndSyncCompany);
        document.addEventListener('htmx:afterSettle', autoDetectAndSyncCompany);
        autoDetectAndSyncCompany();

        function toggleSpamWarning(input) {
            var form = input.closest('form');
            if (!form) return;
            var box = form.querySelector('.spam-disabled-box');
            if (!box) return;
            var checkbox = box.querySelector('input[type="checkbox"]');
            var isPublic = input.value.toLowerCase().includes('@public');
            if (isPublic) {
                box.classList.remove('opacity-40', 'pointer-events-none', 'grayscale');
                if (checkbox) {
                    checkbox.disabled = false;
                }
            } else {
                box.classList.add('opacity-40', 'pointer-events-none', 'grayscale');
                if (checkbox) {
                    checkbox.disabled = true;
                    checkbox.checked = false;
                }
            }
        }

        window.companyTeamCache = window.companyTeamCache || {};

        async function fetchCompanyTeam(companyId) {
            if (!companyId) return [];
            if (window.companyTeamCache[companyId]) {
                return window.companyTeamCache[companyId];
            }
            try {
                const res = await fetch('/api/companies/' + companyId + '/team', { credentials: 'same-origin' });
                if (!res.ok) return [];
                const data = await res.json();
                if (data && data.success && Array.isArray(data.members)) {
                    const members = data.members.filter(m => m.email && m.email.trim().length > 0);
                    window.companyTeamCache[companyId] = members;
                    return members;
                }
            } catch (e) {
                console.error('Error fetching team members:', e);
            }
            return [];
        }

        async function initTeamAutocomplete() {
            const inputs = document.querySelectorAll('input[name="participant_emails"]');
            if (!inputs.length) return;

            for (const input of inputs) {
                if (input.dataset.teamAutocompleteInitialized === 'true') continue;
                input.dataset.teamAutocompleteInitialized = 'true';
                input.setAttribute('autocomplete', 'off');

                let companyId = input.dataset.companyId ||
                                input.closest('[data-company-id]')?.dataset.companyId;
                if (!companyId) {
                    const match = window.location.pathname.match(/\/companies\/([a-f0-9\-]{36})/i);
                    if (match && match[1]) companyId = match[1];
                }
                if (!companyId && typeof getCachedCompanyId === 'function') {
                    companyId = getCachedCompanyId();
                }

                if (!companyId) continue;

                const members = await fetchCompanyTeam(companyId);
                if (!members.length) continue;

                const parent = input.parentElement;
                if (!parent) continue;

                let wrapper = parent.querySelector('.team-autocomplete-wrapper');
                if (!wrapper) {
                    wrapper = document.createElement('div');
                    wrapper.className = 'team-autocomplete-wrapper relative';
                    input.parentNode.insertBefore(wrapper, input);
                    wrapper.appendChild(input);
                }

                let chipsContainer = parent.querySelector('.team-chips-container');
                if (!chipsContainer) {
                    chipsContainer = document.createElement('div');
                    chipsContainer.className = 'team-chips-container mt-2 flex flex-wrap items-center gap-1.5 text-xs';
                    parent.appendChild(chipsContainer);
                }

                let dropdown = wrapper.querySelector('.team-dropdown');
                if (!dropdown) {
                    dropdown = document.createElement('div');
                    dropdown.className = 'team-dropdown absolute left-0 right-0 top-full mt-1 z-50 bg-base-200 border border-base-300 rounded-lg shadow-xl hidden max-h-48 overflow-y-auto font-sans';
                    wrapper.appendChild(dropdown);
                }

                function getParsedEmails() {
                    return input.value
                        .split(',')
                        .map(s => s.trim().toLowerCase())
                        .filter(Boolean);
                }

                function updateChips() {
                    const currentEmails = getParsedEmails();
                    chipsContainer.innerHTML = '<span class="text-[11px] font-medium text-base-content/70 mr-1">Team:</span>';
                    members.forEach(member => {
                        const emailLower = member.email.toLowerCase();
                        const isSelected = currentEmails.includes(emailLower);
                        const btn = document.createElement('button');
                        btn.type = 'button';
                        btn.className = isSelected
                            ? 'px-2 py-0.5 rounded-md bg-success/10 text-base-content border border-success/40 text-[11px] font-mono cursor-pointer hover:bg-success/20 transition flex items-center gap-1'
                            : 'px-2 py-0.5 rounded-md bg-base-200 text-base-content/80 border border-base-300 text-[11px] font-mono cursor-pointer hover:bg-base-300 hover:text-base-content transition flex items-center gap-1';
                        // The glyph is markup, the email is not: it goes in as text so an
                        // address containing angle brackets stays an address.
                        btn.innerHTML = (isSelected ? CHIP_SELECTED_MARK : CHIP_ADD_MARK) + '<span></span>';
                        btn.lastChild.textContent = member.email;
                        btn.title = (member.username ? member.username + ' (' + member.role + ')' : member.role);
                        btn.addEventListener('click', (e) => {
                            e.preventDefault();
                            let emails = input.value.split(',').map(s => s.trim()).filter(Boolean);
                            if (isSelected) {
                                emails = emails.filter(e => e.toLowerCase() !== emailLower);
                            } else {
                                if (!emails.some(e => e.toLowerCase() === emailLower)) {
                                    emails.push(member.email);
                                }
                            }
                            input.value = emails.join(', ');
                            input.dispatchEvent(new Event('input', { bubbles: true }));
                            updateChips();
                        });
                        chipsContainer.appendChild(btn);
                    });
                }

                function getCurrentToken() {
                    const val = input.value;
                    const pos = input.selectionStart || val.length;
                    const left = val.slice(0, pos);
                    const lastComma = left.lastIndexOf(',');
                    const token = lastComma >= 0 ? left.slice(lastComma + 1) : left;
                    return { token: token.trim(), pos, lastComma };
                }

                function renderDropdown() {
                    const { token, lastComma } = getCurrentToken();
                    if (!token) {
                        dropdown.classList.add('hidden');
                        return;
                    }
                    const tokenLower = token.toLowerCase();
                    const currentEmails = getParsedEmails();
                    const matches = members.filter(m => {
                        const emailLower = m.email.toLowerCase();
                        const nameLower = (m.username || '').toLowerCase();
                        return (emailLower.includes(tokenLower) || nameLower.includes(tokenLower)) &&
                               !currentEmails.includes(emailLower);
                    });

                    if (!matches.length) {
                        dropdown.classList.add('hidden');
                        return;
                    }

                    dropdown.innerHTML = '';
                    matches.forEach(m => {
                        const item = document.createElement('div');
                        item.className = 'px-3 py-2 hover:bg-base-300 cursor-pointer text-xs flex items-center justify-between border-b border-base-300 last:border-b-0 text-base-content';
                        // Team API fields are plain strings: create the markup here, then add
                        // each field as text so names and addresses cannot become elements.
                        const email = document.createElement('span');
                        email.className = 'font-mono text-base-content font-medium';
                        email.textContent = m.email;
                        const identity = document.createElement('span');
                        identity.className = 'text-base-content/70 text-[11px]';
                        identity.textContent = m.username ? m.username + ' (' + m.role + ')' : m.role;
                        item.append(email, identity);

                        item.addEventListener('mousedown', (e) => {
                            e.preventDefault();
                            const val = input.value;
                            const before = lastComma >= 0 ? val.slice(0, lastComma + 1) : '';
                            const after = val.slice(input.selectionStart || val.length);
                            const afterClean = after.replace(/^[^,]*/, '');
                            const prefix = before ? before.trim() + ' ' : '';
                            input.value = prefix + m.email + ', ' + afterClean.trim();
                            input.dispatchEvent(new Event('input', { bubbles: true }));
                            dropdown.classList.add('hidden');
                            updateChips();
                        });
                        dropdown.appendChild(item);
                    });

                    dropdown.classList.remove('hidden');
                }

                input.addEventListener('input', () => {
                    updateChips();
                    renderDropdown();
                });

                input.addEventListener('focus', () => {
                    updateChips();
                    renderDropdown();
                });

                input.addEventListener('blur', () => {
                    setTimeout(() => dropdown.classList.add('hidden'), 200);
                });

                updateChips();
            }
        }

        document.addEventListener('DOMContentLoaded', initTeamAutocomplete);
        document.addEventListener('htmx:afterSettle', initTeamAutocomplete);
        function slugifyValue(value) {
            return value.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '');
        }

        // Mirror a typed name into its slug field: an explicit target id when one is named,
        // otherwise the sibling `slug` field of the same form.
        function syncSlugField(input) {
            var slug = input.dataset.slugTarget
                ? document.getElementById(input.dataset.slugTarget)
                : (input.form ? input.form.slug : null);
            if (slug) {
                slug.value = slugifyValue(input.value);
                slug.dispatchEvent(new Event('input', { bubbles: true }));
            }
        }

        function toggleFormCard(button) {
            var card = document.getElementById(button.dataset.card);
            if (!card) return;
            var opening = card.classList.contains('hidden');
            card.classList.toggle('hidden');
            button.setAttribute('aria-expanded', opening);
        }

        // Collapse a create-form card after htmx reports a successful submit.
        function resetAndCollapseForm(form) {
            form.reset();
            var card = document.getElementById(form.dataset.card);
            if (card) card.classList.add('hidden');
            var toggle = document.getElementById(form.dataset.toggle);
            if (toggle) {
                toggle.classList.remove('hidden');
                toggle.setAttribute('aria-expanded', 'false');
            }
        }

        function togglePromptGenerator(button) {
            var box = document.getElementById(button.dataset.box);
            if (!box) return;
            box.classList.toggle('hidden');
            if (box.classList.contains('hidden')) return;
            var input = document.getElementById(button.dataset.focus);
            if (input) input.focus();
        }

        // Mark a form as in-flight so a double submit cannot fire a second request.
        function markSubmitBusy(form) {
            if (form.dataset.busy) return false;
            form.dataset.busy = 'true';
            form.setAttribute('aria-busy', 'true');
            var progress = form.querySelector('[data-progress]');
            if (progress) progress.classList.remove('hidden');
            var button = form.querySelector('[type=submit]');
            if (button) {
                button.classList.add('pointer-events-none');
                button.setAttribute('aria-disabled', 'true');
                var label = button.querySelector('[data-label]');
                if (label) label.textContent = form.dataset.pendingLabel || '';
            }
            return true;
        }

        function copyTextFrom(button) {
            var source = document.getElementById(button.dataset.copyFrom);
            if (!source) return;
            navigator.clipboard.writeText(source.textContent);
            button.textContent = button.dataset.copiedLabel || 'Copied';
        }

        // Applied to the htmx fragment that carries a freshly generated system prompt, which
        // previously shipped as an inline `<script>` in the response body.
        function applyGeneratedPrompt(holder) {
            var target = document.getElementById(holder.dataset.targetId);
            if (target) {
                target.value = holder.dataset.generatedPrompt || '';
                target.dispatchEvent(new Event('input', { bubbles: true }));
                target.dispatchEvent(new Event('change', { bubbles: true }));
            }
            var genBox = document.getElementById(holder.dataset.genBox);
            if (genBox) {
                setTimeout(function () { genBox.classList.add('hidden'); }, 1000);
            }
        }
        // The `theme-controller` checkbox already repaints the page by itself -- daisyUI matches
        // on `:root:has(.theme-controller[value=light]:checked)`. All this does is write the
        // choice down for the next request and put `data-theme` back in agreement with the box,
        // so THEME_INIT_SCRIPT can restore it before the next paint.
        function applyTheme(theme) {
            document.documentElement.setAttribute('data-theme', theme);
            try { localStorage.setItem('ui_theme', theme); } catch (e) {}
        }

        // The reverse direction, on load: THEME_INIT_SCRIPT ran before this markup existed, so the
        // box has to be caught up with the theme it chose.
        function syncThemeController() {
            var toggle = document.getElementById('theme-toggle');
            if (toggle) {
                toggle.checked = document.documentElement.getAttribute('data-theme') === 'light';
            }
        }

        syncThemeController();

        function confirmLogout() {
            document.getElementById('logout-modal').showModal();
        }

        // The SSE extension owns reconnection and emits these lifecycle events around it. Track
        // URLs rather than EventSource objects because a retry replaces the object while keeping
        // the stream URL. Brief network blips stay silent; a visible interruption is acknowledged
        // when every affected stream is open again.
        var interruptedLiveStreams = new Set();
        var liveUpdateWarningTimer = null;
        var liveUpdateRestoredTimer = null;

        function liveStreamKey(event) {
            var source = event.detail && event.detail.source;
            return source && source.url ? source.url : 'unknown';
        }

        function showLiveUpdateStatus(message, restored) {
            var status = document.getElementById('live-update-status');
            if (!status) return;
            status.querySelector('span').textContent = message;
            status.classList.toggle('alert-warning', !restored);
            status.classList.toggle('alert-success', restored);
            status.classList.remove('hidden');
        }

        document.body.addEventListener('htmx:sseError', function (event) {
            interruptedLiveStreams.add(liveStreamKey(event));
            window.clearTimeout(liveUpdateRestoredTimer);
            var status = document.getElementById('live-update-status');
            if (status && !status.classList.contains('hidden')) {
                showLiveUpdateStatus('Live updates paused. Reconnecting…', false);
                return;
            }
            if (liveUpdateWarningTimer) return;
            liveUpdateWarningTimer = window.setTimeout(function () {
                liveUpdateWarningTimer = null;
                if (interruptedLiveStreams.size) {
                    showLiveUpdateStatus('Live updates paused. Reconnecting…', false);
                }
            }, 1000);
        });

        document.body.addEventListener('htmx:sseOpen', function (event) {
            interruptedLiveStreams.delete(liveStreamKey(event));
            interruptedLiveStreams.delete('unknown');
            if (interruptedLiveStreams.size) return;

            window.clearTimeout(liveUpdateWarningTimer);
            liveUpdateWarningTimer = null;
            var status = document.getElementById('live-update-status');
            if (!status || status.classList.contains('hidden')) return;

            showLiveUpdateStatus('Live updates restored.', true);
            liveUpdateRestoredTimer = window.setTimeout(function () {
                status.classList.add('hidden');
            }, 2000);
        });

        // Scoped to the clicked entry's own list, so every `/ui` sidebar highlights with it.
        function selectSidebarItem(el) {
            var menu = el.closest('ul');
            if (!menu) return;
            menu.querySelectorAll('a').forEach(function (item) {
                item.classList.remove('menu-active');
            });
            el.classList.add('menu-active');

            var channelId = el.dataset.mailboxChannel;
            if (channelId) {
                document.querySelectorAll('[data-mailbox-channel]').forEach(function (item) {
                    item.classList.toggle('menu-active', item.dataset.mailboxChannel === channelId);
                });
                var name = el.querySelector('[data-mailbox-channel-name]');
                var label = document.getElementById('mailbox-selector-label');
                if (name && label) label.textContent = name.textContent;
                var popover = el.closest('[popover]');
                if (popover && popover.matches(':popover-open')) popover.hidePopover();
                setMailboxSidebarExpanded(false);
            }
        }

        function setMailboxSidebarExpanded(expanded) {
            var sidebar = document.getElementById('mailbox-sidebar');
            if (!sidebar) return;
            sidebar.classList.toggle('mailbox-sidebar-open', expanded);
            document.querySelectorAll('[aria-controls="mailbox-sidebar"]').forEach(function (control) {
                control.setAttribute('aria-expanded', expanded ? 'true' : 'false');
            });
        }

        function toggleMailboxSidebar() {
            var sidebar = document.getElementById('mailbox-sidebar');
            if (!sidebar) return;
            setMailboxSidebarExpanded(!sidebar.classList.contains('mailbox-sidebar-open'));
        }

        // Below the compact breakpoint the rail is a drawer rather than a column, and these two
        // attributes on <body> are the whole of the phone layout's state -- COMPACT_LAYOUT_STYLES
        // reads them and nothing else does. On a wide window they are inert.
        function setRailOpen(open) {
            document.body.dataset.rail = open ? 'open' : 'closed';
            var toggle = document.querySelector('[data-action="toggle-rail"]');
            if (toggle) toggle.setAttribute('aria-expanded', open ? 'true' : 'false');
        }

        function toggleRail() {
            setRailOpen(document.body.dataset.rail !== 'open');
        }

        // Which of a workspace's two columns the phone is showing. Opening something takes the
        // reader to it and closes whatever navigation is over the top; the back button in the top
        // bar is the way out.
        function setMobilePane(pane) {
            document.body.dataset.pane = pane;
            setRailOpen(false);
        }

        // What the server just rendered decides where a fresh load starts: a detail pane marked
        // empty means nothing is open, so the list is what the reader wants to see first. This is
        // also what makes a deep link -- a thread, a task, a channel in the URL -- open on the
        // thing it names rather than on the list beside it.
        function syncMobilePane() {
            var detail = document.querySelector('.ui-pane-detail');
            setMobilePane(detail && !detail.hasAttribute('data-pane-empty') ? 'detail' : 'list');
        }

        syncMobilePane();
        document.addEventListener('htmx:historyRestore', syncMobilePane);

        // Every workspace opens its detail the same way -- an htmx swap of the pane -- so one
        // listener covers all of them, and none of the pages has to know the phone layout exists.
        // A swap that puts an *empty* pane back (a cancelled form, a deleted record) means the
        // selection is gone, and the reader belongs back on the list.
        document.body.addEventListener('htmx:beforeSwap', function (event) {
            // Only a request the reader made is navigation. A live stream writing into the pane
            // they are not looking at must not drag them over to it, and carries no `xhr`.
            if (!event.detail.xhr) return;
            // Nor is a refresh: a read the stream triggered (`hx-trigger="sse:…"`), or one with no
            // triggering event at all -- htmx issues a poll (`every …`) and a `load` read without
            // one. Only something the reader did, a click or a submit, opens a pane.
            var trigger = event.detail.requestConfig && event.detail.requestConfig.triggeringEvent;
            var type = trigger && typeof trigger.type === 'string' ? trigger.type : '';
            if (!type || type.indexOf('sse:') === 0) return;
            var target = event.detail.target;
            if (!target || !target.closest || !target.closest('.ui-pane-detail')) return;
            var empty = (event.detail.serverResponse || '').indexOf('data-pane-empty') !== -1;
            setMobilePane(empty ? 'list' : 'detail');
        });

        // Scoped to the clicked row's own list, like [`selectSidebarItem`]: the mailbox thread list
        // and the schedule runs list both use these rows, and each highlights within itself.
        function selectThreadRow(el) {
            var list = el.closest('[data-thread-list]');
            if (!list) return;
            list.querySelectorAll('.thread-row').forEach(function (row) {
                row.classList.remove('bg-base-300');
            });
            el.classList.add('bg-base-300');
            // Opening a thread is reading it, so its reply mark has done its job. A schedule run
            // carries no mark, so there this is a no-op.
            clearReplyMark(el);
        }

        function clearReplyMark(row) {
            var mark = row.querySelector('.thread-mark');
            if (mark) {
                mark.textContent = '';
                mark.removeAttribute('title');
            }
        }

        // Settle every thread whose agent has just answered: the row's activity mark goes quiet,
        // and a thread the reader was not looking at gains the reply check. That glyph is an
        // inline SVG rendered by the server into AGENT_REPLIED_MARK, so it matches the one a
        // freshly loaded row is drawn with instead of being a second copy of it here.
        //
        // The check is deliberately a client-side, in-session signal rather than a stored unread
        // flag: it means "this happened while you were watching", so a reload starting clean is
        // correct rather than a bug. The open thread never gets one -- the reply is already on
        // screen, and `quietRepliedRow` has settled that row from the message stream.
        function markLiveAgentReplies(list) {
            var pane = document.getElementById('detail-pane');
            var openThreadId = pane ? pane.dataset.threadId : null;

            list.querySelectorAll('.thread-row[data-last-role="agent"]').forEach(function (row) {
                // The attribute is an arrival, not a standing fact about the thread, so it is spent
                // here. Left on, every later insert of some *other* row would run this again over
                // threads the reader has already dealt with.
                row.removeAttribute('data-last-role');
                // The agent has answered here, so this row's activity mark has said all it can --
                // whether or not this is the thread on screen. The reply is the end of what the
                // dot was promising, and the check beside it now carries the news.
                row.classList.add('thread-replied');
                if (row.dataset.threadId === openThreadId) return;
                var mark = row.querySelector('.thread-mark');
                if (!mark) return;
                mark.innerHTML = AGENT_REPLIED_MARK;
                mark.title = 'Agent replied';
            });
        }

        // The agent's reply landing in the open thread is the answer that thread's activity mark
        // was promising, so the row goes quiet: the reader has the result in front of them and does
        // not need a dot telling them a run they can see the end of is still being wound up.
        //
        // A class rather than emptying the slot, and one `dedupeThreadRow` carries: the bumped row
        // arrives over the column's own connection with no ordering against this one, so a mark
        // cleared here would be re-drawn a moment later by a row rendered before the task settled.
        //
        // It lasts until the column has something new to say about that thread -- a badge with a
        // state on it -- rather than until the reader looks away. Reading a reply settles the row
        // for good; opening another thread is not new information about this one.
        function quietRepliedRow(bubble) {
            if (!bubble || bubble.dataset.role !== 'agent') return;
            var pane = document.getElementById('detail-pane');
            var openThreadId = pane ? pane.dataset.threadId : null;
            if (!openThreadId) return;
            var row = document.querySelector(
                '#thread-list .thread-row[data-thread-id="' + openThreadId + '"]');
            if (row) {
                row.classList.add('thread-replied');
            }
        }

        // Put the newest message's first line at the top of the viewport. For a short message the
        // browser naturally clamps at the bottom; for a long reply this avoids landing at its end.
        function scrollToNewestMessageStart() {
            var pane = document.getElementById('message-scroll');
            if (!pane) return;
            var newest = pane.lastElementChild;
            if (!newest || newest.id === 'no-messages') return;
            pane.scrollTop = newest.offsetTop - pane.offsetTop;
        }

        // Enter sends, Shift+Enter keeps writing — the chat convention, not the textarea one.
        function composerKeydown(event) {
            if (event.key !== 'Enter' || event.shiftKey) return;
            event.preventDefault();
            if (event.target.value.trim() === '') return;
            if (event.target.form) {
                event.target.form.requestSubmit();
            }
        }

        // The box starts one line tall and grows with the message, up to its max height.
        function autoGrowComposer(el) {
            el.style.height = 'auto';
            el.style.height = Math.min(el.scrollHeight, 160) + 'px';
        }

        // Tailwind's browser build styles the page after it is parsed, so on a direct /ui load the
        // pane has nothing to scroll until everything has been applied.
        window.addEventListener('load', scrollToNewestMessageStart);

        // A streamed message is appended through htmx's normal swap machinery, so it reaches the
        // afterSettle handler below too. That handler is written for a *whole pane* arriving and
        // would steal the caret back from a half-typed draft, so streaming marks itself out.
        var streamingMessage = false;

        // Whether the reader is at the live edge of the conversation. Someone who scrolled up is
        // reading history, and an arriving message must not yank them back down.
        function messagesAreAtBottom(pane) {
            return pane.scrollHeight - pane.scrollTop - pane.clientHeight < 80;
        }

        var wasAtBottomBeforeStream = true;

        document.body.addEventListener('htmx:sseBeforeMessage', function () {
            streamingMessage = true;
            var pane = document.getElementById('message-scroll');
            wasAtBottomBeforeStream = !pane || messagesAreAtBottom(pane);
        });

        // A thread that receives a message is bumped to the top of its column, so the live row
        // arrives as an insert at the top and the copy further down is now stale. Removing it
        // *after* the insert is what turns "insert" into "move", and it leaves every other row --
        // including any older pages loaded on demand -- exactly where it was.
        function dedupeThreadRow(list) {
            var rows = list.querySelectorAll('.thread-row[data-thread-id]');
            var seen = {};
            rows.forEach(function (row) {
                var id = row.dataset.threadId;
                if (!seen[id]) {
                    seen[id] = row;
                    return;
                }
                // The stale copy knows two things the stream cannot: whether this thread is the
                // one this browser has open, and whether its reply is still unread.
                if (row.classList.contains('bg-base-300')) {
                    seen[id].classList.add('bg-base-300');
                }
                if (row.classList.contains('thread-replied')) {
                    seen[id].classList.add('thread-replied');
                }
                var staleMark = row.querySelector('.thread-mark');
                var freshMark = seen[id].querySelector('.thread-mark');
                if (staleMark && freshMark && staleMark.textContent && !freshMark.textContent) {
                    freshMark.textContent = staleMark.textContent;
                    freshMark.title = staleMark.title;
                }
                row.remove();
            });
        }

        // Appending rather than replacing is the whole point of the live stream: the draft, the
        // caret and the scroll position all survive a message arriving mid-sentence.
        document.body.addEventListener('htmx:sseMessage', function (event) {
            var swapped = event.target;

            // A thread row's badge redraws in place; nothing about the messages changed.
            //
            // A badge arriving with something on it is a state the reader has not seen, so it also
            // lifts the quiet an earlier reply put on that row. An empty one is the run ending,
            // which is the reply's own news and leaves the row as the reply left it.
            if (swapped && swapped.classList && swapped.classList.contains('thread-activity')) {
                if (swapped.firstElementChild) {
                    var badgeRow = swapped.closest('.thread-row');
                    if (badgeRow) {
                        badgeRow.classList.remove('thread-replied');
                    }
                }
                return;
            }

            // The open thread's activity strip makes the pane taller, which would push the newest
            // message out of view for someone sitting at the live edge.
            if (swapped && swapped.id === 'thread-activity') {
                if (wasAtBottomBeforeStream) {
                    scrollToNewestMessageStart();
                }
                return;
            }

            if (swapped && swapped.id === 'thread-list') {
                dedupeThreadRow(swapped);
                markLiveAgentReplies(swapped);
                // The channel is no longer empty, so its placeholder must go.
                var noThreads = swapped.querySelector('.no-threads');
                if (noThreads) {
                    noThreads.remove();
                }
                return;
            }

            if (swapped && swapped.id === 'message-scroll') {
                quietRepliedRow(swapped.lastElementChild);
            }

            // The thread is no longer empty, so its placeholder must go.
            var placeholder = document.getElementById('no-messages');
            if (placeholder) {
                placeholder.remove();
            }
            if (wasAtBottomBeforeStream) {
                scrollToNewestMessageStart();
            }
        });

        // Only the swaps that bring in the messages themselves: paging the thread column must not
        // yank an already-open thread to its newest message.
        document.body.addEventListener('htmx:afterSettle', function (event) {
            // A streamed append settles here too, and it has already decided about scrolling. The
            // flag is cleared here rather than on sseMessage because this event fires *after* it.
            if (streamingMessage) {
                streamingMessage = false;
                return;
            }
            var settled = event.target;
            if (settled && settled.nodeType === 1 &&
                (settled.id === 'message-scroll' || settled.querySelector('#message-scroll'))) {
                scrollToNewestMessageStart();
                // A sent message swaps the whole pane, so hand the caret back to the new box.
                var composer = document.getElementById('thread-composer');
                if (composer) {
                    composer.focus();
                }
            }
        });

        function localizeTimes(root) {
            var scope = root && root.querySelectorAll ? root : document;
            var times = Array.from(scope.querySelectorAll('time[data-local-time]:not([data-localized])'));
            if (scope.matches && scope.matches('time[data-local-time]:not([data-localized])')) {
                times.unshift(scope);
            }
            times.forEach(function (el) {
                var at = new Date(el.dateTime);
                if (Number.isNaN(at.getTime())) return;

                var precision = el.dataset.localTime;
                var options = precision === 'date'
                    ? { year: 'numeric', month: 'short', day: 'numeric' }
                    : precision === 'time'
                        ? { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit', timeZoneName: 'short' }
                        : { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', timeZoneName: 'short' };

                el.textContent = new Intl.DateTimeFormat(undefined, options).format(at);
                el.title = at.toISOString();
                el.dataset.localized = 'true';
            });
        }

        document.addEventListener('DOMContentLoaded', function () { localizeTimes(document); });
        document.addEventListener('htmx:afterSettle', function (event) { localizeTimes(event.target); });
        localizeTimes(document);

        var UI_SKELETONS = {
            "pane": { markup: "<div class=\"flex items-center gap-3\">\n    <div class=\"skeleton h-12 w-12 shrink-0 rounded-full\"><\/div>\n    <div class=\"flex min-w-0 flex-1 flex-col gap-2\">\n        <div class=\"skeleton h-5 w-2/5\"><\/div>\n        <div class=\"skeleton h-3 w-1/4\"><\/div>\n    <\/div>\n<\/div>\n<div class=\"skeleton h-12 w-full\"><\/div>\n<div class=\"flex flex-col gap-2 rounded-box border border-base-300 p-4\"><div class=\"flex items-center justify-between gap-4\">\n        <div class=\"skeleton h-3 w-24\"><\/div>\n        <div class=\"skeleton h-3 w-1/3\"><\/div>\n    <\/div><div class=\"flex items-center justify-between gap-4\">\n        <div class=\"skeleton h-3 w-24\"><\/div>\n        <div class=\"skeleton h-3 w-1/3\"><\/div>\n    <\/div><div class=\"flex items-center justify-between gap-4\">\n        <div class=\"skeleton h-3 w-24\"><\/div>\n        <div class=\"skeleton h-3 w-1/3\"><\/div>\n    <\/div><div class=\"flex items-center justify-between gap-4\">\n        <div class=\"skeleton h-3 w-24\"><\/div>\n        <div class=\"skeleton h-3 w-1/3\"><\/div>\n    <\/div><div class=\"flex items-center justify-between gap-4\">\n        <div class=\"skeleton h-3 w-24\"><\/div>\n        <div class=\"skeleton h-3 w-1/3\"><\/div>\n    <\/div><\/div>\n<div class=\"skeleton h-40 w-full\"><\/div>", className: "flex flex-1 flex-col gap-4 overflow-hidden bg-base-100 p-6", appends: false },
            "list": { markup: "<div class=\"flex items-center justify-between gap-2 px-3 py-2\">\n    <div class=\"skeleton h-3 w-28\"><\/div>\n    <div class=\"skeleton h-4 w-4 rounded-full\"><\/div>\n<\/div>\n<div class=\"flex flex-col gap-1 px-2\"><div class=\"flex flex-col gap-2 rounded-box px-3 py-2\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-4 w-14 shrink-0 rounded-full\"><\/div>\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-3/4\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 rounded-box px-3 py-2\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-4 w-14 shrink-0 rounded-full\"><\/div>\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-3/4\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 rounded-box px-3 py-2\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-4 w-14 shrink-0 rounded-full\"><\/div>\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-3/4\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 rounded-box px-3 py-2\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-4 w-14 shrink-0 rounded-full\"><\/div>\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-3/4\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 rounded-box px-3 py-2\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-4 w-14 shrink-0 rounded-full\"><\/div>\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-3/4\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 rounded-box px-3 py-2\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-4 w-14 shrink-0 rounded-full\"><\/div>\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-3/4\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 rounded-box px-3 py-2\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-4 w-14 shrink-0 rounded-full\"><\/div>\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-3/4\"><\/div>\n    <\/div><\/div>", className: null, appends: false },
            "thread-column": { markup: "<div class=\"flex items-center justify-between gap-2 border-b border-base-300 px-4 py-3\">\n    <div class=\"flex min-w-0 flex-col gap-2\">\n        <div class=\"skeleton h-5 w-32\"><\/div>\n        <div class=\"skeleton h-3 w-40\"><\/div>\n    <\/div>\n    <div class=\"skeleton h-8 w-28 shrink-0\"><\/div>\n<\/div>\n<div class=\"flex flex-col gap-1 overflow-hidden p-2\"><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><\/div>", className: null, appends: false },
            "thread-rows": { markup: "<div class=\"flex flex-col gap-1 p-2\"><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><div class=\"flex flex-col gap-2 px-3 py-3\">\n        <div class=\"flex items-center gap-2\">\n            <div class=\"skeleton h-3 flex-1\"><\/div>\n            <div class=\"skeleton h-2.5 w-12 shrink-0\"><\/div>\n        <\/div>\n        <div class=\"skeleton h-2.5 w-2/3\"><\/div>\n    <\/div><\/div>", className: null, appends: true },
            "panels": { markup: "<div class=\"flex flex-col gap-6 p-6\">\n    <div class=\"flex flex-col gap-3\">\n        <div class=\"skeleton h-5 w-48\"><\/div>\n        <div class=\"skeleton h-28 w-full\"><\/div><div class=\"skeleton h-28 w-full\"><\/div><div class=\"skeleton h-28 w-full\"><\/div>\n    <\/div>\n    <div class=\"flex flex-col gap-3\">\n        <div class=\"skeleton h-5 w-48\"><\/div>\n        <div class=\"skeleton h-28 w-full\"><\/div><div class=\"skeleton h-28 w-full\"><\/div>\n    <\/div>\n    <div class=\"skeleton h-20 w-full\"><\/div>\n<\/div>", className: null, appends: false },
        };

        (function () {
            // Keyed by element, so a region swapped away takes its saved content with it.
            var painted = new WeakMap();

            function paint(target) {
                var shape = UI_SKELETONS[target.dataset.skeleton];
                if (!shape || painted.has(target)) return;
                target.setAttribute('aria-busy', 'true');

                if (shape.appends) {
                    var addition = document.createElement('div');
                    addition.innerHTML = shape.markup;
                    target.appendChild(addition);
                    painted.set(target, { addition: addition });
                    return;
                }

                // The nodes themselves are kept, not their markup: the element that triggered
                // the request is usually one of them, and htmx reads the root node of that
                // element to find where an `hx-swap-oob` fragment belongs. Re-created from a
                // string, the trigger is left detached for good and every out-of-band fragment
                // in the response is silently dropped -- which is what kept a newly created
                // company out of the sidebar beside it.
                var kept = document.createDocumentFragment();
                while (target.firstChild) {
                    kept.appendChild(target.firstChild);
                }
                painted.set(target, { nodes: kept, className: target.className });
                if (shape.className) {
                    // The shape's classes describe the placeholder, not the region -- so anything
                    // that says *which column this is* is carried across rather than overwritten.
                    // Drop `ui-pane-detail` here and the compact layout loses track of the pane
                    // for the length of the request, which is exactly when it is being opened.
                    target.className = [identity(target), shape.className].filter(Boolean).join(' ');
                }
                target.innerHTML = shape.markup;
            }

            // What a region is, as opposed to how it currently looks: the classes the shell's
            // layout matches on, which outlive whatever is painted into it.
            function identity(target) {
                return Array.prototype.filter.call(target.classList, function (name) {
                    return name.indexOf('ui-pane-') === 0;
                }).join(' ');
            }

            function restore(target) {
                var previous = painted.get(target);
                if (!previous) return;
                painted.delete(target);
                target.removeAttribute('aria-busy');
                if (previous.addition) {
                    previous.addition.remove();
                    return;
                }
                target.className = previous.className;
                target.replaceChildren(previous.nodes);
            }

            document.body.addEventListener('htmx:beforeRequest', function (event) {
                var target = event.detail.target;
                if (target && target.dataset && target.dataset.skeleton) {
                    paint(target);
                }
            });

            // Undoing the placeholder *before* htmx decides what to do with the response costs
            // nothing when a swap follows -- it overwrites the region in the same frame -- and is
            // what leaves the reader's own content behind when one does not, as after an error.
            document.body.addEventListener('htmx:beforeSwap', function (event) {
                if (event.detail.target) {
                    restore(event.detail.target);
                }
            });

            // A request that never reached the server gets no swap at all, so this is the only
            // place a placeholder put up for one comes down.
            document.body.addEventListener('htmx:afterRequest', function (event) {
                if (event.detail.target) {
                    restore(event.detail.target);
                }
            });
        })();
        function toggleScheduleType(select) {
            var form = select.closest('form');
            if (!form) return;
            var isInterval = select.value === 'interval';
            var intervalBox = form.querySelector('.schedule-interval-box');
            var oneoffBox = form.querySelector('.schedule-oneoff-box');
            if (intervalBox) intervalBox.classList.toggle('hidden', !isInterval);
            if (oneoffBox) oneoffBox.classList.toggle('hidden', isInterval);
        }

        // The create pane holds one form per tab -- Easy, Simple and Advanced -- and shows the
        // picked one. A tab whose form the server did not render is simply absent.
        function showAgentTab(tab) {
            ['easy', 'simple', 'advanced'].forEach(function (name) {
                var form = document.getElementById('agent-tab-' + name);
                var button = document.getElementById('agent-tab-' + name + '-btn');
                if (form) form.classList.toggle('hidden', name !== tab);
                if (button) button.classList.toggle('tab-active', name === tab);
            });
        }

        // The generator is off to one side until asked for: writing a prompt by hand is still the
        // normal way to fill the field.
        function toggleAgentPromptGenerator(prefix) {
            var box = document.getElementById('agent-generator-' + prefix);
            if (!box) return;
            box.classList.toggle('hidden');
            if (!box.classList.contains('hidden')) {
                var input = document.getElementById('agent-instructions-' + prefix);
                if (input) input.focus();
            }
        }

        function updateAgentAddressPreview(input) {
            var preview = document.getElementById(input.dataset.addressPreview);
            if (preview) preview.textContent = '@' + (input.value || 'agent-handle');
        }

        function updateSimpleAgentAddressPreview(input) {
            var preview = document.getElementById('agent-address-preview');
            if (preview) preview.textContent = '@' + (slugifyValue(input.value) || 'agent-handle');
        }

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

        // The spam interlock only applies to a channel anyone can write to, so the confirmation
        // stays inert until the participants field actually names @public.
        function toggleChannelSpamConfirm(input) {
            var form = input.closest('form');
            if (!form) return;
            var box = form.querySelector('.spam-confirm-box');
            if (!box) return;
            var checkbox = box.querySelector('input[type="checkbox"]');
            var isPublic = input.value.toLowerCase().includes('@public');
            box.classList.toggle('opacity-40', !isPublic);
            box.classList.toggle('pointer-events-none', !isPublic);
            if (checkbox) {
                checkbox.disabled = !isPublic;
                if (!isPublic) checkbox.checked = false;
            }
        }


function csvValues(data,name){return (data.get(name)||'').split(',').map(function(value){return value.trim()}).filter(Boolean)}
function libraryPayload(form){const data=new FormData(form);return {name:data.get('name'),slug:data.get('slug'),provider:data.get('provider')||null,model:data.get('model')||null,system_prompt:data.get('system_prompt')||null,description:data.get('description')||null,avatar_url:data.get('avatar_url')||null,skill_ids:csvValues(data,'skill_ids')}}
async function libraryRequest(url,method,body){const response=await fetch(url,{method,headers:{'content-type':'application/json'},body:body?JSON.stringify(body):undefined});if(!response.ok)throw new Error(await response.text());return response.status===204?null:response.json()}
async function createLibraryAgent(event){event.preventDefault();await submitLibraryAgent(event.target,'/api/agent-library','POST')}
async function saveLibraryAgent(event,id){event.preventDefault();await submitLibraryAgent(event.target,'/api/agent-library/'+id,'PUT')}
async function submitLibraryAgent(form,url,method){
    if(form.getAttribute('aria-busy')==='true')return;
    const payload=libraryPayload(form);
    const buttons=Array.from(form.querySelectorAll('button[type=submit],button:not([type])'));
    form.setAttribute('aria-busy','true');buttons.forEach(button=>{button.dataset.previousLabel=button.textContent;button.textContent='Saving…';button.disabled=true});
    try{await libraryRequest(url,method,payload);location.reload()}catch(e){alert(e.message)}finally{form.removeAttribute('aria-busy');buttons.forEach(button=>{button.disabled=false;button.textContent=button.dataset.previousLabel})}
}
async function deleteLibraryAgent(id){if(!confirm('Delete this library agent?'))return;try{await libraryRequest('/api/agent-library/'+id,'DELETE');location.reload()}catch(e){alert(e.message)}}


function closeLiveStreamsForNavigation(event, link) {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || link.hasAttribute('download')) return;
    var target = link.getAttribute('target') || document.querySelector('base[target]')?.getAttribute('target');
    if (target && target.toLowerCase() !== '_self') return;
    if ((link.getAttribute('href') || '').startsWith('#')) return;
    var url = new URL(link.href, document.baseURI);
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return;
    var current = new URL(window.location.href);
    if (url.hash && url.origin === current.origin && url.pathname === current.pathname && url.search === current.search) return;
    closeLiveStreams();
}
function closeLiveStreams() {
    if (!window.htmx) return;
    document.querySelectorAll('[sse-connect], [data-sse-connect]').forEach(function (owner) {
        window.htmx.trigger(owner, 'htmx:beforeCleanupElement');
    });
}
// Run after document-level handlers so HTMX and cancelled clicks retain their streams.
// Plain links (list, detail and cross-workspace links) need the same cleanup as the rail.
window.addEventListener('click', function (event) {
    var link = event.target.closest('a[href]');
    if (link) closeLiveStreamsForNavigation(event, link);
});
// Native filter forms also navigate. Release streams after submit handlers have
// accepted the request, without interrupting HTMX requests or cancelled submits.
window.addEventListener('submit', function (event) {
    if (event.defaultPrevented) return;
    var form = event.target;
    var submitter = event.submitter;
    var method = submitter?.getAttribute('formmethod') || form.getAttribute('method') || 'get';
    if (method.toLowerCase() === 'dialog') return;
    var target = submitter?.getAttribute('formtarget') ?? form.getAttribute('target') ?? document.querySelector('base[target]')?.getAttribute('target');
    if (target && target.toLowerCase() !== '_self') return;
    closeLiveStreams();
});
document.addEventListener('click', function (event) {
    var control = event.target.closest('[data-action]');
    if (!control) return;
    switch (control.dataset.action) {
        // Accepted plain navigation is handled by the window listener after this dispatcher.
        case 'navigate-workspace': break;
        case 'confirm-logout': confirmLogout(); break;
        case 'select-sidebar-item': selectSidebarItem(control); break;
        case 'select-thread-row': selectThreadRow(control); break;
        case 'toggle-mailbox-sidebar': toggleMailboxSidebar(); break;
        case 'toggle-rail': toggleRail(); break;
        case 'close-rail': setRailOpen(false); break;
        case 'dismiss-request-error': hideRequestError(); break;
        case 'pane-back': setMobilePane('list'); break;
        case 'show-agent-tab': showAgentTab(control.dataset.tab); break;
        case 'toggle-agent-prompt': toggleAgentPromptGenerator(control.dataset.prefix); break;
        case 'hide-element': {
            var hidden = document.getElementById(control.dataset.target);
            if (hidden) hidden.classList.add('hidden');
            break;
        }
        case 'toggle-element': {
            var toggled = document.getElementById(control.dataset.target);
            if (toggled) toggled.classList.toggle('hidden');
            break;
        }
        case 'toggle-next': {
            if (control.nextElementSibling) control.nextElementSibling.classList.toggle('hidden');
            break;
        }
        case 'toggle-form-card': toggleFormCard(control); break;
        case 'toggle-prompt-generator': togglePromptGenerator(control); break;
        case 'select-company': selectCompany(control.dataset.companyId); break;
        case 'open-dialog': {
            var dialog = document.getElementById(control.dataset.dialog);
            if (dialog) dialog.showModal();
            break;
        }
        case 'close-dialog': {
            var open = control.closest('dialog');
            if (open) open.close();
            break;
        }
        case 'copy-text': copyTextFrom(control); break;
        case 'delete-library-agent': deleteLibraryAgent(control.dataset.agentId); break;
        // 'isolate' carries no behaviour. It exists so that `closest('[data-action]')` stops
        // here rather than resolving to a clickable ancestor -- the delegated stand-in for the
        // `event.stopPropagation()` these controls used to carry inline.
        case 'isolate': break;
    }
});
document.addEventListener('change', function (event) {
    var control = event.target.closest('[data-action]');
    if (!control) return;
    switch (control.dataset.action) {
        case 'theme-toggle': applyTheme(control.checked ? 'light' : 'dark'); break;
        case 'toggle-schedule-type': toggleScheduleType(control); break;
        case 'submit-form': control.form.requestSubmit(); break;
        case 'simulation-mode': {
            var sender = control.form.elements.namedItem('from');
            var live = control.value !== 'verify';
            if (live) sender.value = sender.dataset.serverSender;
            sender.disabled = live;
            break;
        }
        case 'library-multi-select': {
            var root = control.closest('[data-library-multi-select]');
            root.querySelector('input[type=hidden]').value = Array.from(root.querySelectorAll('input[type=checkbox]:checked')).map(function (item) { return item.value; }).join(',');
            break;
        }
        case 'capability-multi-select': {
            var capabilityRoot = control.closest('[data-capability-multi-select]');
            capabilityRoot.querySelector('input[type=hidden]').value = Array.from(capabilityRoot.querySelectorAll('input[type=checkbox]:checked')).map(function (item) { return item.value; }).join(',');
            break;
        }
        case 'model-provider': {
            var grid = control.closest('[data-model-connection]');
            var providerInput = control.nextElementSibling;
            var modelSelect = grid.querySelector('[data-model-select]');
            var modelInput = grid.querySelector('[data-model-input]');
            var custom = control.value === '__custom__';
            var models = control.value === 'google' ? ['gemini-3.6-flash', 'gemini-3.7-flash'] : control.value === 'openai' ? ['gpt-5.6-sol', 'gpt-5.6-terra'] : control.value === 'anthropic' ? ['claude-opus-5-5', 'claude-sonnet-5-5'] : control.value === 'xai' ? ['grok-4.6'] : [];
            providerInput.value = custom ? '' : control.value;
            providerInput.classList.toggle('hidden', !custom);
            modelSelect.replaceChildren(new Option(control.value ? 'Select model' : 'Select provider first', ''), ...models.map(function (model) { return new Option(model, model); }));
            modelSelect.disabled = !control.value || custom;
            modelSelect.classList.toggle('hidden', custom);
            modelInput.value = '';
            modelInput.classList.toggle('hidden', !custom);
            if (custom) providerInput.focus();
            break;
        }
        case 'model-select': control.nextElementSibling.value = control.value; break;
    }
});
document.addEventListener('input', function (event) {
    var control = event.target.closest('[data-input]');
    if (!control) return;
    switch (control.dataset.input) {
        case 'slugify': syncSlugField(control); break;
        case 'agent-address-preview': updateAgentAddressPreview(control); break;
        case 'agent-simple-address-preview': updateSimpleAgentAddressPreview(control); break;
        case 'spam-warning': toggleSpamWarning(control); break;
        case 'channel-spam-confirm': toggleChannelSpamConfirm(control); break;
        case 'auto-grow-composer': autoGrowComposer(control); break;
    }
});
document.addEventListener('keydown', function (event) {
    var control = event.target.closest('[data-keydown]');
    if (!control) return;
    switch (control.dataset.keydown) {
        // Enter submits the surrounding form by default, which throws away a part-filled
        // multi-field form the moment someone presses it in a single-line input.
        case 'block-enter':
            if (event.key === 'Enter' && event.target.tagName !== 'TEXTAREA') event.preventDefault();
            break;
        case 'composer': composerKeydown(event); break;
    }
});
document.addEventListener('submit', function (event) {
    var control = event.target.closest('[data-submit]');
    if (!control) return;
    switch (control.dataset.submit) {
        case 'busy-once':
            if (!markSubmitBusy(control)) event.preventDefault();
            break;
        case 'save-library-agent': saveLibraryAgent(event, control.dataset.agentId); break;
        case 'create-library-agent': createLibraryAgent(event); break;
    }
});
document.addEventListener('htmx:afterRequest', function (event) {
    var control = event.target.closest('[data-after-request]');
    if (!control || !event.detail.successful || event.detail.elt !== control) return;
    switch (control.dataset.afterRequest) {
        case 'reset-form': control.reset(); break;
        case 'reset-and-collapse': resetAndCollapseForm(control); break;
        case 'clear-cached-company': clearCachedCompanyIfMatch(control.dataset.companyId); break;
    }
});
document.addEventListener('htmx:afterSwap', function (event) {
    event.target.querySelectorAll('[data-after-swap="apply-generated-prompt"]').forEach(applyGeneratedPrompt);
});


        var requestErrorTimer = null;

        function hideRequestError() {
            window.clearTimeout(requestErrorTimer);
            requestErrorTimer = null;
            var toast = document.getElementById('request-error-toast');
            if (toast) toast.classList.add('hidden');
        }

        function showRequestError(message) {
            var toast = document.getElementById('request-error-toast');
            if (!toast) return;
            toast.querySelector('#request-error-message').textContent = message;
            toast.classList.remove('hidden');
            window.clearTimeout(requestErrorTimer);
            requestErrorTimer = window.setTimeout(hideRequestError, 10000);
        }

        // What the server said, when it said something a reader can act on. `/ui` error bodies are
        // short plain text; an HTML body is a page rather than a message, so it is reported by
        // status instead of dumped into the alert.
        function requestErrorMessage(xhr) {
            var status = xhr && xhr.status ? xhr.status : 0;
            var body = xhr && typeof xhr.responseText === 'string' ? xhr.responseText.trim() : '';
            if (!body || body.charAt(0) === '<' || body.length > 300) {
                body = status ? 'The server rejected the request.' : 'The server could not be reached.';
            }
            return status ? status + ' — ' + body : body;
        }

        document.body.addEventListener('htmx:responseError', function (event) {
            showRequestError(requestErrorMessage(event.detail && event.detail.xhr));
        });

        document.body.addEventListener('htmx:sendError', function () {
            showRequestError('Network error — the server could not be reached.');
        });

        document.body.addEventListener('htmx:timeout', function () {
            showRequestError('The request timed out. Try again.');
        });

        document.body.addEventListener('htmx:swapError', function () {
            showRequestError('The response could not be displayed. Reload the page.');
        });


        var buttonsAwaitingResponse = new Map();

        function pressedControl(detail) {
            var elt = detail && detail.elt;
            if (!elt || !elt.matches) return null;
            if (elt.matches('button, input[type=submit]')) return elt;

            // A form submit: the button that submitted it, which the triggering event names.
            var trigger = detail.requestConfig && detail.requestConfig.triggeringEvent;
            var submitter = trigger && trigger.submitter;
            if (submitter && elt.contains(submitter)) return submitter;

            // Only a form falls back to a search, and only for its own default submit button. Any
            // other element -- a polling div, a swapped pane -- would just be finding somebody
            // else's button somewhere inside it.
            if (!elt.matches('form')) return null;
            return elt.querySelector('button[type=submit], button:not([type])');
        }

        // The `[.htmx-request_&]` spinners and the `[data-progress]` SVG on the non-htmx forms are
        // already progress of their own, so those buttons are left alone.
        function ownsItsSpinner(button) {
            return !!button.querySelector('.loading, [data-progress]');
        }

        document.body.addEventListener('htmx:beforeRequest', function (event) {
            var button = pressedControl(event.detail);
            if (!button || ownsItsSpinner(button) || buttonsAwaitingResponse.has(event.detail.elt)) return;

            var spinner = document.createElement('span');
            spinner.className = 'loading loading-spinner loading-xs';
            spinner.setAttribute('data-request-spinner', '');
            button.prepend(spinner);
            button.setAttribute('aria-busy', 'true');
            // An icon-only button has nothing to sit beside, so the spinner takes the icon's place.
            var iconOnly = !button.textContent.trim();
            if (iconOnly) button.setAttribute('data-busy-icon-only', '');

            // `htmx:configRequest` has already gathered the parameters, so disabling the submitter
            // here cannot drop its own value from the body. A button htmx disabled itself through
            // `hx-disabled-elt` is left for htmx to re-enable -- re-enabling it twice, from two
            // owners, is how a button ends up permanently dead.
            var weDisabled = !button.disabled;
            if (weDisabled) button.disabled = true;

            buttonsAwaitingResponse.set(event.detail.elt, {
                button: button,
                weDisabled: weDisabled,
                iconOnly: iconOnly
            });
        });

        document.body.addEventListener('htmx:afterRequest', function (event) {
            var pending = buttonsAwaitingResponse.get(event.detail.elt);
            if (!pending) return;
            buttonsAwaitingResponse.delete(event.detail.elt);

            var spinner = pending.button.querySelector('[data-request-spinner]');
            if (spinner) spinner.remove();
            pending.button.removeAttribute('aria-busy');
            if (pending.iconOnly) pending.button.removeAttribute('data-busy-icon-only');
            if (pending.weDisabled) pending.button.disabled = false;
        });


(function () {
    function applyRunCounts(event) {
        var source = document.querySelector('[data-run-counts-source]');
        if (!source || !source.querySelector('template[data-run-counts-key="company"]')) return;
        var root = event.detail && event.detail.elt || event.target;
        if (!root || root === source || source.contains(root)) root = document;
        var templates = new Map();
        source.querySelectorAll('template[data-run-counts-key]').forEach(function (template) {
            templates.set(template.dataset.runCountsKey, template);
        });
        function apply(slot) {
            var template = templates.get(slot.dataset.runCounts);
            slot.replaceChildren(...(template ? [template.content.cloneNode(true)] : []));
        }
        if (root.matches && root.matches('[data-run-counts]')) apply(root);
        if (root.querySelectorAll) root.querySelectorAll('[data-run-counts]').forEach(apply);
    }
    document.addEventListener('htmx:afterSettle', applyRunCounts);
    document.addEventListener('htmx:oobAfterSwap', applyRunCounts);
})();


// A sample is read-only but may finish after navigation. A detached requesting form must
// not replace a newly selected workflow pane. Keep errors beside the author's raw inputs.
document.body.addEventListener('htmx:beforeRequest', function (event) {
    var form = event.detail.elt;
    if (!form || form.id !== 'workflow-sample-form') return;
    form.setAttribute('aria-busy', 'true');
    var status = form.querySelector('[data-sample-status]');
    if (status) status.textContent = 'Running sample…';
    form.addEventListener('htmx:beforeOnLoad', function (response) {
        if (!form.isConnected) response.preventDefault();
    }, { once: true });
});
document.body.addEventListener('htmx:afterRequest', function (event) {
    var form = event.detail.elt;
    if (!form || form.id !== 'workflow-sample-form') return;
    form.removeAttribute('aria-busy');
    if (!event.detail.successful) {
        var status = form.querySelector('[data-sample-status]');
        if (status) status.textContent = 'Sample request failed. Your input is still here; try again.';
    }
});
document.addEventListener('click', function (event) {
    var control = event.target.closest('[data-action="workflow-source-goto"]');
    if (!control) return;
    var field = document.getElementById('workflow-source');
    if (!field) return;
    var wanted = parseInt(control.dataset.line, 10);
    var lines = field.value.split('\n');
    if (!(wanted >= 1 && wanted <= lines.length)) return;
    var start = 0;
    for (var index = 0; index < wanted - 1; index++) start += lines[index].length + 1;
    field.focus();
    field.setSelectionRange(start, start + lines[wanted - 1].length);
    var lineHeight = parseFloat(getComputedStyle(field).lineHeight) || 16;
    field.scrollTop = Math.max(0, (wanted - 3) * lineHeight);
});
function workflowEditorDirty() {
    return document.querySelector('form[data-workflow-editor][data-dirty]');
}
document.addEventListener('input', function (event) {
    var form = event.target.closest && event.target.closest('form[data-workflow-editor]');
    if (form) form.setAttribute('data-dirty', '');
});
document.addEventListener('change', function (event) {
    var form = event.target.closest && event.target.closest('form[data-workflow-editor]');
    if (form) form.setAttribute('data-dirty', '');
});
document.body.addEventListener('htmx:beforeRequest', function (event) {
    var elt = event.detail.elt;
    if (!(elt && elt.matches && elt.matches('form[data-workflow-editor]'))) return;
    var submitter = event.detail.requestConfig && event.detail.requestConfig.triggeringEvent && event.detail.requestConfig.triggeringEvent.submitter;
    var status = document.getElementById('workflow-editor-status');
    if (status) status.textContent = submitter && submitter.value === 'save' ? 'Saving…' : 'Saving and validating…';
    elt.setAttribute('aria-busy', 'true');
});
// `htmx:confirm` precedes `htmx:beforeRequest`, so a cancelled request never reaches the shared
// busy-button handler, which only clears its spinner on `htmx:afterRequest` (F014).
document.body.addEventListener('htmx:confirm', function (event) {
    var elt = event.detail.elt;
    if (elt && elt.matches && elt.matches('form[data-workflow-editor]')) return;
    // Only a request that replaces the whole pane discards the editor; one that fills a region
    // of it (the reference, a compare) does not.
    var dirty = workflowEditorDirty();
    var target = event.detail.target;
    if (!dirty || !target || target.id !== 'workflow-pane') return;
    if (elt && elt.hasAttribute && elt.hasAttribute('data-editor-reload')) return;
    // Accepting keeps the mark: the swap removes the form, and a request that is then refused
    // (an archive confirm cancelled, a failed request) leaves the text still guarded.
    if (!window.confirm('The editor has changes that are not saved. Discard them?')) {
        event.preventDefault();
    }
});
document.body.addEventListener('htmx:afterRequest', function (event) {
    var elt = event.detail.elt;
    if (elt && elt.matches && elt.matches('form[data-workflow-editor]') && !event.detail.successful) {
        var status = document.getElementById('workflow-editor-status');
        if (status) status.textContent = 'Not saved: the request failed. Your text is still here.';
        elt.removeAttribute('aria-busy');
    }
    var body = document.getElementById('workflow-reference-body');
    if (body && elt && elt.id === 'workflow-reference' && !event.detail.successful) {
        body.removeAttribute('aria-busy');
        body.textContent = 'The reference could not be loaded. Close and reopen this panel to try again.';
        elt.setAttribute('hx-trigger', 'toggle');
        if (window.htmx) window.htmx.process(elt);
    }
});
// A card loaded after the page (`pages::lazy_card`) says so when its load fails.
document.body.addEventListener('htmx:afterRequest', function (event) {
    var elt = event.detail.elt;
    if (!(elt && elt.hasAttribute && elt.hasAttribute('data-lazy-card')) || event.detail.successful) return;
    elt.removeAttribute('aria-busy');
    elt.textContent = elt.getAttribute('data-lazy-card');
});
window.addEventListener('beforeunload', function (event) {
    if (!workflowEditorDirty()) return;
    event.preventDefault();
    event.returnValue = '';
});


var workflowAgentRequests = new WeakMap();
document.addEventListener('htmx:beforeRequest', function (event) {
    var element = event.detail.elt;
    if (!element.closest('[data-workflow-agent-picker]')) return;
    var source = document.getElementById('workflow-source');
    if (!source) return;
    var context = { field: source, value: source.value };
    var request = event.detail.xhr;
    workflowAgentRequests.set(request, context);
    element.addEventListener('htmx:beforeOnLoad', function (answer) {
        if (answer.detail.xhr !== request) return;
        if (!context.field.isConnected || document.getElementById('workflow-source') !== context.field || context.field.value !== context.value) {
            answer.preventDefault();
            var status = document.getElementById('workflow-agent-reference-result');
            if (status && context.field.isConnected) status.textContent = 'The source changed. Refresh the agent choices before applying a reference.';
        }
    }, { once: true });
}, true);
document.addEventListener('htmx:afterSwap', function (event) {
    var context = workflowAgentRequests.get(event.detail.xhr);
    var target = event.detail.target;
    var marker = target && target.querySelector('[data-workflow-agent-source]');
    if (!context || !marker || !context.field.isConnected || context.field.value !== context.value) return;
    context.field.value = marker.dataset.workflowAgentSource;
    context.field.dispatchEvent(new Event('input', { bubbles: true }));
    context.field.focus();
    marker.remove();
});


// A comment may return after another reviewer closed the decision. Keep the latest local
// draft in a read-only recovery panel; a comment never submitted these values.
document.addEventListener('htmx:beforeSwap', function (event) {
    var detail = event.detail;
    if (detail.target.id !== 'workflow-decision' || !detail.requestConfig.path.split('?')[0].endsWith('/comments')) return;
    var draft = detail.target.querySelector('form[id^="decision-inputs-"]');
    if (!draft) return;
    var incoming = document.createElement('template');
    incoming.innerHTML = detail.serverResponse;
    if (incoming.content.querySelector('form[id^="decision-inputs-"]')) return;
    var section = incoming.content.querySelector('#workflow-decision');
    if (!section) return;
    var values = {};
    new FormData(draft).forEach(function (value, key) {
        if (key === 'feedback' || key === 'data' || key.startsWith('data:')) values[key] = value;
    });
    var recovery = document.createElement('section');
    recovery.id = 'decision-draft-recovery';
    recovery.className = 'space-y-2';
    var title = document.createElement('h2');
    title.className = 'font-semibold';
    title.textContent = 'Your unfinished decision input';
    var explanation = document.createElement('p');
    explanation.textContent = 'Posting a comment did not submit this input.';
    var content = document.createElement('pre');
    content.className = 'whitespace-pre-wrap text-sm';
    content.textContent = JSON.stringify(values, null, 2);
    recovery.append(title, explanation, content);
    section.append(recovery);
    detail.serverResponse = incoming.innerHTML;
});
// A board filter or a card click marks itself and the region it replaces busy until its answer
// settles (pages/AGENTS.md), an aborted or failed one included. A mark never reaches a history
// snapshot: Back would restore a region that looks busy for good.
var busyMarks = new Set();
function clearBusy(node) {
    node.removeAttribute('aria-busy');
    busyMarks.delete(node);
}
document.addEventListener('htmx:beforeRequest', function (event) {
    var element = event.detail.elt;
    var selector = element.dataset && element.dataset.busy;
    if (!selector) return;
    var marked = [element, document.querySelector(selector)].filter(Boolean);
    marked.forEach(function (node) {
        node.setAttribute('aria-busy', 'true');
        busyMarks.add(node);
    });
    element.addEventListener('htmx:afterRequest', function () {
        marked.forEach(clearBusy);
    }, { once: true });
});
document.addEventListener('htmx:beforeHistorySave', function () {
    busyMarks.forEach(clearBusy);
});
// The board's reload leaf (`data-board-wake`) reads at most once per BOARD_WAKE_MS per tab, and
// always once more after the last wake-up of a burst (plan new-board-view 03). Spacing is counted
// from completion, as `ui_run_counts.rs`'s scheduler does: while any read of the board is in flight
// -- a wake-up's or a filter change's -- a wake-up is held, and the interval starts when that read
// settles (answered, failed or aborted). So reads never overlap however slow they are (a read that
// replaced the one before would starve the board), there is at most one per interval after the
// previous one ends, and a wake-up that arrived after a read began is always followed by another
// read. A held wake-up is replayed on whichever leaf is current then (`board-wake`), so a filter
// change that replaced the leaf meanwhile cannot drop it. Opening a run is the reader's own
// action: it reads at once unless a board read is in flight, and then right after it settles.
// Plain `throttle` drops the trailing event and `delay` starves under constant change.
var BOARD_WAKE_MS = 2000;
// A read that never settles must not hold the board forever; past this it no longer counts.
var BOARD_READ_STALE_MS = 30000;
var boardWake = { inFlight: new Map(), settled: 0, held: false, urgent: false, timer: null };
function boardReading() {
    var now = Date.now();
    boardWake.inFlight.forEach(function (started, xhr) {
        if (now - started > BOARD_READ_STALE_MS) boardWake.inFlight.delete(xhr);
    });
    return boardWake.inFlight.size > 0;
}
// A read of the board itself (its region or its columns), not a card opening the pane.
function isBoardRead(detail) {
    var target = detail.target;
    return !!(target && target.closest && target.closest('#run-board'));
}
function scheduleBoardWake() {
    if (!boardWake.held || boardReading() || boardWake.timer) return;
    var wait = boardWake.urgent ? 0 : Math.max(0, boardWake.settled + BOARD_WAKE_MS - Date.now());
    boardWake.timer = setTimeout(function () {
        boardWake.timer = null;
        if (boardReading()) return;
        var leaf = document.querySelector('[data-board-wake]');
        boardWake.held = false;
        boardWake.urgent = false;
        if (!leaf) return;
        // A leaf a filter change just swapped in may not be processed yet; processing is
        // idempotent, and without it the replayed wake-up would find no listener.
        htmx.process(leaf);
        htmx.trigger(leaf, 'board-wake');
    }, wait);
}
document.addEventListener('htmx:confirm', function (event) {
    var leaf = event.detail.elt;
    if (!leaf.hasAttribute || !leaf.hasAttribute('data-board-wake')) return;
    var trigger = event.detail.triggeringEvent;
    var type = trigger && trigger.type;
    if (type === 'board-wake') return;
    var reading = boardReading();
    var idle = !reading && !boardWake.timer && !boardWake.held;
    if (type === 'run-selected' ? !reading : idle && Date.now() - boardWake.settled >= BOARD_WAKE_MS) {
        return;
    }
    event.preventDefault();
    boardWake.held = true;
    if (type === 'run-selected') {
        boardWake.urgent = true;
        clearTimeout(boardWake.timer);
        boardWake.timer = null;
    }
    scheduleBoardWake();
});
document.addEventListener('htmx:beforeRequest', function (event) {
    if (!isBoardRead(event.detail)) return;
    var xhr = event.detail.xhr;
    var element = event.detail.elt;
    boardWake.inFlight.set(xhr, Date.now());
    // A read is starting: what it reads covers every wake-up so far (an event is sent after its
    // change commits), so none stays held; later ones are held until it settles.
    boardWake.held = false;
    boardWake.urgent = false;
    clearTimeout(boardWake.timer);
    boardWake.timer = null;
    // On the element itself (a replaced form's events no longer bubble), and for this request
    // only: an element with two requests in flight sees the first one's settle event twice.
    element.addEventListener('htmx:afterRequest', function settled(done) {
        if (done.detail.xhr !== xhr) return;
        element.removeEventListener('htmx:afterRequest', settled);
        boardWake.inFlight.delete(xhr);
        boardWake.settled = Date.now();
        scheduleBoardWake();
    });
});
function discardDetachedRunResponse(answer) {
    if (!answer.detail.target.isConnected) answer.preventDefault();
}
document.addEventListener('htmx:beforeRequest', function (event) {
    var element = event.detail.elt;
    if (!element.closest('#run-pane, #run-list, #run-board, #workflow-decision')) return;
    element.addEventListener('htmx:beforeOnLoad', discardDetachedRunResponse, { once: true });
});


document.addEventListener('htmx:sseMessage', function (event) {
    var pane = event.target;
    if (!pane || pane.id !== 'message-scroll') return;
    var limit = Number(pane.dataset.historyLimit);
    if (!Number.isInteger(limit) || limit < 1) return;
    var bubbles = pane.querySelectorAll(':scope > .chat[data-message-id]');
    var excess = bubbles.length - limit;
    if (excess <= 0) return;
    var anchor = bubbles[excess];
    var before = anchor.getBoundingClientRect().top;
    for (var index = 0; index < excess; index++) bubbles[index].remove();
    pane.scrollTop += anchor.getBoundingClientRect().top - before;
});
