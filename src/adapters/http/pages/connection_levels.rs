//! Shared classification control; options reflect current authority, the write rechecks it.
use super::{escape_html_attr, escape_html_text};
use crate::application::workflow::http::HttpConnection;
use crate::entities::access_level::{AccessLevel, LevelActor, LevelTarget, may_assign_level};
use uuid::Uuid;

pub(crate) fn level_form(
    action: &str,
    revision: i64,
    current: AccessLevel,
    actor: LevelActor,
    draft: Option<i16>,
) -> String {
    let selected = draft.unwrap_or(current.as_i16());
    let attempted = draft
        .filter(|level| match AccessLevel::try_from(*level) {
            Ok(level) => may_assign_level(actor, LevelTarget::Assignable(current), level).is_err(),
            Err(_) => true,
        })
        .map(|level| format!("<p>Requested access level: {level}. Choose a permitted level.</p>"))
        .unwrap_or_default();
    if may_assign_level(actor, LevelTarget::Assignable(current), current).is_err() {
        return format!(
            "<p>Access level: {}. A teammate with a sufficient level can change it.</p>{attempted}",
            current.as_i16()
        );
    }
    let options = AccessLevel::ALL
        .into_iter()
        .filter(|level| may_assign_level(actor, LevelTarget::Assignable(current), *level).is_ok())
        .map(|level| {
            format!(
                "<option value=\"{}\"{}>{}</option>",
                level.as_i16(),
                if level.as_i16() == selected {
                    " selected"
                } else {
                    ""
                },
                level.as_i16()
            )
        })
        .collect::<String>();
    format!(
        r#"{attempted}<form method="post" action="{}" data-submit="busy-once" data-pending-label="Saving…" class="space-y-3"><h2 class="text-xl">Access level</h2><input type="hidden" name="expected_revision" value="{revision}"><label>Maximum content this connection may receive<select class="select" name="access_level">{options}</select></label><button class="btn btn-primary" type="submit">Save access level</button></form>"#,
        escape_html_attr(action)
    )
}

pub(crate) fn http_catalog(company: Uuid, connections: &[HttpConnection]) -> String {
    let rows=connections.iter().map(|c|format!(r#"<li><a class="link" href="/ui/companies/{company}/http-connections/{}">{}</a> · access level {}</li>"#,c.id,escape_html_text(c.origin.as_str()),c.access_level.as_i16())).collect::<String>();
    format!("<p>HTTP connections used by workflow actions.</p><ul>{rows}</ul>")
}

pub(crate) fn http_detail(
    connection: &HttpConnection,
    actor: LevelActor,
    draft: Option<i16>,
) -> String {
    let company = connection.company.as_uuid();
    let id = connection.id;
    let source = if connection.origin.as_str().starts_with("https://") {
        format!(
            "<a class=\"link\" href=\"/ui/companies/{company}/connection-credentials/http/{id}\">Choose credential source</a>"
        )
    } else {
        "<p>Vault credentials require an HTTPS destination.</p>".into()
    };
    format!(
        "<p>{}</p>{}{source}",
        escape_html_text(connection.origin.as_str()),
        level_form(
            &format!("/ui/companies/{company}/http-connections/{id}/access-level"),
            connection.revision,
            connection.access_level,
            actor,
            draft
        )
    )
}
