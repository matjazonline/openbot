//! The `/ui` mailbox: a daisyUI three-column reader — channels, then that channel's threads,
//! then the selected thread's messages.
//!
//! Every column is swapped over htmx: picking a channel replaces the thread column, picking a
//! thread replaces the detail pane, and Compose puts a new-thread form in that same pane.

use super::*;

/// Whether a fragment is being swapped into its own slot or piggybacking on another response.
///
/// The pagination `<div>` is rendered inline on a first page load and out-of-band when appended
/// to an existing list, so the two cases stay one function instead of a `bool` at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentSwap {
    Inline,
    OutOfBand,
}

impl FragmentSwap {
    pub(crate) fn oob_attribute(self) -> &'static str {
        match self {
            FragmentSwap::Inline => "",
            FragmentSwap::OutOfBand => r##" hx-swap-oob="outerHTML""##,
        }
    }
}

/// Who the mailbox is rendered for, as the top bar shows them.
pub struct MailboxUser<'a> {
    /// Their account id, which is what their own Team pane is keyed by.
    pub id: Uuid,
    pub username: &'a str,
    pub email: &'a EmailAddress,
    /// Their profile picture; `None` falls back to the letter bubble.
    pub avatar_url: Option<&'a AvatarUrl>,
}

/// Which `/ui` workspace a response belongs to, i.e. which rail icon is lit.
///
/// Both workspaces render through [`ui_shell`], so this is also what the company switcher needs
/// to keep a company change inside the workspace the user is already in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiSection {
    Mailbox,
    Dashboard,
    Channels,
    Agents,
    Schedules,
    Tasks,
    Outbox,
    Companies,
    Team,
}

impl UiSection {
    /// The `/ui` path this workspace is entered by, without any query string.
    pub(crate) fn base_path(self) -> &'static str {
        match self {
            UiSection::Mailbox => "/ui",
            UiSection::Dashboard => "/ui/dashboard",
            UiSection::Channels => "/ui/channels",
            UiSection::Agents => "/ui/agents",
            UiSection::Schedules => "/ui/schedules",
            UiSection::Tasks => "/ui/tasks",
            UiSection::Outbox => "/ui/outbox",
            UiSection::Companies => "/ui/companies",
            UiSection::Team => "/ui/team",
        }
    }
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

/// The chrome every `/ui` response shares: the top bar, the icon rail, and the HTML shell around
/// them. Only what sits to the right of the rail differs between workspaces.
pub struct UiShell<'a> {
    pub title: &'a str,
    pub user: &'a MailboxUser<'a>,
    /// `None` for a user with no company yet — there is nothing for the rail to point at.
    pub company_id: Option<Uuid>,
    pub section: UiSection,
    /// Everything to the right of the icon rail: the sidebar and its panes.
    pub content: &'a str,
    /// Workspace-specific JavaScript, appended after the shared [`MAILBOX_SCRIPT`].
    pub script: &'a str,
}

/// The full HTML document for one `/ui` response.
pub fn ui_shell(shell: &UiShell<'_>) -> String {
    let rail = match shell.company_id {
        Some(company_id) => icon_rail(company_id, shell.section),
        None => String::new(),
    };

    let body = format!(
        r##"
    <div class="flex h-screen flex-col">
        {top_bar}
        <div class="flex min-h-0 flex-1">
            {rail}
            {content}
        </div>
    </div>
    {LOGOUT_MODAL}
        "##,
        top_bar = top_bar(shell.user, shell.company_id),
        content = shell.content,
    );

    ui_layout(shell.title, &body, shell.script)
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
    pub messages: &'a [Message],
    /// The face and name the agent side of this thread is drawn with -- see [`message_bubble_chat`].
    pub agent: Option<&'a Agent>,
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
    pub error: Option<&'a str>,
}

