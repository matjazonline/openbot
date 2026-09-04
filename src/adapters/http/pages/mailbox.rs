//! The `/ui` mailbox: a daisyUI three-column reader — channels, then that channel's threads,
//! then the selected thread's messages.
//!
//! Every column is swapped over htmx: picking a channel replaces the thread column, picking a
//! thread replaces the detail pane, and Compose puts a new-thread form in that same pane.

use super::*;

/// Who the mailbox is rendered for, as the top bar shows them.
pub struct MailboxUser<'a> {
    /// Their account id, which is what their own Team pane is keyed by.
    pub id: Uuid,
    pub username: &'a str,
    pub email: &'a EmailAddress,
    /// Their profile picture; `None` falls back to the letter bubble.
    pub avatar_url: Option<&'a AvatarUrl>,
    /// Whether they are a system operator, and so entitled to the cross-company workspaces.
    /// Decided once per request from [`AppConfig::is_operator`] -- see `routes::ui::workspace_user`.
    pub is_operator: bool,
    /// What this account is to the company currently anchoring the rail.
    pub company_membership: CompanyMembership,
}

impl MailboxUser<'_> {
    pub fn with_company_membership(mut self, membership: CompanyMembership) -> Self {
        self.company_membership = membership;
        self
    }
}

/// Which `/ui` workspace a response belongs to, i.e. which rail icon is lit.
///
/// Every workspace renders through [`ui_shell`], which uses this to highlight its rail icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSection {
    Mailbox,
    Dashboard,
    Channels,
    Agents,
    Schedules,
    Tasks,
    Deliveries,
    Companies,
    Invites,
    /// The signed-in account's own details. Reached from the account menu rather than the rail --
    /// it is the one `/ui` page that is about the reader instead of about a company.
    Profile,
}

/// The confirmation asked for before a session ends: both logout triggers open this rather than
/// posting straight away, so a stray click on the rail cannot sign anyone out.
const LOGOUT_MODAL: &str = r##"
        <dialog id="logout-modal" class="modal">
            <div class="modal-box">
                <h3 class="text-lg font-bold">Log out?</h3>
                <p class="py-4 opacity-70">You will need to sign in again to reach your mailbox.</p>
                <div class="modal-action">
                    <form method="dialog">
                        <button class="btn btn-ghost">Cancel</button>
                    </form>
                    <form method="post" action="/logout">
                        <button type="submit" class="btn btn-error">Log out</button>
                    </form>
                </div>
            </div>
            <form method="dialog" class="modal-backdrop">
                <button>close</button>
            </form>
        </dialog>
        "##;

/// One quiet, shared status for every live region on the page. A mailbox can own several SSE
/// connections at once, so connection state belongs in the shell rather than beside one pane.
const LIVE_UPDATE_STATUS: &str = r##"
        <div id="live-update-status" role="status" aria-live="polite" aria-atomic="true"
            class="alert alert-warning pointer-events-none fixed left-1/2 top-4 z-50 hidden w-auto max-w-[calc(100vw-2rem)] -translate-x-1/2 px-4 py-2 text-sm font-medium shadow-lg">
            <span>Live updates paused. Reconnecting&hellip;</span>
        </div>
        "##;

/// One shared place for "that request failed", for every `/ui` page.
///
/// htmx does not swap a non-2xx response, so without somewhere for the reason to land a failed
/// request leaves the page looking untouched. [`REQUEST_ERROR_SCRIPT`] fills this in.
const REQUEST_ERROR_TOAST: &str = r##"
        <div id="request-error-toast" role="alert" aria-live="assertive" aria-atomic="true"
            class="toast toast-top toast-center z-50 hidden max-w-[calc(100vw-2rem)]">
            <div class="alert alert-error shadow-lg">
                <span id="request-error-message" class="text-sm font-medium break-words"></span>
                <button type="button" class="btn btn-ghost btn-xs" data-action="dismiss-request-error"
                    aria-label="Dismiss">&times;</button>
            </div>
        </div>
        "##;

/// The chrome every `/ui` response shares: the top bar, the icon rail, and the HTML shell around
/// them. Only what sits to the right of the rail differs between workspaces.
pub struct UiShell<'a> {
    pub title: &'a str,
    pub user: &'a MailboxUser<'a>,
    /// The company every workspace above the rail is scoped to, and whose face the rail ends on.
    /// `None` for a user with no company yet — there is nothing for the rail to point at.
    pub company: Option<&'a Company>,
    pub section: UiSection,
    /// Everything to the right of the icon rail: the sidebar and its panes.
    pub content: &'a str,
}

/// The full HTML document for one `/ui` response.
pub fn ui_shell(shell: &UiShell<'_>) -> String {
    let rail = match shell.company {
        Some(company) => icon_rail(shell.user, company, shell.section),
        None => String::new(),
    };

    let body = format!(
        r##"
    <div class="app-shell flex flex-col">
        {top_bar}
        <div class="app-workspace flex min-h-0 flex-1">
            {rail}
            {content}
        </div>
    </div>
    <div id="rail-backdrop" data-action="close-rail" aria-hidden="true"></div>
    {LIVE_UPDATE_STATUS}
    {REQUEST_ERROR_TOAST}
    {LOGOUT_MODAL}
        "##,
        top_bar = top_bar(shell.user, shell.company),
        content = shell.content,
    );

    ui_layout(shell.title, &body)
}

/// The page a `/ui` navigation gets when the request failed outright.
///
/// A browser typing a URL cannot use the toast -- there is no page left to put it on -- and the
/// plain-text body `AppError` and axum's extractor rejections answer with renders as a bare wall of
/// text. This is that same reason, in the app's own chrome.
pub fn ui_error_page(status: u16, reason: &str) -> String {
    let heading = match status {
        400..=403 => "That request was refused",
        404 => "Not found",
        409 => "That conflicts with something already there",
        422 => "That form could not be read",
        _ => "Something went wrong",
    };

    let body = format!(
        r##"
    <div class="flex min-h-full items-center justify-center p-6">
        <div class="card w-full max-w-lg border border-base-300 bg-base-200 shadow-lg">
            <div class="card-body gap-4">
                <div class="alert alert-error text-sm">
                    <span class="font-medium">{status} — {heading}</span>
                </div>
                <p class="text-sm opacity-70">{reason}</p>
                <div class="card-actions justify-end">
                    <a href="/ui" class="btn btn-primary btn-sm">Back to the mailbox</a>
                </div>
            </div>
        </div>
    </div>
        "##,
        heading = heading,
        reason = escape_html_text(reason),
    );

    ui_layout(heading, &body)
}

/// The whole mailbox shell for one request.
pub struct MailboxPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub company: &'a Company,
    pub companies: &'a [Company],
    /// Domain the channel addresses are built on, e.g. `mailagents.com`.
    pub app_domain_name: &'a str,
    pub channels: &'a [Channel],
    pub selected_channel: Option<&'a Channel>,
    pub threads: &'a [Thread],
    pub next_cursor: Option<&'a str>,
    pub selected_thread_id: Option<Uuid>,
    /// What each listed thread is doing, for the row badges.
    pub activity: &'a HashMap<Uuid, ThreadActivity>,
    /// Pre-rendered right-hand pane: messages, the compose form, or a placeholder.
    pub detail_html: &'a str,
}

/// The thread column for one channel.
pub struct ThreadColumn<'a> {
    pub company_id: Uuid,
    pub channel: &'a Channel,
    /// Domain the channel addresses are built on, e.g. `mailagents.com` -- what tells a
    /// participant that is another channel apart from one that is a person.
    pub app_domain_name: &'a str,
    pub threads: &'a [Thread],
    pub next_cursor: Option<&'a str>,
    pub selected_thread_id: Option<Uuid>,
    /// What each thread is doing, for the row badges. Threads with nothing in flight are absent.
    pub activity: &'a HashMap<Uuid, ThreadActivity>,
}

/// The detail pane showing one thread's messages.
pub struct MessagePane<'a> {
    pub company_id: Uuid,
    pub channel: &'a Channel,
    pub thread: &'a Thread,
    pub messages: &'a [ThreadMessageView],
    /// The face and name the agent side of this thread is drawn with -- see [`message_bubble_chat`].
    pub agent: Option<&'a Agent>,
    /// The signed-in reader's address, used to put their own messages on the right.
    pub viewer_email: &'a EmailAddress,
    /// What this thread is doing right now, for the strip above the composer.
    pub activity: Option<ThreadActivity>,
}

/// The detail pane showing the new-message form for a thread that is already open.
pub struct ReplyPane<'a> {
    pub company_id: Uuid,
    pub channel: &'a Channel,
    pub thread: &'a Thread,
    /// The channel's inbound address, e.g. `support@acme.mailagents.com`.
    pub channel_address: &'a str,
    pub sender_email: &'a str,
    pub text_body: &'a str,
    pub deliver: bool,
    pub quiet: bool,
    pub error: Option<&'a str>,
}

/// The detail pane showing the new-thread form.
pub struct ComposePane<'a> {
    pub company_id: Uuid,
    pub channel: &'a Channel,
    /// The channel's inbound address, e.g. `support@acme.mailagents.com`.
    pub channel_address: &'a str,
    pub sender_email: &'a str,
    pub subject: &'a str,
    pub text_body: &'a str,
    pub deliver: bool,
    pub quiet: bool,
    pub error: Option<&'a str>,
}

/// Client-side behaviour for the mailbox: selection highlighting, and keeping an opened thread
/// scrolled to its newest message — every load is htmx.
///
/// Kept out of the `format!` blocks below so its braces need no escaping.
pub(crate) const MAILBOX_SCRIPT: &str = r##"        // The `theme-controller` checkbox already repaints the page by itself -- daisyUI matches
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

            // The open thread's activity strip appearing makes the pane taller, which would push
            // the newest message out of view for someone sitting at the live edge.
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
        });"##;

/// Surfaces every failed `/ui` request as the shared [`REQUEST_ERROR_TOAST`] alert.
///
/// htmx leaves the page untouched on a non-2xx answer, so a rejected form body or a handler that
/// returned an error would otherwise look exactly like a click that did nothing. Errors that never
/// reach a handler -- an extractor rejection, a dropped connection, a timeout -- land here too,
/// which is the whole point of listening on the events rather than rendering an alert per route.
pub(crate) const REQUEST_ERROR_SCRIPT: &str = r##"
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
"##;

/// Puts a spinner on the button that is waiting for the server, for every button, without each
/// one carrying its own markup.
///
/// htmx hangs the request on either the button itself (`hx-post` on a button) or on the form (a
/// submit), so both shapes are resolved back to the one control the reader actually pressed. A
/// button that already shows its own progress keeps doing it -- nothing ends up with two spinners.
pub(crate) const BUTTON_BUSY_SCRIPT: &str = r##"
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
"##;

