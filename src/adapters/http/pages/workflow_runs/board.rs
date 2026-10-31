//! The runs workspace as a board (`/ui/workflow-runs?view=board`, plan `new-board-view`): one
//! column per run state over the company's top-level runs, every open run and the runs finished
//! inside the window, each column cut at
//! [`BOARD_COLUMN_CAP`](crate::application::workflow::board::BOARD_COLUMN_CAP). A card opens the
//! run in the same `#run-pane` the list uses, beside the board.
//!
//! `#run-board` holds the toolbar and the columns, so a filter change replaces it whole (as
//! `#run-list` holds its own filter). Its empty reload leaf reads the region again with the
//! toolbar's current filters and swaps only the columns' content, so the board's horizontal
//! scroll survives a wake-up and a wake-up never undoes a filter the reader just chose.
//!
//! On a phone the board is the shell's list column and the pane its detail column
//! (`COMPACT_LAYOUT_STYLES`): one at a time, the pane's back control returning to the board.
use super::*;
use crate::adapters::http::pages::dashboard::elapsed_label;
use crate::application::workflow::board::{BoardWindow, RunBoardColumn};
use crate::application::workflow::reads::ChildCounts;
use crate::domain::workflow::RunId;
use chrono::{DateTime, Utc};

/// Which view of the runs workspace a request asks for: `view=board`, or the list for anything
/// else, so every link that names no view keeps opening the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunsView {
    List,
    Board,
}

impl RunsView {
    pub fn parse(view: Option<&str>) -> Self {
        match view {
            Some("board") => Self::Board,
            _ => Self::List,
        }
    }
}

/// The board's window from a request: `day`, `week` or `month`; anything else is the default, as
/// an unknown state lists every state.
pub fn board_window(window: Option<&str>) -> BoardWindow {
    match window {
        Some("day") => BoardWindow::Day,
        Some("month") => BoardWindow::Month,
        _ => BoardWindow::default(),
    }
}

/// How the window reads under a finished column and in its select.
fn window_label(window: BoardWindow) -> &'static str {
    match window {
        BoardWindow::Day => "the last 24 hours",
        BoardWindow::Week => "the last 7 days",
        BoardWindow::Month => "the last 30 days",
    }
}

pub struct RunBoardView<'a> {
    pub company_id: Uuid,
    /// In [`crate::application::workflow::reads::RunStateName::ALL`] order.
    pub columns: &'a [RunBoardColumn],
    /// Child counts of the shown cards that have children.
    pub children: &'a HashMap<RunId, ChildCounts>,
    /// Workflow titles by workflow id.
    pub titles: &'a HashMap<Uuid, String>,
    pub window: BoardWindow,
    /// The scope the board shows; its state is always `None`, the states are the columns.
    pub scope: RunFilterView<'a>,
    /// What the scope controls offer.
    pub options: &'a ScopeOptions,
    /// The run open in the pane, highlighted.
    pub selected: Option<Uuid>,
    /// The instant the board was read at: what the cards' ages count from.
    pub as_of: DateTime<Utc>,
}

impl RunBoardView<'_> {
    /// The board's own address, scope and window kept, already escaped for an attribute.
    fn href(&self) -> String {
        format!(
            "/ui/workflow-runs?{}&amp;view=board&amp;window={}",
            self.scope.query(self.company_id),
            self.window.as_str()
        )
    }
}

/// A card's link: the full board with the run open for a new tab, and in place the run's pane
/// into `#run-pane`, pushing the board's address with the run, the card and the pane marked busy
/// meanwhile.
fn card_attrs(view: &RunBoardView<'_>, run: Uuid) -> String {
    let company_id = view.company_id;
    let href = format!("{}&amp;run_id={run}", view.href());
    format!(
        r##"href="{href}" hx-get="/ui/workflow-runs/{run}?company_id={company_id}" hx-target="#run-pane" hx-swap="outerHTML"
            hx-sync="#run-pane:replace" hx-push-url="{href}" data-busy="#run-pane""##
    )
}