/// Client-side behaviour for the mailbox: selection highlighting, and keeping an opened thread
/// scrolled to its newest message — every load is htmx.
///
/// Kept out of the `format!` blocks below so its braces need no escaping.
const MAILBOX_SCRIPT: &str = r##"        // The `theme-controller` checkbox already repaints the page by itself -- daisyUI matches
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

        // Scoped to the clicked entry's own list, so every `/ui` sidebar highlights with it.
        function selectSidebarItem(el) {
            var menu = el.closest('ul');
            if (!menu) return;
            menu.querySelectorAll('a').forEach(function (item) {
                item.classList.remove('menu-active');
            });
            el.classList.add('menu-active');
        }

        function selectThreadRow(el) {
            document.querySelectorAll('#thread-list .thread-row').forEach(function (row) {
                row.classList.remove('bg-base-300');
            });
            el.classList.add('bg-base-300');
            // Opening a thread is reading it, so its reply mark has done its job.
            clearReplyMark(el);
        }

        function clearReplyMark(row) {
            var mark = row.querySelector('.thread-mark');
            if (mark) {
                mark.textContent = '';
                mark.removeAttribute('title');
            }
        }

        // Mark threads whose agent answered while the reader was looking elsewhere. The glyph is
        // an inline SVG rendered by the server into AGENT_REPLIED_MARK, so it matches the one a
        // freshly loaded row is drawn with instead of being a second copy of it here.
        //
        // This is deliberately a client-side, in-session signal rather than a stored unread flag:
        // it means "this happened while you were watching", so a reload starting clean is correct
        // rather than a bug. The open thread never gets a mark -- the reply is already on screen.
        function markLiveAgentReplies(list) {
            var pane = document.getElementById('detail-pane');
            var openThreadId = pane ? pane.dataset.threadId : null;

            list.querySelectorAll('.thread-row[data-last-role="agent"]').forEach(function (row) {
                if (row.dataset.threadId === openThreadId) return;
                var mark = row.querySelector('.thread-mark');
                if (!mark) return;
                mark.innerHTML = AGENT_REPLIED_MARK;
                mark.title = 'Agent replied';
            });
        }

        // A thread reads newest-last, so opening one should land on its latest message.
        function scrollMessagesToBottom() {
            var pane = document.getElementById('message-scroll');
            if (pane) {
                pane.scrollTop = pane.scrollHeight;
            }
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
        window.addEventListener('load', scrollMessagesToBottom);

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
            if (swapped && swapped.classList && swapped.classList.contains('thread-activity')) {
                return;
            }

            // The open thread's activity strip appearing makes the pane taller, which would push
            // the newest message out of view for someone sitting at the live edge.
            if (swapped && swapped.id === 'thread-activity') {
                if (wasAtBottomBeforeStream) {
                    scrollMessagesToBottom();
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

            // The thread is no longer empty, so its placeholder must go.
            var placeholder = document.getElementById('no-messages');
            if (placeholder) {
                placeholder.remove();
            }
            if (wasAtBottomBeforeStream) {
                scrollMessagesToBottom();
            }
        });

        // Only the swaps that bring in the messages themselves: paging the thread column must not
        // yank an already-open thread back down.
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
                scrollMessagesToBottom();
                // A sent message swaps the whole pane, so hand the caret back to the new box.
                var composer = document.getElementById('thread-composer');
                if (composer) {
                    composer.focus();
                }
            }
        });"##;

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
fn ui_layout(title: &str, body: &str, script: &str) -> String {
    let skeletons = skeleton_script();

    format!(
        r##"<!DOCTYPE html>
<html lang="en" data-theme="dark" class="h-full">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title} - Mail Agents</title>
    <link href="https://cdn.jsdelivr.net/npm/daisyui@5" rel="stylesheet" type="text/css" />
    <link href="https://cdn.jsdelivr.net/npm/daisyui@5/themes.css" rel="stylesheet" type="text/css" />
    <style>{BRAND_LOGO_STYLES}{DARK_THEME_BLUES}{FIELD_STYLES}</style>
    <script>{THEME_INIT_SCRIPT}</script>
    <script src="https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4"></script>
    <script src="https://unpkg.com/htmx.org@2.0.4"></script>
    <script src="https://unpkg.com/htmx-ext-sse@2.2.3/sse.js"></script>
</head>
<body class="h-full overflow-hidden bg-base-100 text-base-content">
{body}
    <script>
        var AGENT_REPLIED_MARK = '{agent_replied_mark}';
{MAILBOX_SCRIPT}
{skeletons}
{script}
    </script>
</body>
</html>"##,
        agent_replied_mark = icon(Icon::Check, BUTTON_ICON),
    )
}