/// Applies the reader's saved theme before the first paint, so returning to a light-theme page
/// never flashes dark first. That is why it is inline in `<head>` rather than in
/// [`MAILBOX_SCRIPT`] at the end of `<body>` -- from there it would arrive after the paint.
///
/// With nothing saved yet we follow the operating system, and fall back to dark, which is what
/// `/ui` looked like before there was a choice to make.
const THEME_INIT_SCRIPT: &str = r#"
        (function () {
            var saved = null;
            try { saved = localStorage.getItem('ui_theme'); } catch (e) {}
            if (saved !== 'light' && saved !== 'dark') {
                saved = window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
            }
            document.documentElement.setAttribute('data-theme', saved);
        })();"#;

/// The HTML shell for every `/ui` response: daisyUI over the Tailwind browser build, plus htmx.
fn ui_layout(title: &str, body: &str) -> String {
    let app_css = crate::adapters::http::routes::assets::app_css_url();
    let app_js = crate::adapters::http::routes::assets::app_js_url();
    let theme_init_js = crate::adapters::http::routes::assets::theme_init_js_url();
    format!(
        r##"<!DOCTYPE html>
<html lang="en" data-theme="dark" class="h-full">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title} - Mail Agents</title>
    <link href="{app_css}" rel="stylesheet" type="text/css" />
    <style>{SPINNER_STYLES}{BRAND_LOGO_STYLES}{DARK_THEME_BLUES}{FIELD_STYLES}{APP_SHELL_STYLES}{THREAD_ROW_STYLES}{MAILBOX_LAYOUT_STYLES}{COMPACT_LAYOUT_STYLES}</style>
    <script src="{theme_init_js}"></script>
    <script src="/assets/htmx-2.0.4.min.js" defer></script>
    <script src="/assets/htmx-ext-sse-2.2.3.js" defer></script>
</head>
<body class="h-full overflow-hidden bg-base-100 text-base-content">
{body}
    <script src="{app_js}"></script>
</body>
</html>"##
    )
}

pub(crate) fn application_javascript() -> String {
    format!(
        "var CHIP_SELECTED_MARK = {selected:?};\nvar CHIP_ADD_MARK = {add:?};\nvar AGENT_REPLIED_MARK = {replied:?};\n{app}\n{legacy}\n{mailbox}\n{local}\n{skeletons}\n{schedules}\n{agents}\n{channels}\n{library}\n{delegation}\n{request_errors}\n{button_busy}",
        selected = icon(Icon::Check, BUTTON_ICON),
        add = icon(Icon::Plus, BUTTON_ICON),
        replied = icon(Icon::Check, BUTTON_ICON),
        app = super::layout::APP_SCRIPT,
        legacy = super::layout::LEGACY_FORMS_SCRIPT,
        mailbox = MAILBOX_SCRIPT,
        local = LOCAL_TIME_SCRIPT,
        skeletons = skeleton_script(),
        schedules = super::schedules::SCHEDULES_SCRIPT,
        agents = super::agent_settings::AGENT_SETTINGS_SCRIPT,
        channels = super::channel_settings::CHANNEL_SETTINGS_SCRIPT,
        library = super::agent_library_multi_select::AGENT_LIBRARY_SCRIPT,
        delegation = EVENT_DELEGATION_SCRIPT,
        request_errors = REQUEST_ERROR_SCRIPT,
        button_busy = BUTTON_BUSY_SCRIPT,
    )
}

