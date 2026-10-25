use super::*;

use crate::entities::attention::{
    AttentionItem, AttentionPage, AttentionSourceKind, AttentionView, BusinessPriority,
};

pub struct AttentionPageView<'a> {
    pub user: &'a MailboxUser<'a>,
    pub companies: &'a [Company],
    pub company: &'a Company,
    pub view: AttentionView,
    pub page: &'a AttentionPage,
    pub manager: bool,
}

pub fn attention_page(page: &AttentionPageView<'_>) -> String {
    let company_id = page.company.id;
    let selected = view_name(page.view);
    let team_tab = if page.manager {
        tab(company_id, AttentionView::TeamWork, page.view)
    } else {
        String::new()
    };
    let company_options = page
        .companies
        .iter()
        .map(|company| {
            format!(
                r#"<option value="{}"{}>{}</option>"#,
                company.id,
                if company.id == company_id {
                    " selected"
                } else {
                    ""
                },
                escape_html_text(&company.name)
            )
        })
        .collect::<String>();
    let list_url = format!("/ui/work/list?company_id={company_id}&view={selected}");
    let freshness = if page.view == AttentionView::TeamWork {
        r#"<p class="px-5 pt-4 text-xs opacity-60">Binding refusals are updated within 30 s; everything else as it changes.</p>"#
    } else {
        ""
    };
    let content = format!(
        r##"<section class="flex min-w-0 flex-1 flex-col overflow-y-auto bg-base-100"
                hx-ext="sse" sse-connect="/companies/{company_id}/attention/events">
            <header class="flex flex-wrap items-center justify-between gap-4 border-b border-base-300 p-5">
                <div>
                    <h1 class="text-xl font-semibold">Operational work</h1>
                    <p class="text-sm opacity-65">Decisions to make and workflow problems to look at.</p>
                </div>
                <form method="get" action="/ui/work">
                    <input type="hidden" name="view" value="{selected}">
                    <div class="join">
                        <select class="select select-sm join-item" name="company_id"
                                aria-label="Company">{company_options}</select>
                        <button class="btn btn-sm join-item" type="submit">Switch</button>
                    </div>
                </form>
            </header>
            <nav class="tabs tabs-border px-5 pt-3" aria-label="Work queue views">
                {my_tab}{unassigned_tab}{team_tab}
            </nav>
            {freshness}
            <div id="attention-list" hx-get="{list_url}" hx-trigger="sse:reconcile"
                 hx-sync="#attention-list:replace">{list}</div>
        </section>"##,
        my_tab = tab(company_id, AttentionView::MyWork, page.view),
        unassigned_tab = tab(company_id, AttentionView::Unassigned, page.view),
        list = attention_list(company_id, page.view, page.page),
    );
    ui_shell(&UiShell {
        title: "Operational work",
        user: page.user,
        company: Some(page.company),
        section: UiSection::Work,
        content: &content,
    })
}

fn tab(company_id: Uuid, target: AttentionView, selected: AttentionView) -> String {
    let name = view_name(target);
    format!(
        r#"<a class="tab{}" href="/ui/work?company_id={company_id}&amp;view={name}">{}</a>"#,
        if target == selected {
            " tab-active"
        } else {
            ""
        },
        view_label(target)
    )
}