pub fn mailbox_page(page: &MailboxPage<'_>) -> String {
    let thread_column_html = match page.selected_channel {
        Some(channel) => thread_column(&ThreadColumn {
            company_id: page.company.id,
            channel,
            threads: page.threads,
            next_cursor: page.next_cursor,
            selected_thread_id: page.selected_thread_id,
            activity: page.activity,
        }),
        None => empty_thread_column(),
    };

    let content = format!(
        "{sidebar}{thread_column_html}{detail_html}",
        sidebar = channel_sidebar(page),
        detail_html = page.detail_html,
    );

    ui_shell(&UiShell {
        title: &format!("{} Mailbox", page.company.name),
        user: page.user,
        company_id: Some(page.company.id),
        section: UiSection::Mailbox,
        content: &content,
        script: "",
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
        company_id: None,
        section: UiSection::Mailbox,
        content,
        script: "",
    })
}

/// The wordmark, in both inks: the dark one for the light theme, the light one for the dark
/// theme. Both are sent, and [`BRAND_LOGO_STYLES`] hides the one that does not belong -- swapping
/// the `src` from script instead would leave the mark blank for a moment on every theme change.
const BRAND_LOGO: &str = r##"
                <img src="/assets/busybots-logo-dark-hor.png" alt="BusyBots"
                    class="brand-logo brand-logo-on-light h-10 w-auto">
                <img src="/assets/busybots-logo-light.png" alt="BusyBots"
                    class="brand-logo brand-logo-on-dark h-10 w-auto">
"##;

/// Which of [`BRAND_LOGO`]'s two inks the current theme shows. Written against `data-theme` on
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
                        onchange="applyTheme(this.checked ? 'light' : 'dark')" />
                    <svg class="swap-off h-5 w-5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M12 3v2.25m6.364.386-1.591 1.591M21 12h-2.25m-.386 6.364-1.591-1.591M12 18.75V21m-4.773-4.227-1.591 1.591M5.25 12H3m4.227-4.773L5.636 5.636M15.75 12a3.75 3.75 0 1 1-7.5 0 3.75 3.75 0 0 1 7.5 0Z" />
                    </svg>
                    <svg class="swap-on h-5 w-5" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" stroke-width="1.5" stroke="currentColor">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M21.752 15.002A9.72 9.72 0 0 1 18 15.75c-5.385 0-9.75-4.365-9.75-9.75 0-1.33.266-2.597.748-3.752A9.753 9.753 0 0 0 3 11.25C3 16.635 7.365 21 12.75 21a9.753 9.753 0 0 0 9.002-5.998Z" />
                    </svg>
                </label>
"##;

