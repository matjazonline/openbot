//! Restricted messages retain their stable position, author and time, without content sinks.
use crate::entities::message_view::ThreadMessageView;

pub(crate) fn restricted_message(message: &ThreadMessageView) -> String {
    let author = message
        .author
        .handle
        .as_deref()
        .unwrap_or_else(|| message.author.display());
    format!(
        r#"<div class="chat chat-start" data-message-id="{id}" data-role="{role}" data-restricted-message>
        <div class="chat-header gap-1 opacity-70">{author}<time class="text-xs opacity-60">{time}</time></div>
        <div class="chat-bubble max-w-2xl text-sm opacity-70">Restricted message</div>
        </div>"#,
        id = message.canonical_id.as_uuid(),
        role = if message.is_agent() { "agent" } else { "human" },
        author = super::escape_html_text(author),
        time = super::format_date_time(message.created_at)
    )
}