/// "2 child runs · 1 open · 1 failed", or nothing for a run without children.
fn child_rollup(children: ChildCounts) -> String {
    if children.total == 0 {
        return String::new();
    }
    let mut label = format!(
        "{} child run{}",
        children.total,
        if children.total == 1 { "" } else { "s" }
    );
    if children.open > 0 {
        label.push_str(&format!(" · {} open", children.open));
    }
    if children.failed > 0 {
        label.push_str(&format!(" · {} failed", children.failed));
    }
    format!(r#"<span class="text-xs opacity-70">{label}</span>"#)
}

fn board_card(view: &RunBoardView<'_>, run: &RunSummary) -> String {
    let id = run.run.as_uuid();
    let title = view
        .titles
        .get(&run.workflow.as_uuid())
        .map(String::as_str)
        .unwrap_or("(workflow)");
    // Open runs age from creation, finished ones from finishing.
    let age = match run.finished_at {
        Some(finished) => format!("finished {}", elapsed_label(finished, view.as_of)),
        None => format!("created {}", elapsed_label(run.created_at, view.as_of)),
    };
    let rerun = run
        .rerun_of
        .map(|original| {
            format!(
                r#"<span class="badge badge-xs badge-outline">rerun of {}</span>"#,
                short_id(original.as_uuid())
            )
        })
        .unwrap_or_default();
    let selected = view.selected == Some(id);
    format!(
        r##"<li><a {attrs} data-run="{id}"{current}
            class="flex flex-col items-start gap-1 rounded-box border bg-base-100 p-2 text-sm hover:bg-base-200 aria-busy:cursor-progress aria-busy:opacity-60 {edge}">
            <span class="w-full truncate font-semibold" title="{title_attr}">{title}</span>
            {state}
            <span class="font-mono text-[11px] opacity-70">run {short} · {age}</span>{rerun}{children}</a></li>"##,
        attrs = card_attrs(view, id),
        current = if selected {
            r#" aria-current="true""#
        } else {
            ""
        },
        edge = if selected {
            "border-primary"
        } else {
            "border-base-300"
        },
        title_attr = escape_html_attr(title),
        title = escape_html_text(title),
        state = run_state_badge(&run.state, run.waiting_reason.as_deref()),
        short = short_id(id),
        children = child_rollup(view.children.get(&run.run).copied().unwrap_or_default()),
    )
}

fn board_column(view: &RunBoardView<'_>, column: &RunBoardColumn) -> String {
    let state = column.state.as_str();
    let window = if column.state.is_open() {
        String::new()
    } else {
        format!(
            r#"<p class="text-xs opacity-70">finished in {}</p>"#,
            window_label(view.window)
        )
    };
    let cut = if column.cut {
        format!(
            r#"<p class="text-xs opacity-70" data-cut>Showing {shown} of {total} · <a class="link" href="/ui/workflow-runs?{scope}&amp;state={state}">open in list</a></p>"#,
            shown = column.cards.len(),
            total = column.total,
            scope = view.scope.query(view.company_id),
        )
    } else {
        String::new()
    };
    let cards: String = if column.cards.is_empty() {
        r#"<li class="px-1 text-sm opacity-70">No runs.</li>"#.to_owned()
    } else {
        column
            .cards
            .iter()
            .map(|run| board_card(view, run))
            .collect()
    };
    format!(
        r#"<section class="flex max-h-full w-72 shrink-0 flex-col rounded-box bg-base-200" data-state="{state}" aria-label="{state} runs">
            <header class="space-y-0.5 border-b border-base-300 px-3 py-2">
                <h2 class="flex items-center justify-between font-semibold"><span>{state}</span><span class="badge badge-sm" data-total>{total}</span></h2>{window}{cut}
            </header>
            <ul class="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-2">{cards}</ul>
        </section>"#,
        total = column.total,
    )
}

/// The List/Board switch: the same scope, only `view` changes.
pub fn runs_view_toggle(company_id: Uuid, scope: RunFilterView<'_>, current: RunsView) -> String {
    let query = scope.query(company_id);
    let tab = |label: &str, href: String, active: bool| {
        format!(
            r#"<a class="tab{}" href="{href}"{}>{label}</a>"#,
            if active { " tab-active" } else { "" },
            if active {
                r#" aria-current="page""#
            } else {
                ""
            },
        )
    };
    format!(
        r#"<nav class="tabs tabs-box tabs-xs shrink-0" aria-label="Runs view">{}{}</nav>"#,
        tab(
            "List",
            format!("/ui/workflow-runs?{query}"),
            current == RunsView::List
        ),
        tab(
            "Board",
            format!("/ui/workflow-runs?{query}&amp;view=board"),
            current == RunsView::Board
        ),
    )
}

/// The `#run-board` region: the toolbar (view switch, scope, window) and the six columns. Its
/// reload leaf reads the region again on the stream's `runs`, `resync` and `tick` (the tick moves
/// finished runs out of the window and ages the cards: the cutoff is recomputed on every read) and
/// when another run is opened, with the open run highlighted and whatever the toolbar's form holds
/// now, and swaps in only the columns. Wake-ups are coalesced in `WORKFLOW_RUNS_SCRIPT`
/// (`data-board-wake`, replayed as `board-wake`): no two board reads overlap, at most one starts
/// per two seconds after the previous one settles, and a wake-up during a read is always followed
/// by another. The leaf syncs on
/// `#run-board` with the toolbar so the later read wins. It says `reload=columns`, so the server
/// leaves out the toolbar's options, which `hx-select` would discard.
pub fn run_board(view: &RunBoardView<'_>) -> String {
    let company_id = view.company_id;
    let windows: String = [BoardWindow::Day, BoardWindow::Week, BoardWindow::Month]
        .into_iter()
        .map(|window| {
            format!(
                r#"<option value="{value}"{selected}>Finished in {label}</option>"#,
                value = window.as_str(),
                selected = if window == view.window {
                    " selected"
                } else {
                    ""
                },
                label = window_label(window),
            )
        })
        .collect();
    let columns: String = view
        .columns
        .iter()
        .map(|column| board_column(view, column))
        .collect();
    format!(
        r##"<div id="run-board" class="ui-pane-list flex min-h-0 min-w-0 flex-1 flex-col transition-opacity aria-busy:opacity-60">
            <span hidden data-board-wake hx-get="/ui/workflow-runs" hx-vals='{{"reload": "columns"}}' hx-trigger="sse:runs, sse:resync, sse:tick, run-selected from:body, board-wake"
                hx-target="#run-board-columns" hx-select="#run-board-columns > section" hx-swap="innerHTML"
                hx-sync="#run-board:replace" hx-include="#run-selected, #run-board-filters"></span>
            <div class="flex flex-wrap items-center gap-3 border-b border-base-300 px-4 py-2">
                <h1 class="font-semibold">Runs</h1>
                {toggle}
                <form id="run-board-filters" class="flex flex-wrap items-center gap-2" hx-get="/ui/workflow-runs" hx-target="#run-board" hx-swap="outerHTML"
                    hx-trigger="change" hx-sync="#run-board:replace" hx-include="#run-selected" data-busy="#run-board">
                    <input type="hidden" name="company_id" value="{company_id}">
                    <input type="hidden" name="view" value="board">
                    <input type="hidden" name="part" value="columns">{scope_controls}
                    <select id="run-board-window" class="select select-sm w-auto" name="window" aria-label="Finished columns window">{windows}</select>
                </form>
                <a class="btn btn-ghost btn-xs ml-auto" href="/ui/workflows?company_id={company_id}">Workflows</a>
            </div>
            <div id="run-board-columns" class="flex min-h-0 flex-1 items-start gap-3 overflow-x-auto p-3">{columns}</div>
        </div>"##,
        scope_controls = scope_controls(company_id, view.scope, view.options, RunsView::Board),
        toggle = runs_view_toggle(company_id, view.scope, RunsView::Board),
    )
}

/// `#run-pane` with no run open on the board: a target for a card, empty for the phone layout.
const EMPTY_BOARD_PANE: &str =
    r#"<div id="run-pane" class="ui-pane-detail" data-pane-empty></div>"#;

pub struct RunsBoardPage<'a> {
    pub user: &'a MailboxUser<'a>,
    pub company: &'a Company,
    /// Pre-rendered `#run-board`.
    pub board_html: &'a str,
    /// Pre-rendered `#run-pane`, or `None`: no run open, and the board takes the full width.
    pub pane_html: Option<&'a str>,
}

/// The board workspace. On a wide window the pane's column shows only while it holds a run's pane
/// (a `<section>`); with none open it holds an empty `#run-pane` for a card to swap into, and
/// takes no width. On a phone the column steps aside (`display: contents`) and the shell shows the
/// board or the pane, whichever the reader is in.
pub fn runs_board_page(page: &RunsBoardPage<'_>) -> String {
    let company_id = page.company.id;
    let content = format!(
        r##"<main class="flex min-h-0 min-w-0 flex-1" hx-ext="sse" sse-connect="/ui/workflow-runs/events?company_id={company_id}">
            {board}
            <div class="hidden shrink-0 border-l border-base-300 max-md:contents md:w-[36rem] md:max-w-[50%] md:has-[>section]:flex">{pane}</div>
        </main>"##,
        board = page.board_html,
        pane = page.pane_html.unwrap_or(EMPTY_BOARD_PANE),
    );
    ui_shell(&UiShell {
        title: &format!("{} Runs", page.company.name),
        user: page.user,
        company: Some(page.company),
        section: UiSection::Runs,
        content: &content,
    })
}