/// The bar across the top of every mailbox response: the brand mark on the left, the signed-in
/// account on the right. It sits above all four columns, so it is the one piece of chrome that
/// never moves when htmx swaps a column.
///
/// The brand mark comes in two inks, one per theme -- see [`BRAND_LOGO_STYLES`].
fn top_bar(user: &MailboxUser<'_>, company_id: Option<Uuid>) -> String {
    // Profile lives in the Team workspace -- your own pane there is where the avatar is set -- so
    // the entry only appears once there is a company whose team you are on.
    let profile_entry = match company_id {
        Some(company_id) => format!(
            r##"<li><a href="/ui/team?company_id={company_id}&member_id={user_id}">Profile</a></li>"##,
            user_id = user.id,
        ),
        None => String::new(),
    };

    format!(
        r##"
        <header class="navbar h-16 min-h-16 shrink-0 justify-between gap-4 border-b border-base-300 bg-base-200 px-4">
            <a href="/ui" class="flex items-center" title="BusyBots">
{BRAND_LOGO}
            </a>
            <div class="flex items-center gap-1">
{THEME_CONTROLLER}
                <div class="dropdown dropdown-end">
                    <div tabindex="0" role="button" class="btn btn-ghost h-auto gap-3 px-2 py-1">
                        <div class="hidden text-right leading-tight sm:block">
                            <div class="text-sm font-semibold">{username}</div>
                            <div class="text-[11px] font-normal opacity-60">{email}</div>
                        </div>
                        <div id="account-avatar">{avatar}</div>
                    </div>
                    <ul tabindex="0" class="menu menu-sm dropdown-content z-50 mt-3 w-60 rounded-box bg-base-300 p-2 shadow-xl">
                        <li class="menu-title truncate">{email}</li>
                        {profile_entry}
                        <li><a href="/ui/companies">Companies</a></li>
                        <li>
                            <button type="button" class="w-full text-left" onclick="confirmLogout()">Log out</button>
                        </li>
                    </ul>
                </div>
            </div>
        </header>
        "##,
        username = escape_html_text(user.username),
        email = escape_html_text(user.email),
        avatar = avatar_bubble(user.avatar_url, user.username, AvatarSize::Bar),
    )
}

/// The top bar's avatar on its own, swapped out of band after the account's picture changes.
///
/// The bar is the one piece of chrome an htmx swap never replaces, so without this the reader
/// would keep seeing their old face until the next full page load.
pub fn account_avatar_oob(username: &str, avatar_url: Option<&AvatarUrl>) -> String {
    format!(
        r##"<div id="account-avatar" hx-swap-oob="outerHTML">{avatar}</div>"##,
        avatar = avatar_bubble(avatar_url, username, AvatarSize::Bar),
    )
}

/// The slim left rail: the `/ui` workspaces first, then links out to the classic pages.
///
/// The workspace the response belongs to is lit, so the rail says where you are as well as where
/// you can go.
fn icon_rail(company_id: Uuid, section: UiSection) -> String {
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
            UiSection::Outbox,
            "/ui/outbox",
            Icon::PaperAirplane,
            "Outbox",
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
        (UiSection::Team, "/ui/team", Icon::People, "Team"),
    ];

    let links: String = destinations
        .iter()
        .map(|(destination, path, glyph, title)| {
            format!(
                r##"<a href="{path}?company_id={company_id}" class="btn btn-square btn-lg {style}" title="{title}">{glyph}</a>"##,
                style = if section == *destination {
                    "btn-primary"
                } else {
                    "btn-ghost"
                },
                glyph = icon(*glyph, RAIL_ICON),
            )
        })
        .collect();

    format!(
        r##"
        <nav class="flex w-16 shrink-0 flex-col items-center gap-2 border-r border-base-300 bg-base-300 py-3">
            {links}
            <button type="button" class="btn btn-square btn-lg btn-ghost mt-auto"
                title="Log out" onclick="confirmLogout()">{sign_out}</button>
        </nav>
        "##,
        sign_out = icon(Icon::SignOut, RAIL_ICON),
    )
}

/// The rail's buttons are square and large, so its glyphs are drawn a size up from body icons.
const RAIL_ICON: &str = "h-6 w-6";

