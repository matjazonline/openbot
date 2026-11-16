//! Target-preserving URLs and form fields shared by all mailbox surfaces.
use crate::entities::address::AddressTarget;

pub fn inbox_target_query(target: AddressTarget) -> String {
    match target {
        AddressTarget::Desk(id) => format!("channel_id={id}"),
        AddressTarget::Principal(id) => format!("principal_id={id}"),
    }
}

pub fn inbox_target_input(target: AddressTarget) -> String {
    let (name, id) = match target {
        AddressTarget::Desk(id) => ("channel_id", id),
        AddressTarget::Principal(id) => ("principal_id", id.as_uuid()),
    };
    format!(r#"<input type="hidden" name="{name}" value="{id}">"#)
}

use super::*;

pub(super) fn sidebar_items(page: &MailboxPage<'_>) -> String {
    let mut html = String::new();
    for (title, direct) in [("Desks", false), ("Direct", true)] {
        html.push_str(&format!(r#"<li class="menu-title">{title}</li>"#));
        for inbox in page
            .inboxes
            .iter()
            .filter(|inbox| inbox.desk().is_none() == direct)
        {
            let address = inbox.slug().map(|slug| {
                crate::entities::address::address_for(
                    slug,
                    &page.company.slug,
                    page.app_domain_name,
                )
            });
            html.push_str(&channel_menu_item(
                page.company.id,
                inbox,
                address.as_ref(),
                page.selected_inbox
                    .is_some_and(|selected| selected.target() == inbox.target()),
            ));
        }
    }
    if page.direct_truncated {
        html.push_str(
            r#"<li class="px-2 text-xs opacity-60">Showing the first 200 direct inboxes.</li>"#,
        );
    }
    html
}

pub(super) fn new_message_button(pane: &MessagePane<'_>) -> String {
    if pane.inbox.slug().is_none() {
        return String::new();
    }
    format!(
        r##"<button type="button" class="btn btn-primary btn-sm" title="Send another message in this thread"
        hx-get="/ui/reply?company_id={company_id}&{target_query}&thread_id={thread_id}"
        hx-target="#detail-pane" hx-swap="outerHTML" hx-sync="#detail-pane:replace">{pencil} New Message</button>"##,
        company_id = pane.company_id,
        target_query = inbox_target_query(pane.inbox.target()),
        thread_id = pane.thread.id,
        pencil = icon(Icon::Pencil, BUTTON_ICON)
    )
}