pub fn attention_list(company_id: Uuid, view: AttentionView, page: &AttentionPage) -> String {
    let warning = if page.truncated {
        r#"<div class="alert alert-warning mx-5 mt-4 text-sm">The working set reached 1,000 items. Narrow or resolve work before relying on totals.</div>"#
    } else {
        ""
    };
    let body = if page.items.is_empty() {
        r#"<div class="p-10 text-center text-sm opacity-60">No work items in this view.</div>"#
            .to_string()
    } else {
        page.items
            .iter()
            .map(|item| attention_card(item, page.as_of))
            .collect::<String>()
    };
    let next = page.next_cursor.as_ref().map_or_else(String::new, |cursor| {
        format!(
            r#"<div class="p-5 text-center"><a class="btn btn-sm" href="/ui/work?company_id={company_id}&amp;view={}&amp;cursor={}">Next page</a></div>"#,
            view_name(view), escape_html_attr(cursor)
        )
    });
    format!(r#"{warning}<div class="grid gap-3 p-5">{body}</div>{next}"#)
}

fn attention_card(item: &AttentionItem, as_of: chrono::DateTime<chrono::Utc>) -> String {
    let link = item.href.as_deref().map_or_else(String::new, |href| {
        format!(
            r#"<a class="link link-primary font-medium" href="{}">Open source</a>"#,
            escape_html_attr(href)
        )
    });
    let due = due_label(item.due_at);
    // The work, not the table: this badge is read by somebody deciding whether to pick the item
    // up.
    let source = match item.source_kind {
        AttentionSourceKind::Decision => "Decision",
        AttentionSourceKind::RunReconciliation => "Needs reconciliation",
        AttentionSourceKind::RunFailure => "Failed run",
        AttentionSourceKind::BindingRefusal => "Refused messages",
        AttentionSourceKind::DeliveryFailure => "Delivery failure",
    };
    let expiry = item.expires_at.map_or_else(String::new, |expires| {
        format!(" · Expires {}", expires.format("%Y-%m-%d %H:%M UTC"))
    });
    let age = age_label(item.created_at, as_of);
    format!(
        r#"<article class="card border border-base-300 bg-base-200 shadow-sm">
            <div class="card-body gap-2 p-4">
                <div class="flex flex-wrap items-center gap-2 text-xs">
                    <span class="badge badge-outline">{source}</span>
                    <span class="badge {priority_class}">{priority}</span>
                    <span class="opacity-60">{state}</span>
                </div>
                <h2 class="font-semibold">{title}</h2>
                <p class="text-sm">{action}</p>
                <div class="flex flex-wrap items-center justify-between gap-3 text-xs opacity-70">
                    <span>{responsible} · {age} · {due}{expiry}</span>
                    {link}
                </div>
            </div>
        </article>"#,
        priority_class = priority_class(item.priority),
        priority = item.priority,
        state = escape_html_text(&item.state),
        title = escape_html_text(&item.title),
        action = escape_html_text(&item.next_action),
        responsible = escape_html_text(&item.responsibility_label),
        age = escape_html_text(&age),
        due = escape_html_text(&due),
        expiry = escape_html_text(&expiry),
    )
}

/// When an item is due, in the queue's own words.
fn due_label(due_at: Option<chrono::DateTime<chrono::Utc>>) -> String {
    due_at.map_or_else(
        || "No due time".to_string(),
        |due| format!("Due {}", due.format("%Y-%m-%d %H:%M UTC")),
    )
}

/// How long an item has waited, in the queue's own words.
fn age_label(
    created_at: chrono::DateTime<chrono::Utc>,
    as_of: chrono::DateTime<chrono::Utc>,
) -> String {
    let seconds = (as_of - created_at).num_seconds().max(0);
    match seconds {
        0..=59 => "Age <1m".into(),
        60..=3_599 => format!("Age {}m", seconds / 60),
        3_600..=86_399 => format!("Age {}h", seconds / 3_600),
        _ => format!("Age {}d", seconds / 86_400),
    }
}

pub(crate) const fn priority_class(priority: BusinessPriority) -> &'static str {
    match priority {
        BusinessPriority::Normal => "badge-ghost",
        BusinessPriority::High => "badge-warning",
        BusinessPriority::Urgent => "badge-error",
    }
}

const fn view_name(view: AttentionView) -> &'static str {
    match view {
        AttentionView::MyWork => "my_work",
        AttentionView::Unassigned => "unassigned",
        AttentionView::TeamWork => "team_work",
    }
}