/// The channel column: one menu entry per channel, channel actions at the bottom.
fn channel_sidebar(page: &MailboxPage<'_>) -> String {
    let company_switcher = company_switcher(page.company, page.companies, UiSection::Mailbox);
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
        <aside class="flex w-64 shrink-0 flex-col border-r border-base-300 bg-base-200">
            {company_switcher}
            <ul id="channel-menu" class="menu w-full flex-1 flex-nowrap gap-1 overflow-y-auto px-2">
                {menu_body}
            </ul>
            {footer}
        </aside>
        "##,
        footer = channel_actions(page.company.id, page.selected_channel, FragmentSwap::Inline),
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

/// The company picker at the top of both sidebars. Switching company stays in the workspace the
/// user is already in, so `section` decides where the options point.
pub(crate) fn company_switcher(
    company: &Company,
    companies: &[Company],
    section: UiSection,
) -> String {
    let options: String = companies
        .iter()
        .map(|c| {
            format!(
                r##"<li><a href="{base_path}?company_id={id}" class="{active}">{name}</a></li>"##,
                base_path = section.base_path(),
                id = c.id,
                active = if c.id == company.id {
                    "menu-active"
                } else {
                    ""
                },
                name = escape_html_text(&c.name),
            )
        })
        .collect();

    format!(
        r##"
            <div class="dropdown dropdown-bottom w-full p-3">
                <div tabindex="0" role="button" class="btn btn-ghost btn-block justify-between">
                    <span class="truncate font-bold">{name}</span>
                    <span class="opacity-60">{caret}</span>
                </div>
                <ul tabindex="0" class="menu dropdown-content z-50 mt-1 w-56 rounded-box bg-base-300 p-2 shadow-xl">
                    {options}
                </ul>
            </div>
        "##,
        name = escape_html_text(&company.name),
        caret = icon(Icon::ChevronDown, BUTTON_ICON),
    )
}

/// One `<option>` per channel for a `/ui` sidebar filter, with the current one marked.
///
/// Shared by the Tasks and Outbox workspaces: both filter the same company's channels the same
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
                    hx-target="#detail-pane" hx-swap="outerHTML">{pencil} New Thread</button>"##,
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
                    <a class="flex flex-col items-start gap-0.5 {active}"
                        hx-get="/ui/threads?company_id={company_id}&channel_id={channel_id}"
                        hx-target="#thread-column" hx-swap="outerHTML"
                        hx-push-url="/ui?company_id={company_id}&channel_id={channel_id}"
                        onclick="selectSidebarItem(this)">
                        <span class="flex w-full items-center gap-2">
                            <span class="truncate">{name}</span>
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
        <section id="thread-column"{THREAD_COLUMN_SKELETON} class="flex w-96 shrink-0 flex-col border-r border-base-300 bg-base-100">
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
        <section id="thread-column"{THREAD_COLUMN_SKELETON} class="flex w-96 shrink-0 flex-col border-r border-base-300 bg-base-100"
            hx-ext="sse"
            sse-connect="/ui/threads/events?company_id={company_id}&channel_id={channel_id}{after}">
            <div class="flex items-center justify-between gap-2 border-b border-base-300 px-4 py-3">
                <div class="min-w-0">
                    <h2 class="truncate text-lg font-bold">{channel_name}</h2>
                    <p class="truncate text-xs opacity-60">Newest threads first</p>
                </div>
                <div class="flex shrink-0 items-center gap-2">
                    {compose_button}
                    <button type="button" class="btn btn-ghost btn-sm btn-square text-xl leading-none" title="Reload threads"
                        hx-get="/ui/threads?company_id={company_id}&channel_id={channel_id}"
                        hx-target="#thread-column" hx-swap="outerHTML">{reload_glyph}</button>
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

/// A thread row's activity mark, or nothing at all when the thread is idle.
///
/// Every state gets a glyph, because every state now tells the reader something they cannot see
/// from the messages: the agent has not started, or is answering, or has stopped and needs a
/// person. Only a live run animates -- it is the one thing that will resolve on its own -- and only
/// a failure takes a colour. The rest stay quiet, with the wording on hover.
pub fn thread_activity_mark(activity: Option<ThreadActivity>) -> String {
    match activity {
        None => String::new(),
        Some(activity) => format!(
            r##"<span class="shrink-0 leading-none {tint}{pulse}" title="{label}">{mark}</span>"##,
            tint = if activity == ThreadActivity::Failed {
                "text-error"
            } else {
                "opacity-60"
            },
            pulse = if activity.is_running() {
                " animate-pulse"
            } else {
                ""
            },
            label = escape_html_text(activity.label()),
            mark = icon(activity_icon(activity), BUTTON_ICON),
        ),
    }
}

/// The glyph standing in for a thread state where there is room for a mark but not for a sentence.
///
/// `Queued` and `Working` share one on purpose: a queued task is picked up within a poll interval,
/// and swapping shapes that fast reads as a flicker rather than as progress. The full wording
/// rides along as the element's `title`.
fn activity_icon(activity: ThreadActivity) -> Icon {
    match activity {
        ThreadActivity::Queued | ThreadActivity::Working => Icon::DotFill,
        ThreadActivity::WaitingApproval => Icon::Hourglass,
        ThreadActivity::WaitingReply => Icon::Mail,
        ThreadActivity::Failed => Icon::Alert,
    }
}

/// A thread row's activity slot: an element the live column can swap into on its own.
///
/// Each row listens on its own event name so a status change redraws only that badge. Reusing the
/// column's `thread` event would insert the whole row at the top of the list, moving a thread
/// merely because its task changed state -- a reorder nobody asked for.
///
/// `hx-target="this"` is not decoration. htmx attributes are inherited, and this slot lives inside
/// a row button carrying `hx-target="#detail-pane"`; without pinning the target, every badge
/// update swapped itself over the whole open conversation.
fn thread_activity_slot(thread_id: Uuid, activity: Option<ThreadActivity>) -> String {
    format!(
        r##"<span class="thread-activity" sse-swap="{event}" hx-target="this" hx-swap="innerHTML">{mark}</span>"##,
        event = thread_activity_event(thread_id),
        mark = thread_activity_mark(activity),
    )
}

/// The SSE event name carrying one thread's activity. Shared with the stream that emits it.
pub fn thread_activity_event(thread_id: Uuid) -> String {
    format!("activity-{thread_id}")
}

/// The strip under the open thread's messages: a spinner while an agent is working, a plain badge
/// while it is blocked, and nothing when the thread is idle.
///
/// Unlike the column, this says everything it knows. The reader is here, waiting on this thread,
/// so "Agent replying…" is the answer to the question they are actually asking -- where the same
/// badge on a row they are not looking at would just be noise.
///
/// The spinner is shown only for work actually in progress. A thread parked on an approval gets a
/// badge with no spinner, because something spinning forever reads as broken rather than blocked.
pub fn thread_activity_strip(activity: Option<ThreadActivity>) -> String {
    let Some(activity) = activity else {
        return String::new();
    };

    let spinner = if activity == ThreadActivity::Working {
        r##"<span class="loading loading-dots loading-sm"></span>"##
    } else {
        ""
    };

    format!(
        r##"<div class="flex items-center gap-2 border-t border-base-300 px-6 py-2 text-xs opacity-70">{spinner}<span class="badge badge-sm shrink-0 {style}">{label}</span></div>"##,
        style = task_status_style(activity.task_status()),
        label = escape_html_text(activity.label()),
    )
}

fn thread_row(column: &ThreadColumn<'_>, thread: &Thread) -> String {
    thread_row_fragment(
        column.company_id,
        column.channel,
        thread,
        column.selected_thread_id == Some(thread.id),
        column.activity.get(&thread.id).copied(),
        None,
    )
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
    activity: Option<ThreadActivity>,
    last_role: Option<MessageRole>,
) -> String {
    let participants = if thread.participant_emails.is_empty() {
        "No participants".to_string()
    } else {
        escape_html_text(&thread.participant_emails.join(", "))
    };
    let channel_id = channel.id;

    format!(
        r##"
                <button type="button" data-thread-id="{thread_id}"{last_role}
                    class="thread-row block w-full border-b border-base-300 px-4 py-3 text-left transition hover:bg-base-200 {active}"
                    hx-get="/ui/messages?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                    hx-target="#detail-pane" hx-swap="outerHTML"
                    hx-push-url="/ui?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                    onclick="selectThreadRow(this)">
                    <div class="flex items-baseline justify-between gap-2">
                        <span class="truncate font-semibold">{subject}</span>
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
        activity_slot = thread_activity_slot(thread.id, activity),
        // Only rows arriving over the stream carry this: the reply mark means "while you were
        // watching", so a freshly rendered page has nothing to mark and needs no lookup.
        last_role = match last_role {
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
                        hx-target="#thread-list" hx-swap="beforeend">
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
        <section id="detail-pane"{PANE_SKELETON} class="flex flex-1 items-center justify-center bg-base-100 p-8"{oob}>
            <p class="text-center text-sm opacity-60">{message}</p>
        </section>
        "##,
        oob = swap.oob_attribute(),
        message = escape_html_text(message),
    )
}

pub fn message_pane(pane: &MessagePane<'_>) -> String {
    let participants = if pane.thread.participant_emails.is_empty() {
        "No participants".to_string()
    } else {
        escape_html_text(&pane.thread.participant_emails.join(", "))
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
        <section id="detail-pane"{PANE_SKELETON} data-thread-id="{thread_id}" class="flex flex-1 flex-col bg-base-100" hx-ext="sse"
            sse-connect="/ui/events?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}{after}">
            <div class="flex items-start justify-between gap-3 border-b border-base-300 px-6 py-4">
                <div class="min-w-0">
                    <h2 class="truncate text-xl font-bold">{subject}</h2>
                    <p class="truncate text-xs opacity-70">{participants}</p>
                    <p class="truncate font-mono text-[11px] opacity-40">{thread_id}</p>
                </div>
                <div class="flex shrink-0 items-center gap-2">
                    <button type="button" class="btn btn-primary btn-sm" title="Send another message in this thread"
                        hx-get="/ui/reply?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                        hx-target="#detail-pane" hx-swap="outerHTML">{pencil} New Message</button>
                    <button type="button" class="btn btn-ghost btn-sm btn-square" title="Reload messages"
                        hx-get="/ui/messages?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                        hx-target="#detail-pane" hx-swap="outerHTML">{reload}</button>
                    <a href="/companies/{company_id}/channels/{channel_id}/simulate?thread_id={thread_id}"
                        class="btn btn-outline btn-sm">Open in Simulator</a>
                </div>
            </div>
            <div id="message-scroll" class="flex-1 space-y-1 overflow-y-auto px-6 py-4"
                sse-swap="message" hx-swap="beforeend">
                {messages_html}
            </div>
            <div id="thread-activity" sse-swap="activity" hx-target="this" hx-swap="innerHTML">{activity_strip}</div>
            {composer}
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
    )
}

/// The chat-style send box under the messages: one line that grows as it is typed, sending into
/// the open thread without leaving the pane.
///
/// It posts to the same `/ui/reply` endpoint as the header's "New Message" form, so a rejected
/// message comes back as that fuller form with the text kept and the reason on top.
fn thread_composer(pane: &MessagePane<'_>) -> String {
    format!(
        r##"
            <form class="border-t border-base-300 px-6 py-3" hx-post="/ui/reply"
                hx-target="#detail-pane" hx-swap="outerHTML">
                <input type="hidden" name="company_id" value="{company_id}">
                <input type="hidden" name="channel_id" value="{channel_id}">
                <input type="hidden" name="thread_id" value="{thread_id}">
                <div class="flex items-end gap-2">
                    <div class="aura aura-holo flex-1 [--aura-radius:var(--radius-field)]">
                        <textarea id="thread-composer" name="text_body" rows="1" required
                            placeholder="Write a message... (Enter to send, Shift+Enter for a new line)"
                            class="textarea block max-h-40 min-h-12 w-full resize-none text-sm"
                            onkeydown="composerKeydown(event)" oninput="autoGrowComposer(this)"></textarea>
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
    message: &Message,
    agent: Option<&Agent>,
    scope: MessageScope,
) -> String {
    let is_agent =
        message.role == MessageRole::Agent || message.direction == MessageDirection::Outbound;
    let body = if is_agent {
        format!(
            r##"<div class="{MARKDOWN_CONTENT_STYLES}">{}</div>"##,
            render_markdown(&message.clean_text_body)
        )
    } else {
        format!(
            r##"<div class="whitespace-pre-wrap">{}</div>"##,
            escape_html_text(&message.clean_text_body)
        )
    };

    let (writer, avatar_url) = match (is_agent, agent) {
        (true, Some(agent)) => (agent.name.as_str(), agent.avatar_url.as_ref()),
        _ => (message.sender.as_str(), None),
    };

    format!(
        r##"
                <div class="chat {side}">
                    <div class="chat-image">{avatar}</div>
                    <div class="chat-header gap-1 opacity-70">
                        {writer}
                        <time class="text-xs opacity-60">{created_at}</time>
                    </div>
                    <div class="chat-bubble {bubble_class} max-w-2xl text-sm">{body}{attachments}</div>
                    <div class="chat-footer font-mono text-[11px] opacity-40">{subject}</div>
                </div>
        "##,
        side = if is_agent { "chat-end" } else { "chat-start" },
        bubble_class = if is_agent { "chat-bubble-primary" } else { "" },
        avatar = avatar_bubble(avatar_url, writer, AvatarSize::Row),
        writer = escape_html_text(writer),
        created_at = super::format_date_time(message.created_at),
        subject = escape_html_text(&message.subject),
        body = body,
        attachments = attachment_chips(message, scope),
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
fn attachment_chips(message: &Message, scope: MessageScope) -> String {
    let Some(attachments) = message.attachments.as_ref().filter(|a| !a.is_empty()) else {
        return String::new();
    };

    let chips: String = attachments
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
fn send_form_footer(deliver: bool, cancel_attributes: &str) -> String {
    format!(
        r##"<label class="label cursor-pointer justify-start gap-3">
                        <input type="checkbox" name="deliver" value="true" class="toggle toggle-primary toggle-sm" {deliver_checked}>
                        <span class="text-xs opacity-70">Deliver the agent reply by email (off keeps it in-app)</span>
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
    )
}

pub fn compose_pane(pane: &ComposePane<'_>) -> String {
    format!(
        r##"
        <section id="detail-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-6 py-4">
                <h2 class="text-xl font-bold">New thread in {channel_name}</h2>
                <p class="text-xs opacity-70">The message enters the channel as if it had been emailed in.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
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
            &format!(
                r##"hx-get="/ui/threads?company_id={company_id}&channel_id={channel_id}"
                            hx-target="#thread-column" hx-swap="outerHTML""##,
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
        <section id="detail-pane"{PANE_SKELETON} class="flex flex-1 flex-col bg-base-100">
            <div class="border-b border-base-300 px-6 py-4">
                <h2 class="text-xl font-bold">New message in {subject}</h2>
                <p class="text-xs opacity-70">The message enters the channel as if it had been emailed in, continuing this thread.</p>
            </div>
            <div class="flex-1 overflow-y-auto px-6 py-4">
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
            &format!(
                r##"hx-get="/ui/messages?company_id={company_id}&channel_id={channel_id}&thread_id={thread_id}"
                            hx-target="#detail-pane" hx-swap="outerHTML""##,
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
        r##"<div id="thread-list"{THREAD_ROWS_SKELETON} class="flex-1 overflow-y-auto" sse-swap="thread" hx-swap="afterbegin"{oob}>"##,
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
