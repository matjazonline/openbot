//! The signed-in reader's actionable alerts. Labels are fixed taxonomy and current company/
//! channel names; source bodies and failure details never enter this view.

use super::*;
use crate::entities::notification::{NotificationPage, NotificationState};

pub struct NotificationsPageView<'a> {
    pub user: &'a MailboxUser<'a>,
    pub company: &'a Company,
    pub page: &'a NotificationPage,
}

pub fn notifications_page(view: &NotificationsPageView<'_>) -> String {
    let content = format!(
        r##"<main class="min-w-0 flex-1 overflow-y-auto bg-base-100"
            hx-ext="sse" sse-connect="/companies/{company_id}/notifications/events">
            <section class="mx-auto max-w-4xl p-6 lg:p-10">
                <header class="mb-6 flex items-end justify-between gap-4">
                    <div><h1 class="text-2xl font-bold">Notifications</h1>
                    <p class="mt-1 text-sm opacity-60">Alerts point to current operational work. Opening one never completes it.</p></div>
                    <span class="badge badge-primary badge-lg">{unread} unread</span>
                </header>
                <div id="notification-list" hx-get="/ui/notifications/list?company_id={company_id}"
                    hx-trigger="sse:reconcile" hx-swap="outerHTML">{list}</div>
            </section>
        </main>"##,
        company_id = view.company.id,
        unread = view.page.unread_count,
        list = notification_list(view.page),
    );
    ui_shell(&UiShell {
        title: "Notifications",
        user: view.user,
        company: Some(view.company),
        section: UiSection::Notifications,
        content: &content,
    })
}

pub fn notification_list(page: &NotificationPage) -> String {
    let rows = if page.items.is_empty() {
        r#"<div class="rounded-box border border-base-300 p-8 text-center text-sm opacity-60">No notifications yet.</div>"#.to_string()
    } else {
        page.items.iter().map(notification_row).collect()
    };
    format!(
        r#"<div id="notification-list" class="space-y-3" data-unread-count="{}">{rows}</div>"#,
        page.unread_count
    )
}

fn notification_row(
    notification: &crate::entities::notification::ActionableNotification,
) -> String {
    let unread = notification.read_at.is_none() && notification.state == NotificationState::Active;
    let state = match notification.state {
        NotificationState::Active => "Action needed",
        NotificationState::Resolved => "Resolved at source",
        NotificationState::Withdrawn => "No longer assigned",
    };
    let badge = match notification.state {
        NotificationState::Active => "badge-warning",
        NotificationState::Resolved => "badge-success",
        NotificationState::Withdrawn => "badge-ghost",
    };
    format!(
        r##"<article class="rounded-box border border-base-300 p-4 {unread_style}">
            <div class="flex items-start justify-between gap-4">
                <div class="min-w-0">
                    <form method="post" action="{href}">
                        <button type="submit" class="font-semibold hover:underline">{label}</button>
                    </form>
                    <p class="mt-1 truncate text-sm opacity-65">{company} / {channel}</p>
                    <p class="mt-2 text-xs opacity-50">{created}</p>
                </div>
                <span class="badge badge-sm {badge}">{state}</span>
            </div>
        </article>"##,
        unread_style = if unread {
            "bg-primary/5"
        } else {
            "bg-base-100"
        },
        href = escape_html_attr(&notification.href),
        label = escape_html_text(notification.action_kind.label()),
        company = escape_html_text(&notification.company_label),
        channel = escape_html_text(&notification.channel_label),
        created = escape_html_text(&format_date(notification.created_at)),
        state = escape_html_text(state),
    )
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::entities::notification::{
        ActionableNotification, NotificationActionKind, NotificationId, NotificationSourceKind,
    };

    #[test]
    fn notification_rows_render_only_safe_taxonomy_and_escaped_labels() {
        let item = ActionableNotification {
            id: NotificationId::random(),
            company_id: Uuid::new_v4(),
            company_label: "<company>".into(),
            channel_id: Uuid::new_v4(),
            channel_label: "support & sales".into(),
            source_kind: NotificationSourceKind::Task,
            source_id: Uuid::new_v4(),
            action_kind: NotificationActionKind::TaskFailure,
            source_generation: 1,
            state: NotificationState::Active,
            read_at: None,
            created_at: Utc::now(),
            state_changed_at: Utc::now(),
            href: "/ui/notifications/\"unsafe/open".into(),
        };
        let html = notification_list(&NotificationPage {
            items: vec![item],
            unread_count: 1,
        });
        assert!(!html.contains("<company>"));
        assert!(html.contains("&lt;company&gt;"));
        assert!(html.contains("support &amp; sales"));
        assert!(html.contains("&quot;unsafe"));
    }
}