const fn view_label(view: AttentionView) -> &'static str {
    match view {
        AttentionView::MyWork => "My work",
        AttentionView::Unassigned => "Unassigned",
        AttentionView::TeamWork => "Team work",
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::entities::{attention::AttentionResponsibility, transport::PrincipalId};

    fn item(source_kind: AttentionSourceKind, href: Option<String>) -> AttentionItem {
        let now = Utc::now();
        AttentionItem {
            source_kind,
            source_id: Uuid::new_v4(),
            company_id: Uuid::new_v4(),
            channel_id: Some(Uuid::new_v4()),
            thread_id: None,
            run_id: Some(Uuid::new_v4()),
            correlation_id: None,
            state: "open<script>".into(),
            responsibility: AttentionResponsibility::Principal(PrincipalId::new(Uuid::new_v4())),
            responsibility_label: "Casey & <team>".into(),
            title: "<img src=x onerror=alert(1)>".into(),
            next_action: "Call \"now\" & confirm".into(),
            priority: BusinessPriority::Urgent,
            due_at: None,
            expires_at: None,
            version: 1,
            created_at: now,
            updated_at: now,
            href,
        }
    }

    fn page(items: Vec<AttentionItem>, next_cursor: Option<String>) -> AttentionPage {
        AttentionPage {
            items,
            next_cursor,
            as_of: Utc::now(),
            working_set_size: 1,
            truncated: false,
        }
    }

    #[test]
    fn list_escapes_source_text_and_links() {
        let item = item(
            AttentionSourceKind::Decision,
            Some("/ui?x=\"&y=<unsafe>".into()),
        );
        let rendered = attention_list(
            item.company_id,
            AttentionView::MyWork,
            &page(vec![item], Some("cursor&next".into())),
        );

        assert!(!rendered.contains("<script>"));
        assert!(!rendered.contains("<img"));
        assert!(!rendered.contains("<unsafe>"));
        assert!(rendered.contains("Casey &amp; &lt;team&gt;"));
        assert!(rendered.contains("cursor&amp;next"));
        assert!(rendered.contains("href=\"/ui?x=&quot;&amp;y=&lt;unsafe&gt;\""));
        assert!(rendered.contains(">Decision<"));
    }

    /// The list subscribes to the attention stream and re-reads on every reconcile, and a newer
    /// read replaces one still in flight, so two reads finishing out of order keep the latest.
    #[test]
    fn the_page_reconciles_its_list_live_and_in_order() {
        let company = Company {
            channel_defaults: Default::default(),
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            name: "Acme".into(),
            slug: "acme".into(),
            avatar_url: None,
            created_at: Utc::now(),
        };
        let email = crate::entities::value_objects::EmailAddress::from("owner@example.com");
        let user = MailboxUser {
            id: company.user_id,
            username: "owner",
            email: &email,
            avatar_url: None,
            is_operator: false,
            company_membership: crate::entities::company_member::CompanyMembership::Owner,
        };
        let html = attention_page(&AttentionPageView {
            user: &user,
            companies: std::slice::from_ref(&company),
            company: &company,
            view: AttentionView::TeamWork,
            page: &page(Vec::new(), None),
            manager: true,
        });
        assert!(html.contains(&format!(
            r#"sse-connect="/companies/{}/attention/events""#,
            company.id
        )));
        assert!(html.contains(r#"hx-trigger="sse:reconcile""#));
        assert!(html.contains(r##"hx-sync="#attention-list:replace""##));
        assert!(html.contains("Binding refusals are updated within 30 s"));
        assert!(!html.contains("all_owned"));
    }

    /// A run has no page until p09, so its card offers no link rather than a dead one.
    #[test]
    fn a_source_without_a_page_has_no_link() {
        let failed = item(AttentionSourceKind::RunFailure, None);
        let rendered = attention_list(
            failed.company_id,
            AttentionView::TeamWork,
            &page(vec![failed], None),
        );
        assert!(rendered.contains("Failed run"));
        assert!(!rendered.contains("Open source"));
    }
}