const EVENT_DELEGATION_SCRIPT: &str = r##"
function closeLiveStreamsForNavigation(event, link) {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || link.hasAttribute('download')) return;
    var target = link.getAttribute('target');
    if (target && target.toLowerCase() !== '_self') return;
    if (!window.htmx) return;
    document.querySelectorAll('[sse-connect], [data-sse-connect]').forEach(function (owner) {
        window.htmx.trigger(owner, 'htmx:beforeCleanupElement');
    });
}
document.addEventListener('click', function (event) {
    var control = event.target.closest('[data-action]');
    if (!control) return;
    switch (control.dataset.action) {
        case 'confirm-logout': confirmLogout(); break;
        case 'select-sidebar-item': selectSidebarItem(control); break;
        case 'select-thread-row': selectThreadRow(control); break;
        case 'toggle-mailbox-sidebar': toggleMailboxSidebar(); break;
        case 'toggle-rail': toggleRail(); break;
        case 'close-rail': setRailOpen(false); break;
        case 'dismiss-request-error': hideRequestError(); break;
        case 'navigate-workspace': closeLiveStreamsForNavigation(event, control); break;
        case 'pane-back': setMobilePane('list'); break;
        case 'show-agent-tab': showAgentTab(control.dataset.tab); break;
        case 'toggle-agent-prompt': toggleAgentPromptGenerator(control.dataset.prefix); break;
        case 'show-channel-tab': showChannelTab(control.dataset.tab); break;
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
        case 'show-channel-form-tab': showChannelFormTab(control.dataset.tab); break;
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
        case 'pick-channel-library-agent': pickChannelLibraryAgent(control); break;
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
        case 'toggle-schedule-delivery': toggleScheduleDelivery(control); break;
        case 'sync-channel-agents': syncChannelAgents(control); break;
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
        case 'model-provider': {
            var grid = control.closest('[data-model-connection]');
            var providerInput = control.nextElementSibling;
            var modelSelect = grid.querySelector('[data-model-select]');
            var modelInput = grid.querySelector('[data-model-input]');
            var custom = control.value === '__custom__';
            var models = control.value === 'google' ? ['gemini-3.6-flash', 'gemini-3.7-flash'] : control.value === 'openai' ? ['gpt-5.6-sol', 'gpt-5.6-terra'] : [];
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
        case 'pick-agent-radio': selectAgentInSelection(control, false); break;
        case 'pick-agent-library': selectAgentInSelection(control, true); break;
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
"##;

pub(crate) fn theme_init_javascript() -> &'static str {
    THEME_INIT_SCRIPT
}

pub fn mailbox_page(page: &MailboxPage<'_>) -> String {
    let thread_column_html = match page.selected_channel {
        Some(channel) => thread_column(&ThreadColumn {
            company_id: page.company.id,
            channel,
            app_domain_name: page.app_domain_name,
            threads: page.threads,
            next_cursor: page.next_cursor,
            selected_thread_id: page.selected_thread_id,
            activity: page.activity,
        }),
        None => empty_thread_column(),
    };

    let content = format!(
        "{sidebar}<div class=\"ui-pane-list flex min-h-0 w-96 shrink-0 flex-col\">{compact_header}{thread_column_html}</div>{detail_html}",
        sidebar = channel_sidebar(page),
        compact_header = compact_mailbox_header(page),
        detail_html = page.detail_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Mailbox", page.company.name),
        user: page.user,
        company: Some(page.company),
        section: UiSection::Mailbox,
        content: &content,
    })
}

/// Shown when the signed-in user has no company yet — there is nothing to put in the columns.
pub fn mailbox_no_company_page(user: &MailboxUser<'_>) -> String {
    let content = r##"
        <div class="flex flex-1 items-center justify-center p-8">
            <div class="card w-full max-w-md bg-base-200 shadow-xl">
                <div class="card-body items-center text-center">
                    <h2 class="card-title">No companies yet</h2>
                    <p class="text-sm opacity-70">Create a company to get channels, threads and a mailbox.</p>
                    <div class="card-actions mt-4">
                        <a href="/ui/companies" class="btn btn-primary">Go to Companies</a>
                    </div>
                </div>
            </div>
        </div>
        "##;

    ui_shell(&UiShell {
        title: "Mailbox",
        user,
        company: None,
        section: UiSection::Mailbox,
        content,
    })
}

/// The wordmark, in both inks: the dark one for the light theme, the light one for the dark
/// theme. Both are sent, and [`BRAND_LOGO_STYLES`] hides the one that does not belong -- swapping
/// the `src` from script instead would leave the mark blank for a moment on every theme change.
fn brand_logo() -> String {
    let logo_light = crate::adapters::http::routes::assets::logo_light_url();
    format!(
        r##"
                <img src="/assets/busybots-logo-dark-hor.png" alt="BusyBots"
                    class="brand-logo brand-logo-on-light h-10 w-auto">
                <img src="{logo_light}" alt="BusyBots"
                    class="brand-logo brand-logo-on-dark h-10 w-auto">
"##
    )
}

/// Which of [`brand_logo`]'s two inks the current theme shows. Written against `data-theme` on
/// `<html>`, which `THEME_INIT_SCRIPT` sets before the first paint and `applyTheme` keeps current.
///
/// Dark is the default here as it is everywhere else: an unset `data-theme` is the dark theme.
const BRAND_LOGO_STYLES: &str = r##"
        .brand-logo-on-light { display: none; }
        [data-theme="light"] .brand-logo-on-dark { display: none; }
        [data-theme="light"] .brand-logo-on-light { display: block; }
"##;

/// The two blues the dark theme paints with, pulled back from daisyUI's own.
///
/// Stock `dark` puts primary at `oklch(58% 0.233 277)` -- a light, electric indigo that glares
/// against a `base-100` this dark, and is not the mark's colour either. Both are re-cut here at
/// the wordmark's hue (`#0000ff` is `oklch(45% 0.313 264)`) and darker, so a `btn-primary` sits
/// *in* the surface rather than on top of it, and so the light ink it carries gains contrast as
/// the field loses it. `info` follows primary down by the same amount -- the dashboard draws the
/// two side by side, and one of them staying at full brightness is what would read as a mistake.
///
/// Overriding the variables rather than the components keeps every `primary`/`info` utility --
/// buttons, badges, toggles, chart strokes -- on one definition. Dark only: the light theme
/// carries these colours over a white field, where they were never too loud to begin with.
const DARK_THEME_BLUES: &str = r##"
        [data-theme="dark"] {
            --color-primary: oklch(50% 0.19 264.05);
            --color-primary-content: oklch(96% 0.02 264.05);
            --color-info: oklch(66% 0.13 232.661);
        }
"##;

/// The one ring a focused field gets, and how round every field is.
///
/// daisyUI paints a focused field with both its own 1px border *and* a 2px outline held 2px
/// away from it -- three concentric edges, which reads as a double border. Pulling the outline
/// 1px inward lays it exactly over the border, so one 2px ring is all that shows. An outline is
/// not part of layout, so nothing moves when a field takes focus.
///
/// `--radius-field` is the corner radius daisyUI gives every field-sized component, so setting
/// it here rounds inputs, selects, textareas, buttons and tabs together rather than leaving each
/// to pick its own. Both shipped themes set it to `.25rem`.
///
/// It needs `!important` where [`DARK_THEME_BLUES`] does not. daisyUI hangs the light theme off
/// `:root:has(input.theme-controller[value=light]:checked)` as well as `[data-theme=light]`, and
/// our theme switch is exactly that checkbox -- so in light the `:has` selector outweighs
/// anything we could write at token level, and only in dark would the corners round. Overriding
/// the priority beats matching daisyUI's selector shape, which is theirs to change.
///
/// Only the four text-entry fields are pulled in. A checkbox, toggle or range shares the offset
/// outline but has no interior to hold a ring, so those keep their halo.
const FIELD_STYLES: &str = r##"
        :root, [data-theme] { --radius-field: .5rem !important; }

        .input:focus, .input:focus-within,
        .select:focus, .select:focus-within, .select:open,
        .textarea:focus, .textarea:focus-within,
        .file-input:focus, .file-input:focus-within { outline-offset: -1px; }

        .form-control > .label { white-space: normal; }
"##;

/// Shared visual language for every authenticated workspace.
///
/// The pages deliberately keep their layout utilities close to their markup, while the shell
/// owns the product-wide qualities that should never drift between workspaces: typography,
/// surface depth, navigation states, motion, scrollbars and accessible keyboard focus.
const APP_SHELL_STYLES: &str = r##"
        :root {
            color-scheme: light dark;
            font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont,
                "Segoe UI", sans-serif;
            font-synthesis: none;
            text-rendering: optimizeLegibility;
        }

        body {
            background-image:
                radial-gradient(circle at 18% -10%, color-mix(in oklab, var(--color-primary) 9%, transparent), transparent 28rem),
                linear-gradient(135deg, color-mix(in oklab, var(--color-base-200) 35%, transparent), transparent 42%);
        }

        ::selection {
            background: color-mix(in oklab, var(--color-primary) 28%, transparent);
        }

        :where(a, button, input, select, textarea):focus-visible {
            outline: 2px solid var(--color-primary);
            outline-offset: 2px;
        }

        .app-topbar {
            background: color-mix(in oklab, var(--color-base-100) 88%, transparent);
            box-shadow: 0 1px 0 color-mix(in oklab, var(--color-base-content) 7%, transparent);
            backdrop-filter: blur(18px) saturate(140%);
        }

        .app-rail {
            scrollbar-width: none;
            background: color-mix(in oklab, var(--color-base-200) 86%, var(--color-base-300));
        }
        .app-rail::-webkit-scrollbar { display: none; }
        .app-rail .btn {
            position: relative;
            min-height: 2.75rem;
            transition: background-color 160ms ease, color 160ms ease, transform 160ms ease;
        }
        .app-rail .btn:hover { transform: translateY(-1px); }
        .app-rail .btn-primary::before {
            position: absolute;
            left: -.55rem;
            width: 3px;
            height: 1.5rem;
            border-radius: 999px;
            background: currentColor;
            content: "";
        }

        aside.bg-base-200 {
            background: color-mix(in oklab, var(--color-base-200) 90%, transparent);
        }

        .workspace-heading h2 {
            letter-spacing: -.025em;
        }

        .menu :where(li > a, li > button) {
            border-radius: .625rem;
            transition: background-color 140ms ease, color 140ms ease, transform 140ms ease;
        }
        .menu :where(li > a, li > button):hover { transform: translateX(1px); }
        .menu .menu-active {
            box-shadow: inset 3px 0 0 var(--color-primary);
            font-weight: 650;
        }

        .card, .modal-box, section.rounded-box {
            border-color: color-mix(in oklab, var(--color-base-content) 10%, transparent);
            box-shadow: 0 1px 2px color-mix(in oklab, #000 10%, transparent),
                0 12px 32px color-mix(in oklab, #000 5%, transparent);
        }

        .btn { font-weight: 650; letter-spacing: -.01em; }
        .badge { font-weight: 650; }
        .table :where(th) {
            font-size: .6875rem;
            letter-spacing: .06em;
            text-transform: uppercase;
            opacity: .65;
        }

        * { scrollbar-color: color-mix(in oklab, var(--color-base-content) 18%, transparent) transparent; }

        @media (prefers-reduced-motion: reduce) {
            *, *::before, *::after {
                scroll-behavior: auto !important;
                transition-duration: .01ms !important;
                animation-duration: .01ms !important;
                animation-iteration-count: 1 !important;
            }
        }
"##;

/// The activity mark on a row whose reply the reader has just watched arrive: none.
///
/// Hiding rather than emptying the slot is what keeps the column honest afterwards. The badge for
/// that thread keeps arriving and keeps being swapped into the hidden slot, so its mark is already
/// current the moment `selectThreadRow` lifts the class and the row speaks again.
const THREAD_ROW_STYLES: &str = r##"
        .thread-row.thread-replied .thread-activity { display: none; }
"##;

/// The channel sidebar becomes an overlay when the viewport cannot comfortably hold all three
/// mailbox columns. Its compact replacement stays above the thread header: a quick channel
/// dropdown plus a button that reveals the full mailbox, including channel addresses and actions.
///
/// On a phone -- see [`COMPACT_LAYOUT_STYLES`] -- the rail has left the flow too, so the overlay
/// starts at the edge of the window rather than beside a column that is no longer there.
const MAILBOX_LAYOUT_STYLES: &str = r##"
        .mailbox-compact-header, .mailbox-sidebar-close { display: none; }

        @media (max-width: 79.999rem) {
            #mailbox-sidebar {
                display: none;
                position: fixed;
                z-index: 35;
                top: 4rem;
                bottom: 0;
                left: 4rem;
                box-shadow: 12px 0 32px color-mix(in oklab, #000 22%, transparent);
            }
            #mailbox-sidebar.mailbox-sidebar-open { display: flex; }
            .mailbox-compact-header { display: flex; }
            .mailbox-sidebar-close { display: inline-flex; }
        }

        @media (max-width: 47.999rem) {
            #mailbox-sidebar {
                left: 0;
                width: min(20rem, 85vw);
                z-index: 46;
            }
        }
"##;

/// What a workspace becomes on a phone: one column at a time, with the rail as a drawer over it.
///
/// The three-column reader assumes a window wide enough to hold a list *and* what it points at.
/// Below `48rem` there is room for one of them, so the shell stops laying the columns out side by
/// side and starts showing whichever one the reader is actually in: the list until they pick
/// something, the detail until they come back. `data-pane` on `<body>` is that one bit of state --
/// written by `syncMobilePane` on load and by the pane swaps themselves, and read only here, so
/// nothing about the wider layout depends on it.
///
/// A workspace opts in by naming its columns rather than by being special-cased: `ui-pane-list`
/// on the column that lists, `ui-pane-detail` on the column that shows one thing, `ui-split` on a
/// pair of columns that should stack rather than drill, and `ui-pane-stacked` on the one of that
/// pair that sits on top -- the Dashboard's filters, the Team tab's members, a schedule's runs.
const COMPACT_LAYOUT_STYLES: &str = r##"
        /* `dvh` follows a mobile browser's collapsing toolbars; `vh` does not, and leaves the
           composer under the address bar. The `vh` line is the fallback for browsers without it. */
        .app-shell { height: 100vh; height: 100dvh; }

        #rail-backdrop { display: none; }
        .ui-mobile-only, .ui-mobile-back, .rail-label { display: none; }

        @media (max-width: 47.999rem) {
            .ui-desktop-only { display: none; }
            .ui-mobile-only { display: inline-flex; }
            body[data-pane="detail"] .ui-mobile-back { display: inline-flex; }

            /* The rail slides in over the workspace, wide enough here to name its destinations --
               a column of unlabelled glyphs is affordable beside content, not on top of it. */
            .app-rail {
                position: fixed;
                top: 4rem;
                bottom: 0;
                left: 0;
                z-index: 47;
                width: 15rem;
                align-items: stretch;
                transform: translateX(-100%);
                transition: transform 180ms ease;
                box-shadow: 12px 0 32px color-mix(in oklab, #000 28%, transparent);
            }
            body[data-rail="open"] .app-rail { transform: none; }
            .app-rail .btn {
                width: 100%;
                justify-content: flex-start;
                gap: .75rem;
                padding-inline: .75rem;
            }
            .app-rail .btn-primary::before { left: .125rem; }
            .rail-label { display: inline; }

            #rail-backdrop {
                display: block;
                position: fixed;
                inset: 4rem 0 0 0;
                z-index: 46;
                background: color-mix(in oklab, #000 45%, transparent);
                opacity: 0;
                pointer-events: none;
                transition: opacity 180ms ease;
            }
            body[data-rail="open"] #rail-backdrop { opacity: 1; pointer-events: auto; }

            /* Stacking the workspace lets a column that is on its own take the full window, and
               lets the two that are not sit above one another instead of shoulder to shoulder. */
            .app-workspace { flex-direction: column; }

            .ui-pane-list, .ui-pane-detail {
                width: 100%;
                min-width: 0;
                min-height: 0;
                flex: 1 1 auto;
                border-right-width: 0;
            }
            body[data-pane="detail"] .ui-pane-list { display: none; }
            body:not([data-pane="detail"]) .ui-pane-detail { display: none; }

            .ui-split { flex-direction: column; }
            .ui-pane-stacked {
                width: 100%;
                flex: 0 0 auto;
                max-height: 45vh;
                overflow-y: auto;
                border-right-width: 0;
                border-bottom: 1px solid var(--color-base-300);
            }
        }
"##;

/// The light/dark switch in the top bar: daisyUI's `theme-controller` checkbox wrapped in a
/// `swap`, so the box flips the theme in CSS on its own and the icon rotates with it.
/// `applyTheme` in [`MAILBOX_SCRIPT`] only has to write the choice down and keep `data-theme`
/// in agreement with the box.
///
/// The icon shows the theme the click would take you *to*, not the one you are already in.
const THEME_CONTROLLER: &str = r##"
                <label class="swap swap-rotate btn btn-ghost btn-circle" title="Switch between light and dark">
                    <input id="theme-toggle" type="checkbox" class="theme-controller" value="light"
                        aria-label="Switch between light and dark"
                        data-action="theme-toggle" />
                    <svg class="swap-off h-5 w-5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M12 3v2.25m6.364.386-1.591 1.591M21 12h-2.25m-.386 6.364-1.591-1.591M12 18.75V21m-4.773-4.227-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0Z" />
                    </svg>
                    <svg class="swap-on h-5 w-5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M21.752 15.002A9.72 9.72 0 0 1 18 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 0 0 3 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 0 0 9.002-5.998Z" />
                    </svg>
                </label>
"##;

/// The bar across the top of every mailbox response: the brand mark on the left, the company the
/// response is scoped to in the middle, the signed-in account on the right. It sits above all four
/// columns, so it is the one piece of chrome that never moves when htmx swaps a column.
///
/// The brand mark comes in two inks, one per theme -- see [`BRAND_LOGO_STYLES`]. Nothing in the
/// account menu is scoped to a company, so apart from the middle group the bar renders the same
/// for a reader who has no company yet.
///
/// The two outer groups are given the same `flex-1 basis-0`, so they split whatever the middle
/// group leaves and the company name sits on the bar's centre line rather than merely between its
/// neighbours -- which would drift as the account's name grows.
fn top_bar(user: &MailboxUser<'_>, company: Option<&Company>) -> String {
    let brand_logo = brand_logo();
    let company_name = match company {
        Some(company) => topbar_company(company, FragmentSwap::Inline),
        None => String::new(),
    };

    format!(
        r##"
        <header class="app-topbar navbar z-40 h-16 min-h-16 shrink-0 gap-2 border-b border-base-300 px-3 sm:gap-4 sm:px-4 lg:px-5">
            <div class="flex flex-1 basis-0 items-center gap-1">
{rail_toggle}
                <button type="button" class="ui-mobile-back btn btn-ghost btn-square btn-sm"
                    title="Back to the list" aria-label="Back to the list" data-action="pane-back">{back}</button>
                <a href="/ui" class="ui-desktop-only flex items-center" title="BusyBots">
{brand_logo}
                </a>
            </div>
            <div class="flex min-w-0 items-center justify-center">
                {company_name}
            </div>
            <div class="flex flex-1 basis-0 items-center justify-end gap-1">
{THEME_CONTROLLER}
                <div class="dropdown dropdown-end">
                    <div tabindex="0" role="button" class="btn btn-ghost h-auto gap-3 px-2 py-1">
                        {chip}
                    </div>
                    <ul tabindex="0" class="menu menu-sm dropdown-content z-50 mt-3 w-64 rounded-box border border-base-300 bg-base-100 p-2 shadow-2xl">
                        <li class="menu-title truncate">{email}</li>
                        <li><a href="/ui/profile">Profile</a></li>
                        <li><a href="/ui/invites">My Invites</a></li>
{agent_library}
                        <li>
                            <button type="button" class="w-full text-left" data-action="confirm-logout">{sign_out} Log out</button>
                        </li>
                    </ul>
                </div>
            </div>
        </header>
        "##,
        email = escape_html_text(user.email),
        rail_toggle = if company.is_some() {
            rail_toggle_button()
        } else {
            String::new()
        },
        back = icon(Icon::ArrowLeft, BUTTON_ICON),
        sign_out = icon(Icon::SignOut, BUTTON_ICON),
        chip = account_chip(user, FragmentSwap::Inline),
        agent_library = agent_library_entry(user),
    )
}

/// The button that brings the rail out on a phone, where it is a drawer rather than a column.
///
/// An account with no company yet gets no rail — [`ui_shell`] leaves it out — so it gets no
/// button for one either, rather than one that opens nothing.
fn rail_toggle_button() -> String {
    format!(
        r##"<button type="button" class="ui-mobile-only btn btn-ghost btn-square btn-sm"
                    title="Show navigation" aria-label="Show navigation"
                    aria-controls="app-rail" aria-expanded="false" data-action="toggle-rail">{menu}</button>"##,
        menu = icon(Icon::Menu, BUTTON_ICON),
    )
}

/// The operator-only way into the global agent library.
///
/// Hidden from everyone else rather than shown disabled: `/ui/agent-library` answers a
/// non-operator with "not found", so an entry they could see would promise a page that, for them,
/// is not there. It lives in the account menu rather than the rail because the library is global
/// -- it is the one workspace that is not scoped to the company the rail points at.
fn agent_library_entry(user: &MailboxUser<'_>) -> &'static str {
    if user.is_operator {
        r##"                        <li><a href="/ui/agent-library">Agent library</a></li>"##
    } else {
        ""
    }
}

/// Who the top bar says you are: your name, your address and your face, as one fragment.
///
/// The three are swapped together rather than separately because they are one claim -- a rename
/// that reached the avatar but not the name beside it would leave the bar contradicting itself.
///
/// Rendered out of band after the account is saved, because the bar is the one piece of chrome an
/// htmx swap never replaces: without it the reader keeps seeing their old name until the next full
/// page load.
pub fn account_chip(user: &MailboxUser<'_>, swap: FragmentSwap) -> String {
    format!(
        r##"<div id="account-chip" class="flex items-center gap-3"{oob}>
                            <div class="hidden text-right leading-tight sm:block">
                                <div class="text-sm font-semibold">{username}</div>
                                <div class="text-[11px] font-normal opacity-60">{email}</div>
                            </div>
                            {avatar}
                        </div>"##,
        oob = swap.oob_attribute(),
        username = escape_html_text(user.username),
        email = escape_html_text(user.email),
        avatar = avatar_bubble(user.avatar_url, user.username, AvatarSize::Bar),
    )
}

/// The slim left rail: the `/ui` workspaces first, then links out to the classic pages.
///
/// The workspace the response belongs to is lit, so the rail says where you are as well as where
/// you can go.
fn icon_rail(user: &MailboxUser<'_>, company: &Company, section: UiSection) -> String {
    let company_id = company.id;
    let destinations = [
        (UiSection::Mailbox, "/ui", Icon::Mail, "Mailbox"),
        (UiSection::Channels, "/ui/channels", Icon::Hash, "Channels"),
        (UiSection::Agents, "/ui/agents", Icon::Hubot, "Agents"),
        (
            UiSection::Schedules,
            "/ui/schedules",
            Icon::Stopwatch,
            "Schedules",
        ),
        (UiSection::Tasks, "/ui/tasks", Icon::Gear, "Tasks"),
        (
            UiSection::Deliveries,
            "/ui/deliveries",
            Icon::PaperAirplane,
            "Deliveries",
        ),
        (
            UiSection::Dashboard,
            "/ui/dashboard",
            Icon::Graph,
            "Dashboard",
        ),
        (
            UiSection::Companies,
            "/ui/companies",
            Icon::Organization,
            "Companies",
        ),
    ];

    let links: String = destinations
        .iter()
        .filter(|(destination, _, _, _)| rail_section_visible(user, *destination))
        .map(|(destination, path, glyph, title)| {
            format!(
                r##"<a href="{path}?company_id={company_id}" class="btn btn-square btn-md {style}" title="{title}" aria-label="{title}" data-action="navigate-workspace"{current}>{glyph}<span class="rail-label">{title}</span></a>"##,
                style = if section == *destination {
                    "btn-primary"
                } else {
                    "btn-ghost"
                },
                current = if section == *destination { r##" aria-current="page""## } else { "" },
                glyph = icon(*glyph, RAIL_ICON),
            )
        })
        .collect();

    format!(
        r##"
        <nav id="app-rail" class="app-rail flex w-16 shrink-0 flex-col items-center gap-1.5 overflow-y-auto border-r border-base-300 px-2 py-3" aria-label="Primary navigation">
            {links}
            {company_badge}
        </nav>
        "##,
        company_badge = if user.company_membership.is_team() {
            rail_company_badge(company, FragmentSwap::Inline)
        } else {
            Default::default()
        },
    )
}

/// The rail mirrors route authorization instead of advertising workspaces the caller cannot open.
fn rail_section_visible(user: &MailboxUser<'_>, section: UiSection) -> bool {
    match section {
        UiSection::Mailbox => user.company_membership.is_team(),
        UiSection::Channels
        | UiSection::Agents
        | UiSection::Schedules
        | UiSection::Tasks
        | UiSection::Deliveries => user.company_membership.manages_company_operations(),
        UiSection::Dashboard => {
            user.company_membership.manages_company_operations() || user.is_operator
        }
        // Company settings are readable by the team, while their edit controls remain owner-only.
        UiSection::Companies => user.company_membership.is_team(),
        UiSection::Invites | UiSection::Profile => false,
    }
}

/// The foot of the rail: the company everything above it is scoped to, as its picture or its
/// letter.
///
/// It stands where the sign-out button used to, because the rail's job is to say where you are:
/// which company you are in is the one thing the icons above it do not show, and logging out
/// already has an entry in the account menu -- where a deliberate click, rather than a stray one
/// at the edge of the window, is what reaches it.
///
/// Rendered out of band after a company's settings are saved, so a new picture reaches the chrome
/// that no pane swap would otherwise touch -- the rail's counterpart of [`account_avatar_oob`].
pub fn rail_company_badge(company: &Company, swap: FragmentSwap) -> String {
    format!(
        r##"<a id="rail-company" href="/ui/companies?company_id={company_id}"
                class="btn btn-square btn-md btn-ghost mt-auto" title="{name}" aria-label="Company: {name}" data-action="navigate-workspace"{oob}>{avatar}<span class="rail-label">{name}</span></a>"##,
        company_id = company.id,
        name = escape_html_text(&company.name),
        oob = swap.oob_attribute(),
        avatar = avatar_bubble(company.avatar_url.as_ref(), &company.name, AvatarSize::Bar),
    )
}

/// The company at the centre of the top bar: the one every workspace above the rail is scoped to,
/// said in words rather than as the rail badge's picture alone.
///
/// It is deliberately not a link. The rail badge already leads to the company's settings, and a
/// second way in would make the bar's job -- naming where you are -- read as navigation.
///
/// Rendered out of band after a company is saved, for the same reason as
/// [`rail_company_badge`]: a rename reaches no pane swap, so without this the bar would keep
/// announcing the old name until the next full page load.
pub fn topbar_company(company: &Company, swap: FragmentSwap) -> String {
    format!(
        r##"<div id="topbar-company" class="flex min-w-0 items-center gap-2"{oob}>
                    {avatar}
                    <span class="truncate text-sm font-semibold">{name}</span>
                </div>"##,
        oob = swap.oob_attribute(),
        name = escape_html_text(&company.name),
        avatar = avatar_bubble(company.avatar_url.as_ref(), &company.name, AvatarSize::Row),
    )
}

/// The rail's buttons are square and large, so its glyphs are drawn a size up from body icons.
const RAIL_ICON: &str = "h-6 w-6";

/// The header at the top of every `/ui` sidebar (first column): the workspace title and its one-line description.
pub(crate) fn sidebar_header(title: &str, subtitle: &str) -> String {
    format!(
        r##"
            <div class="workspace-heading border-b border-base-300 px-4 py-5">
                <h2 class="text-base font-semibold leading-tight">{title}</h2>
                <p class="mt-1 text-xs leading-relaxed opacity-60">{subtitle}</p>
            </div>
        "##,
        title = escape_html_text(title),
        subtitle = escape_html_text(subtitle),
    )
}

/// The channel column: one menu entry per channel, channel actions at the bottom.
fn channel_sidebar(page: &MailboxPage<'_>) -> String {
    let header = sidebar_header("Mailbox", "Inbound channels and conversations.");
    let items: String = page
        .channels
        .iter()
        .map(|channel| {
            channel_menu_item(
                page.company.id,
                channel,
                &channel.inbound_address(&page.company.slug, page.app_domain_name),
                page.selected_channel.is_some_and(|c| c.id == channel.id),
            )
        })
        .collect();

    let menu_body = if page.channels.is_empty() {
        r##"<li class="px-2 py-6 text-center text-xs opacity-60">No channels yet. Create one under Channels.</li>"##
            .to_string()
    } else {
        items
    };

    format!(
        r##"
        <aside id="mailbox-sidebar" class="flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            <div class="relative">
                {header}
                <button type="button" class="mailbox-sidebar-close btn btn-ghost btn-sm btn-square absolute right-3 top-3"
                    title="Collapse mailbox column" aria-label="Collapse mailbox column"
                    aria-controls="mailbox-sidebar" aria-expanded="false" data-action="toggle-mailbox-sidebar">
                    {collapse}
                </button>
            </div>
            <ul id="channel-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">
                {menu_body}
            </ul>
            {footer}
        </aside>
        "##,
        header = header,
        collapse = icon(Icon::ArrowLeft, BUTTON_ICON),
        footer = channel_actions(page.company.id, page.selected_channel, FragmentSwap::Inline),
    )
}

/// The mailbox controls that replace the channel sidebar at compact widths.
///
/// The selector stays outside `#thread-column`, so an htmx channel swap does not rebuild or move
/// it. [`selectSidebarItem`] keeps its label and active item in sync with the full sidebar.
///
/// The menu is a native popover rather than a `<details>` dropdown: the browser then owns the
/// interactions a menu is expected to have -- a click outside or Escape dismisses it, and it
/// renders in the top layer, so it is never clipped by the scrolling columns beside it. Only
/// picking a channel is left to us, because light dismissal does not fire for a click *inside*
/// the menu.
fn compact_mailbox_header(page: &MailboxPage<'_>) -> String {
    let selected_name = page
        .selected_channel
        .map(|channel| channel.name.as_str())
        .unwrap_or("Select a channel");
    let options = if page.channels.is_empty() {
        r##"<li class="px-3 py-4 text-center text-xs opacity-60">No channels yet.</li>"##
            .to_string()
    } else {
        page.channels
            .iter()
            .map(|channel| {
                let active = page
                    .selected_channel
                    .is_some_and(|selected| selected.id == channel.id);
                format!(
                    r##"<li><a class="{active}" data-mailbox-channel="{channel_id}"
                            hx-get="/ui/threads?company_id={company_id}&channel_id={channel_id}"
                            hx-target="#thread-column" hx-swap="outerHTML"
                            hx-sync="#thread-column:replace"
                            hx-push-url="/ui?company_id={company_id}&channel_id={channel_id}"
                            data-action="select-sidebar-item">
                            <span class="min-w-0 truncate" data-mailbox-channel-name>{name}</span>
                            <span class="badge badge-ghost badge-sm font-mono">{slug}</span>
                        </a></li>"##,
                    active = if active { "menu-active" } else { "" },
                    channel_id = channel.id,
                    company_id = page.company.id,
                    name = escape_html_text(&channel.name),
                    slug = escape_html_text(&channel.slug),
                )
            })
            .collect()
    };

    format!(
        r##"
            <div class="mailbox-compact-header shrink-0 items-center gap-2 border-b border-r border-base-300 bg-base-200 px-3 py-2">
                <button type="button" class="btn btn-ghost btn-sm btn-square"
                    title="Expand mailbox column" aria-label="Expand mailbox column"
                    aria-controls="mailbox-sidebar" aria-expanded="false" data-action="toggle-mailbox-sidebar">
                    {expand}
                </button>
                <button type="button" class="btn btn-ghost btn-sm min-w-0 flex-1 justify-between px-2"
                    popovertarget="mailbox-selector-menu" style="anchor-name:--mailbox-selector">
                    <span class="min-w-0 text-left leading-tight">
                        <span class="block text-[10px] font-semibold uppercase tracking-wider opacity-50">Mailbox</span>
                        <span id="mailbox-selector-label" class="block truncate">{selected_name}</span>
                    </span>
                    {chevron}
                </button>
                <ul id="mailbox-selector-menu" popover style="position-anchor:--mailbox-selector"
                    class="dropdown menu w-72 flex-nowrap rounded-box border border-base-300 bg-base-100 p-2 shadow-2xl">
                    {options}
                </ul>
            </div>
        "##,
        expand = icon(Icon::ChevronRight, BUTTON_ICON),
        chevron = icon(Icon::ChevronDown, BUTTON_ICON),
        selected_name = escape_html_text(selected_name),
    )
}

/// Channel management is the `/ui` Channels workspace, so these link straight into the state they
/// mean: the create form open, or the selected channel's settings already loaded.
///
/// Picking a channel only swaps the thread column, so this block is re-rendered out-of-band with
/// it — otherwise "Edit Channel" would keep pointing at whatever the last full page load selected,
/// or stay missing entirely when the mailbox was entered without a channel in the URL.
pub fn channel_actions(
    company_id: Uuid,
    selected_channel: Option<&Channel>,
    swap: FragmentSwap,
) -> String {
    let edit_button = match selected_channel {
        Some(channel) => format!(
            r##"<a href="/ui/channels?company_id={company_id}&channel_id={channel_id}"
                    class="btn btn-ghost btn-sm btn-block justify-start">{pencil} Edit Channel</a>"##,
            channel_id = channel.id,
            pencil = icon(Icon::Pencil, BUTTON_ICON),
        ),
        None => String::new(),
    };

    format!(
        r##"
            <div id="channel-actions" class="space-y-1 border-t border-base-300 p-2"{oob}>
                <a href="/ui/channels?company_id={company_id}&new=1" class="btn btn-ghost btn-sm btn-block justify-start">{plus} New Channel</a>
                {edit_button}
            </div>
        "##,
        oob = swap.oob_attribute(),
        plus = icon(Icon::Plus, BUTTON_ICON),
    )
}

/// One `<option>` per channel for a `/ui` sidebar filter, with the current one marked.
///
/// Shared by the Tasks and Deliveries workspaces: both filter the same company's channels the same
/// way, and a channel must not read differently depending on which queue you are looking at. The
/// "All channels" option is left to the caller, since only it knows what "all" is called there.
pub(crate) fn channel_filter_options(channels: &[Channel], selected: Option<Uuid>) -> String {
    channels
        .iter()
        .map(|channel| {
            format!(
                r##"<option value="{id}"{selected}>{name}</option>"##,
                id = channel.id,
                selected = selected_when(selected == Some(channel.id)),
                name = escape_html_text(&channel.name),
            )
        })
        .collect()
}

pub(crate) fn selected_when(selected: bool) -> &'static str {
    if selected { " selected" } else { "" }
}

/// Compose is only meaningful inside a channel, so it lives in that channel's thread column header
/// and starts its new thread in the channel the column is showing.
fn compose_button(company_id: Uuid, channel: &Channel) -> String {
    format!(
        r##"<button id="compose-button" type="button" class="btn btn-primary btn-sm"
                    title="Start a new thread in this channel"
                    hx-get="/ui/compose?company_id={company_id}&channel_id={channel_id}"
                    hx-target="#detail-pane" hx-swap="outerHTML" hx-sync="#detail-pane:replace">{pencil} New Thread</button>"##,
        channel_id = channel.id,
        pencil = icon(Icon::Pencil, BUTTON_ICON),
    )
}

fn channel_menu_item(
    company_id: Uuid,
    channel: &Channel,
    address: &EmailAddress,
    selected: bool,
) -> String {
    format!(
        r##"
                <li>
                    <a class="flex flex-col items-start gap-0.5 {active}" data-mailbox-channel="{channel_id}"
                        hx-get="/ui/threads?company_id={company_id}&channel_id={channel_id}"
                        hx-target="#thread-column" hx-swap="outerHTML"
                        hx-sync="#thread-column:replace"
                        hx-push-url="/ui?company_id={company_id}&channel_id={channel_id}"
                        data-action="select-sidebar-item">
                        <span class="flex w-full items-center gap-2">
                            <span class="truncate" data-mailbox-channel-name>{name}</span>
                            <span class="badge badge-ghost badge-sm font-mono">{slug}</span>{disabled_badge}
                        </span>
                        <span class="w-full truncate font-mono text-[11px] opacity-60">{address}</span>
                    </a>
                </li>
        "##,
        active = if selected { "menu-active" } else { "" },
        channel_id = channel.id,
        name = escape_html_text(&channel.name),
        slug = escape_html_text(&channel.slug),
        disabled_badge = disabled_badge(channel),
        address = escape_html_text(address),
    )
}

/// The thread column before any channel is picked.
pub fn empty_thread_column() -> String {
    format!(
        r##"
        <section id="thread-column"{THREAD_COLUMN_SKELETON} class="flex min-h-0 w-full min-w-0 flex-1 flex-col border-r border-base-300 bg-base-100">
            <div class="flex items-center justify-between border-b border-base-300 px-4 py-3">
                <h2 class="text-lg font-bold">Threads</h2>
            </div>
            <div class="flex flex-1 items-center justify-center p-6 text-center text-sm opacity-60">
                Select a channel to see its threads.
            </div>
        </section>
    "##
    )
}

/// The channel's threads, newest first, in the column beside the sidebar.
///
/// `min-h-0 flex-1` on the section is what makes `#thread-list` scroll. The section is a flex item
/// in the column-direction `.ui-pane-list`, where `min-height: auto` keeps an item at its content
/// height: without those two utilities the section grows past the pane, the list's `flex-1` is
/// measured against an unbounded height, and its `overflow-y-auto` never has anything to scroll --
/// the rows below the fold are simply clipped by the shell's `overflow-hidden` body.
pub fn thread_column(column: &ThreadColumn<'_>) -> String {
    // Where the live column resumes from: the *newest* thread on the page. The column is sorted
    // newest-first, so that is the first row, not the last one.
    let after = column
        .threads
        .first()
        .map(|thread| format!("&after={}", thread.cursor()))
        .unwrap_or_default();

    format!(
        r##"
        <section id="thread-column"{THREAD_COLUMN_SKELETON} class="flex min-h-0 w-full min-w-0 flex-1 flex-col border-r border-base-300 bg-base-100"
            hx-ext="sse"
            sse-connect="/ui/threads/events?company_id={company_id}&channel_id={channel_id}{after}">
            <div class="flex items-center justify-between gap-2 border-b border-base-300 px-4 py-3">
                <div class="min-w-0">
                    <h2 class="truncate text-lg font-bold">{channel_name}</h2>
                    <p class="truncate text-xs opacity-60">Newest threads first</p>
                </div>
                <div class="flex shrink-0 flex-wrap items-center gap-2">
                    {compose_button}
                    <button type="button" class="btn btn-ghost btn-sm btn-square text-xl leading-none" title="Reload threads"
                        hx-get="/ui/threads?company_id={company_id}&channel_id={channel_id}"
                        hx-target="#thread-column" hx-swap="outerHTML" hx-sync="#thread-column:replace">{reload_glyph}</button>
                </div>
            </div>
            {list_open}
                {list_html}
            </div>
        </section>
        "##,
        reload_glyph = icon(Icon::Sync, BUTTON_ICON),
        channel_name = escape_html_text(&column.channel.name),
        company_id = column.company_id,
        channel_id = column.channel.id,
        after = after,
        list_open = thread_list_open_tag(FragmentSwap::Inline),
        compose_button = compose_button(column.company_id, column.channel),
        list_html = thread_list_fragment(column, FragmentSwap::Inline),
    )
}

/// Thread cards plus the pagination footer, shared by the first page and every "load older" page.
pub fn thread_list_fragment(column: &ThreadColumn<'_>, swap: FragmentSwap) -> String {
    let cards = if column.threads.is_empty() && swap == FragmentSwap::Inline {
        r##"
            <p class="no-threads p-6 text-center text-sm opacity-60">No threads in this channel yet. Use New Thread to start one.</p>
        "##
        .to_string()
    } else {
        column
            .threads
            .iter()
            .map(|thread| thread_row(column, thread))
            .collect()
    };

    format!(
        "{cards}{pagination}",
        pagination = thread_pagination(column, swap)
    )
}

fn thread_row(column: &ThreadColumn<'_>, thread: &Thread) -> String {
    thread_row_fragment(
        column.company_id,
        column.channel,
        thread,
        column.selected_thread_id == Some(thread.id),
        ThreadRowMarks {
            activity: column.activity.get(&thread.id).copied(),
            from_other_channel: opened_by_another_channel(thread, column.app_domain_name),
            ..ThreadRowMarks::default()
        },
    )
}

/// Whether another channel's agent opened this thread.
///
/// A platform address becomes a thread principal only as the sender of an internal message:
/// [`is_third_party_address`] keeps platform addresses out of the third parties a thread pulls in,
/// a channel is never a participant of its own thread (self-outreach is rejected), and outbound
/// recipients are never added at all. So the address form is the whole test, and it needs no
/// company lookup -- cross-company internal delivery is rejected at ingest.
///
/// [`is_third_party_address`]: crate::use_cases::thread::ThreadUseCases
pub fn opened_by_another_channel(thread: &Thread, app_domain_name: &str) -> bool {
    thread
        .participant_projection
        .subjects_for(TransportKind::Email)
        .into_iter()
        .any(|email| parse_platform_address(email, app_domain_name).is_some())
}

/// The mark on anything that came from another channel in this company.
///
/// One helper, so the thread row, the message bubble and the admin thread card cannot drift into
/// three glyphs for one fact. [`icon`] renders `aria-hidden`, so the wrapper carries the title --
/// the same arrangement [`thread_activity_mark`] uses.
/// Which interface a message's author wrote over.
///
/// Shown beside the name because the name no longer implies it: a principal reached over mail and
/// the same principal reached over Slack render as one person, and the badge is what says which
/// conversation this particular turn came from. A message no transport carried -- a schedule
/// prompt, a system note -- gets no badge, which is the honest rendering of "the platform said
/// this".
fn transport_badge(transport: Option<TransportKind>, handle: Option<&str>) -> String {
    let Some(transport) = transport else {
        return String::new();
    };
    format!(
        r##"<span class="badge badge-ghost badge-xs font-mono uppercase" title="{title}">{label}</span>"##,
        title = escape_html_text(handle.unwrap_or(transport.as_str())),
        label = escape_html_text(transport.as_str()),
    )
}

pub fn other_channel_glyph(from_other_channel: bool, title: &str) -> String {
    if !from_other_channel {
        return String::new();
    }
    format!(
        r##"<span class="shrink-0 leading-none opacity-60" title="{title}">{glyph}</span>"##,
        title = escape_html_text(title),
        glyph = icon(Icon::Hubot, BUTTON_ICON),
    )
}

/// The marks one thread row carries beyond the thread itself.
///
/// A struct rather than three more positional arguments: `Option<ThreadActivity>`,
/// `Option<MessageRole>` and a `bool` in a row are exactly the arguments a call site can swap
/// without the compiler noticing.
#[derive(Clone, Copy, Default)]
pub struct ThreadRowMarks {
    /// What this thread is doing. Threads with nothing in flight leave it `None`.
    pub activity: Option<ThreadActivity>,
    /// Who spoke last. Only rows arriving over the stream carry this -- see [`thread_row_fragment`].
    pub last_role: Option<MessageRole>,
    /// This thread was opened by an agent in another channel.
    pub from_other_channel: bool,
}

/// One thread card on its own, for the live column stream.
///
/// The same function renders the cards inside the column, so a row that streams in is
/// indistinguishable from one that came with the page.
pub fn thread_row_fragment(
    company_id: Uuid,
    channel: &Channel,
    thread: &Thread,
    selected: bool,
    marks: ThreadRowMarks,
) -> String {
    let participant_addresses = thread
        .participant_projection
        .subjects_for(TransportKind::Email);
    let participants = if participant_addresses.is_empty() {
        "No participants".to_string()
    } else {
        escape_html_text(&participant_addresses.join(", "))
    };
    let channel_id = channel.id;

    format!(
        r##"
                <button type="button" data-thread-id="{thread_id}"{last_role}
                    class="thread-row block w-full border-b border-base-300 px-4 py-3 text-left transition hover:bg-base-200 {active}"
                    hx-get="/ui/messages?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                    hx-target="#detail-pane" hx-swap="outerHTML"
                    hx-sync="#detail-pane:replace"
                    hx-push-url="/ui?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                    data-action="select-thread-row">
                    <div class="flex items-baseline justify-between gap-2">
                        <span class="flex min-w-0 items-center gap-1.5">{channel_glyph}<span class="truncate font-semibold">{subject}</span></span>
                        <span class="flex shrink-0 items-center gap-1.5">
                            <span class="thread-mark text-sm leading-none text-success"></span>
                            {activity_slot}
                            <span class="text-xs opacity-60">{updated_at}</span>
                        </span>
                    </div>
                    <div class="truncate text-xs opacity-70">{participants}</div>
                    <div class="truncate font-mono text-[11px] opacity-40">{thread_id}</div>
                </button>
        "##,
        active = if selected { "bg-base-300" } else { "" },
        company_id = company_id,
        channel_id = channel_id,
        thread_id = thread.id,
        subject = escape_html_text(&thread.subject),
        channel_glyph = other_channel_glyph(
            marks.from_other_channel,
            "Opened by an agent in another channel",
        ),
        activity_slot = thread_activity_slot(thread.id, marks.activity),
        // Only rows arriving over the stream carry this: the reply mark means "while you were
        // watching", so a freshly rendered page has nothing to mark and needs no lookup.
        last_role = match marks.last_role {
            Some(role) => format!(r##" data-last-role="{}""##, role.as_str()),
            None => String::new(),
        },
        updated_at = super::format_date_time(thread.updated_at),
        participants = participants,
    )
}

fn thread_pagination(column: &ThreadColumn<'_>, swap: FragmentSwap) -> String {
    let oob = swap.oob_attribute();
    match column.next_cursor {
        Some(cursor) => format!(
            r##"
                <div id="thread-pagination" class="p-3"{oob}>
                    <button class="btn btn-ghost btn-sm btn-block" hx-disabled-elt="this"
                        hx-get="/ui/threads/list?company_id={company_id}&channel_id={channel_id}&cursor={cursor}"
                        hx-target="#thread-list" hx-swap="beforeend" hx-sync="#thread-list:replace">
                        Load older threads
                    </button>
                </div>
            "##,
            company_id = column.company_id,
            channel_id = column.channel.id,
        ),
        None => format!(r##"<div id="thread-pagination"{oob}></div>"##),
    }
}

/// The right-hand pane before a thread is opened.
pub fn empty_detail_pane(message: &str, swap: FragmentSwap) -> String {
    format!(
        r##"
        <section id="detail-pane"{PANE_SKELETON} data-pane-empty class="ui-pane-detail flex min-w-0 flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn message_pane(pane: &MessagePane<'_>) -> String {
    let participant_addresses = pane
        .thread
        .participant_projection
        .subjects_for(TransportKind::Email);
    let participants = if participant_addresses.is_empty() {
        "No participants".to_string()
    } else {
        escape_html_text(&participant_addresses.join(", "))
    };

    let messages_html = if pane.messages.is_empty() {
        // `id` so the first streamed message can clear it -- see `removeEmptyMessagePlaceholder`.
        r##"<p id="no-messages" class="p-6 text-center text-sm opacity-60">This thread has no messages yet.</p>"##
            .to_string()
    } else {
        pane.messages
            .iter()
            .map(|message| {
                message_bubble_chat(
                    message,
                    pane.agent,
                    Some(pane.viewer_email),
                    MessageScope {
                        company_id: pane.company_id,
                        channel_id: pane.channel.id,
                    },
                )
            })
            .collect()
    };

    // Where the live stream should resume from: the newest message already on the page, so a
    // message that lands between this render and the browser connecting is not missed. An empty
    // thread has nothing to resume from, and streams from its start.
    let after = pane
        .messages
        .last()
        .map(|message| format!("&after={}", message.cursor()))
        .unwrap_or_default();

    format!(
        r##"
        <section id="detail-pane"{PANE_SKELETON} data-thread-id="{thread_id}" class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100" hx-ext="sse"
            sse-connect="/ui/events?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}{after}">
            <div class="flex flex-wrap items-start justify-between gap-3 border-b border-base-300 px-4 py-3 sm:px-6 sm:py-4">
                <div class="min-w-0 grow basis-48">
                    <h2 class="truncate text-lg font-bold sm:text-xl">{subject}</h2>
                    <p class="truncate text-xs opacity-70">{participants}</p>
                    <p class="truncate font-mono text-[11px] opacity-40">{thread_id}</p>
                </div>
                <div class="flex shrink-0 flex-wrap items-center gap-2">
                    <button type="button" class="btn btn-primary btn-sm" title="Send another message in this thread"
                        hx-get="/ui/reply?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                        hx-target="#detail-pane" hx-swap="outerHTML" hx-sync="#detail-pane:replace">{pencil} New Message</button>
                    <button type="button" class="btn btn-ghost btn-sm btn-square" title="Reload messages"
                        hx-get="/ui/messages?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                        hx-target="#detail-pane" hx-swap="outerHTML" hx-sync="#detail-pane:replace">{reload}</button>
                    <a href="/companies/{company_id}/channels/{channel_id}/simulate?thread_id={thread_id}"
                        class="btn btn-outline btn-sm">Open in Simulator</a>
                </div>
            </div>
            <div id="message-scroll" class="flex-1 space-y-1 overflow-y-auto px-4 py-4 sm:px-6"
                sse-swap="message" hx-swap="beforeend">
                {messages_html}
            </div>
            <div id="thread-activity" sse-swap="activity" hx-target="this" hx-swap="innerHTML">{activity_strip}</div>
            {composer}
            {diagnostics_dialog}
        </section>
        "##,
        pencil = icon(Icon::Pencil, BUTTON_ICON),
        reload = icon(Icon::Sync, BUTTON_ICON),
        subject = escape_html_text(&pane.thread.subject),
        participants = participants,
        thread_id = pane.thread.id,
        company_id = pane.company_id,
        channel_id = pane.channel.id,
        after = after,
        messages_html = messages_html,
        activity_strip = thread_activity_strip(pane.activity),
        composer = thread_composer(pane),
        diagnostics_dialog = DIAGNOSTICS_DIALOG,
    )
}

/// Where the transport-identifier pane lands when a bubble's `ids` link is followed.
///
/// Empty until asked for. Nothing about a provider key is in the thread's own HTML, which is the
/// point: the pane is a separate, separately authorized request.
const DIAGNOSTICS_DIALOG: &str = r##"
            <dialog id="message-diagnostics" class="modal">
                <div class="modal-box">
                    <h3 class="mb-3 text-sm font-bold">Transport identifiers</h3>
                    <div id="message-diagnostics-body"></div>
                    <div class="modal-action">
                        <form method="dialog"><button class="btn btn-sm">Close</button></form>
                    </div>
                </div>
                <form method="dialog" class="modal-backdrop"><button>close</button></form>
            </dialog>
"##;

/// The chat-style send box under the messages: one line that grows as it is typed, sending into
/// the open thread without leaving the pane.
///
/// It posts to the same `/ui/reply` endpoint as the header's "New Message" form, so a rejected
/// message comes back as that fuller form with the text kept and the reason on top.
fn thread_composer(pane: &MessagePane<'_>) -> String {
    format!(
        r##"
            <form class="border-t border-base-300 px-4 py-3 sm:px-6" hx-post="/ui/reply"
                hx-target="#detail-pane" hx-swap="outerHTML">
                <input type="hidden" name="company_id" value="{company_id}">
                <input type="hidden" name="channel_id" value="{channel_id}">
                <input type="hidden" name="thread_id" value="{thread_id}">
                <div class="flex items-end gap-2">
                    <div class="aura aura-holo flex-1 [--aura-radius:var(--radius-field)]">
                        <textarea id="thread-composer" name="text_body" rows="1" required
                            placeholder="Write a message... (Enter to send, Shift+Enter for a new line)"
                            class="textarea block max-h-40 min-h-12 w-full resize-none text-sm"
                            data-keydown="composer" data-input="auto-grow-composer"></textarea>
                    </div>
                    <button type="submit" class="btn btn-primary" title="Send">
                        <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                        <span class="[.htmx-request_&]:hidden">Send</span>
                    </button>
                </div>
                <label class="label mt-1 cursor-pointer justify-start gap-2 p-0">
                    <input type="checkbox" name="deliver" value="true" class="toggle toggle-primary toggle-xs">
                    <span class="text-xs opacity-60">Deliver the agent reply by email (off keeps it in-app)</span>
                </label>
                <label class="label mt-1 cursor-pointer justify-start gap-2 p-0">
                    <input type="checkbox" name="quiet" value="true" class="checkbox checkbox-primary checkbox-xs">
                    <span class="text-xs opacity-60">Send quietly (save to history without running the agent)</span>
                </label>
            </form>
        "##,
        company_id = pane.company_id,
        channel_id = pane.channel.id,
        thread_id = pane.thread.id,
    )
}

/// One message as a daisyUI chat bubble: agent replies on the right, everyone else on the left.
///
/// Public because the live message stream (`/ui/events`) renders bubbles one at a time as they
/// arrive. Both paths share this one definition, so a bubble that streams in is indistinguishable
/// from one that came with the page.
/// One message as a daisyUI chat row: the writer's face, then their name and the time, the bubble,
/// and the subject underneath.
///
/// `agent` is the channel's agent when it runs exactly one, and is what the agent side is drawn as.
/// A reply is stored with the *channel's* address as its sender rather than the agent that wrote
/// it, so with a stack of agents there is no one face to show and the address stands in.
pub fn message_bubble_chat(
    message: &ThreadMessageView,
    agent: Option<&Agent>,
    viewer_email: Option<&EmailAddress>,
    scope: MessageScope,
) -> String {
    let is_agent = message.is_agent();
    // Written by an agent in *another* channel: an inbound message only carries the agent role
    // when it came in over the internal delivery path, which validates the sender against its
    // source channel. Nothing arriving from the wire can reach this combination.
    let from_other_channel =
        message.direction == MessageDirection::Inbound && message.role == MessageRole::Agent;
    // "Mine" is decided on the handle the message was written with, so a message a member sent
    // over mail and one they composed in the app both read as theirs.
    let is_viewer = match (viewer_email, message.author.email_address()) {
        (Some(viewer), Some(author)) => author.eq_ignore_ascii_case(viewer.as_ref()),
        _ => false,
    };
    let body = if is_agent {
        format!(
            r##"<div class="{MARKDOWN_CONTENT_STYLES}">{}</div>"##,
            render_markdown(&message.body)
        )
    } else {
        format!(
            r##"<div class="whitespace-pre-wrap">{}</div>"##,
            escape_html_text(&message.body)
        )
    };

    // The passed agent is *this* channel's. Attributing a sibling channel's message to it would
    // put the wrong name and face on work a different agent did, so that one keeps its address.
    let (writer, avatar_url) = match (is_agent && !from_other_channel, agent) {
        (true, Some(agent)) => (agent.name.as_str(), agent.avatar_url.as_ref()),
        _ => (message.author.display(), None),
    };

    format!(
        r##"
                <div class="chat {side}" data-role="{role}">
                    <div class="chat-image">{avatar}</div>
                    <div class="chat-header gap-1 opacity-70">
                        {channel_glyph}{writer}{transport_badge}
                        <time class="text-xs opacity-60">{created_at}</time>
                    </div>
                    <div class="chat-bubble {bubble_class} max-w-2xl text-sm">{body}{attachments}</div>
                    <div class="chat-footer font-mono text-[11px] opacity-40">{subject}{diagnostics}{tasks}</div>
                </div>
        "##,
        side = if is_viewer { "chat-end" } else { "chat-start" },
        // Read by the column when a bubble streams in: only the agent's own reply answers the
        // question its row's activity mark was asking.
        role = if is_agent { "agent" } else { "human" },
        bubble_class = if is_viewer { "chat-bubble-primary" } else { "" },
        channel_glyph = other_channel_glyph(from_other_channel, "From an agent in another channel"),
        transport_badge =
            transport_badge(message.author.transport, message.author.handle.as_deref()),
        avatar = avatar_bubble(avatar_url, writer, AvatarSize::Row),
        writer = escape_html_text(writer),
        created_at = super::format_date_time(message.created_at),
        subject = escape_html_text(&message.subject),
        diagnostics = diagnostics_link(message, scope),
        tasks = tasks_link(scope),
        body = body,
        attachments = attachment_chips(message, scope),
    )
}

/// A direct link into the company's Tasks workspace in list view.
fn tasks_link(scope: MessageScope) -> String {
    format!(
        r##" · <a class="link link-hover"
                    title="Tasks list"
                    data-action="navigate-workspace"
                    href="/ui/tasks?company_id={company_id}&amp;view=list">tasks</a>"##,
        company_id = scope.company_id,
    )
}

/// The way into the one pane that shows provider identifiers.
///
/// A link rather than rendered detail: the pane is separately authorized and separately loaded, so
/// a `Message-ID` is never in the HTML of an ordinary thread read. The message is named by its
/// canonical association id, which is what the route scopes.
fn diagnostics_link(message: &ThreadMessageView, scope: MessageScope) -> String {
    format!(
        r##" · <button type="button" class="link link-hover"
                    title="Transport identifiers for this message"
                    data-action="open-dialog" data-dialog="message-diagnostics"
                    hx-get="/ui/threads/{thread_id}/messages/{message_id}/diagnostics?company_id={company_id}&channel_id={channel_id}"
                    hx-target="#message-diagnostics-body" hx-swap="innerHTML">ids</button>"##,
        thread_id = message.thread_id,
        message_id = message.id,
        company_id = scope.company_id,
        channel_id = scope.channel_id,
    )
}

/// Where a message is being shown from.
///
/// An attachment link carries the company and channel because that is what the download route
/// authorizes against -- the same scope the thread itself was opened with, so a file is never
/// reachable from further away than the thread it hangs on.
#[derive(Debug, Clone, Copy)]
pub struct MessageScope {
    pub company_id: Uuid,
    pub channel_id: Uuid,
}

/// What was attached to a message, under its body.
///
/// An attachment that was never stored -- mail that arrived before there was a bucket, or an
/// upload that failed -- is still listed, as a chip that does not link anywhere. Saying nothing
/// would misreport the mail.
fn attachment_chips(message: &ThreadMessageView, scope: MessageScope) -> String {
    if message.attachments.is_empty() {
        return String::new();
    }

    let chips: String = message
        .attachments
        .iter()
        .map(|attachment| attachment_chip(message.thread_id, attachment, scope))
        .collect();

    format!(
        r##"<div class="mt-2 flex flex-wrap gap-2 border-t border-current/20 pt-2">{chips}</div>"##
    )
}

fn attachment_chip(
    thread_id: Uuid,
    attachment: &AttachmentMetadata,
    scope: MessageScope,
) -> String {
    let label = format!(
        r##"<span class="max-w-[16rem] truncate">{filename}</span>
                            <span class="opacity-60">{size}</span>"##,
        filename = escape_html_text(&attachment.filename),
        size = escape_html_text(&file_size(attachment.size_bytes)),
    );

    match attachment.storage_key.is_some() {
        true => format!(
            r##"<a class="btn btn-xs btn-ghost gap-1 normal-case"
                            href="/ui/threads/{thread_id}/attachments/{sha256}?company_id={company_id}&channel_id={channel_id}"
                            download="{filename}" title="Download {filename}">📎 {label}</a>"##,
            sha256 = escape_html_text(&attachment.sha256_hash),
            company_id = scope.company_id,
            channel_id = scope.channel_id,
            filename = escape_html_text(&attachment.filename),
        ),
        false => format!(
            r##"<span class="btn btn-xs btn-ghost btn-disabled gap-1 normal-case"
                            title="This attachment was not stored, so there is nothing to download">📎 {label}</span>"##
        ),
    }
}

/// An attachment's size, as somebody deciding whether to download it would read it.
fn file_size(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;

    match bytes {
        0..KB => format!("{bytes} B"),
        KB..MB => format!("{:.0} KB", bytes as f64 / KB as f64),
        _ => format!("{:.1} MB", bytes as f64 / MB as f64),
    }
}

/// Why the last submit did not go through, above the form that will be retried.
pub(crate) fn form_error_banner(error: Option<&str>) -> String {
    match error {
        Some(message) => format!(
            r##"<div class="alert alert-error mb-4 text-sm">{}</div>"##,
            escape_html_text(message)
        ),
        None => String::new(),
    }
}

/// An envelope field the sender cannot change: the addresses, and a reply's subject.
fn readonly_field(label: &str, value: &str) -> String {
    format!(
        r##"<label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">{label}</span></div>
                        <input type="text" class="input w-full font-mono text-sm" value="{value}" readonly>
                    </label>"##,
        label = escape_html_text(label),
        value = escape_html_text(value),
    )
}

/// The delivery toggle and the Send / Cancel row, shared by both send forms.
///
/// Only Cancel differs between them, so it arrives as the htmx attributes that undo this form.
fn send_form_footer(deliver: bool, quiet: bool, cancel_attributes: &str) -> String {
    format!(
        r##"<label class="label cursor-pointer justify-start gap-3">
                        <input type="checkbox" name="deliver" value="true" class="toggle toggle-primary toggle-sm" {deliver_checked}>
                        <span class="text-xs opacity-70">Deliver the agent reply by email (off keeps it in-app)</span>
                    </label>
                    <label class="label cursor-pointer justify-start gap-3">
                        <input type="checkbox" name="quiet" value="true" class="checkbox checkbox-primary checkbox-sm" {quiet_checked}>
                        <span class="text-xs opacity-70">Send quietly (save to history without running the agent)</span>
                    </label>
                    <div class="flex items-center gap-3 pt-2">
                        <button type="submit" class="btn btn-primary">
                            <span class="loading loading-spinner loading-sm hidden [.htmx-request_&]:inline-block"></span>
                            <span class="[.htmx-request_&]:hidden">Send</span>
                            <span class="hidden [.htmx-request_&]:inline">Sending...</span>
                        </button>
                        <button type="button" class="btn btn-ghost" {cancel_attributes}>Cancel</button>
                    </div>"##,
        deliver_checked = if deliver { "checked" } else { "" },
        quiet_checked = if quiet { "checked" } else { "" },
    )
}

pub fn compose_pane(pane: &ComposePane<'_>) -> String {
    format!(
        r##"
        <section id="detail-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <h2 class="text-xl font-bold">New thread in {channel_name}</h2>
                <p class="text-xs opacity-70">The message enters the channel as if it had been emailed in.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                <form hx-post="/ui/compose" hx-target="#detail-pane" hx-swap="outerHTML" class="space-y-4">
                    <input type="hidden" name="company_id" value="{company_id}">
                    <input type="hidden" name="channel_id" value="{channel_id}">
                    {to_field}
                    {from_field}
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Subject</span></div>
                        <input type="text" name="subject" required placeholder="Subject" value="{subject}"
                            class="input w-full">
                    </label>
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Message</span></div>
                        <textarea name="text_body" rows="10" required placeholder="Write the first message of the thread..."
                            class="textarea w-full">{text_body}</textarea>
                    </label>
                    {footer}
                </form>
            </div>
        </section>
        "##,
        channel_name = escape_html_text(&pane.channel.name),
        company_id = pane.company_id,
        channel_id = pane.channel.id,
        to_field = readonly_field("To", pane.channel_address),
        from_field = readonly_field("From", pane.sender_email),
        subject = escape_html_text(pane.subject),
        text_body = escape_html_text(pane.text_body),
        error_html = form_error_banner(pane.error),
        footer = send_form_footer(
            pane.deliver,
            pane.quiet,
            &format!(
                r##"hx-get="/ui/threads?company_id={company_id}&channel_id={channel_id}"
                            hx-target="#thread-column" hx-swap="outerHTML" hx-sync="#thread-column:replace""##,
                company_id = pane.company_id,
                channel_id = pane.channel.id,
            ),
        ),
    )
}

/// The form for a further message in an open thread — the header's "New Message" button.
///
/// The subject is fixed to the thread's own, because the message is threaded by its `In-Reply-To`
/// header rather than by what the sender types.
pub fn reply_pane(pane: &ReplyPane<'_>) -> String {
    format!(
        r##"
        <section id="detail-pane"{PANE_SKELETON} class="ui-pane-detail flex min-w-0 flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-4 py-4 sm:px-6">
                <h2 class="text-xl font-bold">New message in {subject}</h2>
                <p class="text-xs opacity-70">The message enters the channel as if it had been emailed in, continuing this thread.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
                {error_html}
                <form hx-post="/ui/reply" hx-target="#detail-pane" hx-swap="outerHTML" class="space-y-4">
                    <input type="hidden" name="company_id" value="{company_id}">
                    <input type="hidden" name="channel_id" value="{channel_id}">
                    <input type="hidden" name="thread_id" value="{thread_id}">
                    {to_field}
                    {from_field}
                    {subject_field}
                    <label class="form-control w-full">
                        <div class="label"><span class="text-xs opacity-70">Message</span></div>
                        <textarea name="text_body" rows="10" required placeholder="Write your message..."
                            class="textarea w-full">{text_body}</textarea>
                    </label>
                    {footer}
                </form>
            </div>
        </section>
        "##,
        subject = escape_html_text(&pane.thread.subject),
        company_id = pane.company_id,
        channel_id = pane.channel.id,
        thread_id = pane.thread.id,
        to_field = readonly_field("To", pane.channel_address),
        from_field = readonly_field("From", pane.sender_email),
        subject_field = readonly_field("Subject", &pane.thread.reply_subject()),
        text_body = escape_html_text(pane.text_body),
        error_html = form_error_banner(pane.error),
        footer = send_form_footer(
            pane.deliver,
            pane.quiet,
            &format!(
                r##"hx-get="/ui/messages?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                            hx-target="#detail-pane" hx-swap="outerHTML" hx-sync="#detail-pane:replace""##,
                company_id = pane.company_id,
                channel_id = pane.channel.id,
                thread_id = pane.thread.id,
            ),
        ),
    )
}

/// The opening tag of the thread list, in one place.
///
/// The container is rendered twice -- inline inside the column, and out of band after this client
/// sends a message -- and it carries the attributes that make the column live. Splitting those
/// across two string literals is what silently stopped the live column after the first send: the
/// out-of-band copy replaced the element with one that had no `sse-swap`, and nothing streamed
/// into it again until a full reload.
fn thread_list_open_tag(swap: FragmentSwap) -> String {
    format!(
        r##"<div id="thread-list"{THREAD_ROWS_SKELETON} data-thread-list class="flex-1 overflow-y-auto" sse-swap="thread" hx-swap="afterbegin"{oob}>"##,
        oob = swap.oob_attribute(),
    )
}

/// The thread list re-rendered out of band, so a freshly composed thread shows up in the column.
pub fn thread_list_oob(column: &ThreadColumn<'_>) -> String {
    format!(
        "{list_open}{list_html}</div>",
        list_open = thread_list_open_tag(FragmentSwap::OutOfBand),
        list_html = thread_list_fragment(column, FragmentSwap::Inline),
    )
}

/// The provider identifiers behind one message, for an operator matching it against a mail
/// server's or a provider's own logs.
///
/// Loaded on demand rather than rendered with the thread: this is the only pane in the app that
/// shows a provider key, and the request for it is authorized on its own. Every key is shown with
/// the interface it belongs to, because a bare key identifies nothing -- the same `Message-ID`
/// text is one outbound message on the sending channel's binding and one inbound message on the
/// receiving channel's.
pub fn message_diagnostics_pane(audit: &MessageAuditView) -> String {
    let keys = if audit.external_keys.is_empty() {
        r##"<p class="opacity-60">No transport carried this message.</p>"##.to_string()
    } else {
        audit
            .external_keys
            .iter()
            .map(|key| {
                format!(
                    r##"<tr>
                        <td class="font-mono">{transport}</td>
                        <td class="font-mono break-all">{binding}</td>
                        <td class="font-mono break-all">{value}</td>
                    </tr>"##,
                    transport = escape_html_text(key.transport.as_str()),
                    binding = escape_html_text(&key.binding_id.to_string()),
                    value = escape_html_text(key.key.as_str()),
                )
            })
            .collect()
    };

    format!(
        r##"
        <div class="space-y-3 text-xs">
            <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
                <dt class="opacity-60">Message</dt>
                <dd class="font-mono break-all">{canonical_id}</dd>
                <dt class="opacity-60">In this thread</dt>
                <dd class="font-mono break-all">{association_id}</dd>
                <dt class="opacity-60">Author</dt>
                <dd>{author}<span class="opacity-60 font-mono"> · {principal_id}</span></dd>
                <dt class="opacity-60">Direction / role</dt>
                <dd class="font-mono">{direction} · {role}</dd>
                <dt class="opacity-60">Correlation</dt>
                <dd class="font-mono break-all">{correlation_id}</dd>
                <dt class="opacity-60">Recorded</dt>
                <dd>{created_at}</dd>
            </dl>
            <div class="overflow-x-auto">
                <table class="table table-xs">
                    <thead><tr><th>Transport</th><th>Interface</th><th>Provider key</th></tr></thead>
                    <tbody>{keys}</tbody>
                </table>
            </div>
        </div>
        "##,
        canonical_id = escape_html_text(&audit.canonical_id.to_string()),
        association_id = escape_html_text(&audit.id.to_string()),
        author = escape_html_text(audit.author.display()),
        principal_id = escape_html_text(&audit.author.principal_id.to_string()),
        direction = escape_html_text(audit.direction.as_str()),
        role = escape_html_text(audit.role.as_str()),
        correlation_id = escape_html_text(&audit.correlation_id.to_string()),
        created_at = super::format_date_time(audit.created_at),
        keys = keys,
    )
}
