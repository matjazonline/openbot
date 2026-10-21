use super::*;

pub(super) fn text(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
pub(super) fn identifier() -> Value {
    json!({"type":"string","minLength":1,"maxLength":MAX_IDENTIFIER,"pattern":"^[A-Za-z0-9][A-Za-z0-9_.-]*$"})
}
pub(super) fn integer(max: usize) -> Value {
    json!({"type":"integer","minimum":1,"maximum":max})
}
pub(super) fn array(items: Value, min: usize, max: usize) -> Value {
    json!({"type":"array","items":items,"minItems":min,"maxItems":max})
}
pub(super) fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
pub(super) fn scope() -> Value {
    json!({"oneOf":[
        object(json!({"kind":{"const":"company"},"id":identifier()}), &["kind"]),
        object(json!({"kind":{"enum":["agent","user"]},"id":identifier()}), &["kind","id"])
    ]})
}
/// Explicit company-member reviewers: 1..16 unique `{"kind":"user","id":<uuid>}` items, the
/// 04.11 destination shape. There are no groups or external reviewers (05-D2 §4.1).
pub(super) fn reviewers() -> Value {
    let mut list = array(
        object(
            json!({"kind":{"const":"user"},"id":uuid()}),
            &["kind", "id"],
        ),
        1,
        MAX_REVIEWERS,
    );
    list["uniqueItems"] = json!(true);
    list
}
/// Code points are only a coarse bound: `InputCheck::Message` and `CanonicalContent::parse`
/// enforce the canonical 998-byte subject at compilation (literals) and at resolution.
pub(super) fn content() -> Value {
    let subject = json!({"type":"string","maxLength":crate::application::transport::ingress::MAX_SUBJECT_BYTES});
    object(json!({"subject":subject,"body":text(MAX_TEXT)}), &["body"])
}
pub(super) fn uuid() -> Value {
    json!({"type":"string","pattern":"^[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}$"})
}
/// One frozen thread-history message (06-D6 A). `audience` and `entry_kind` keep the
/// message's disclosure boundary as data; nothing beyond internal notes is dropped.
pub(super) fn history_item() -> Value {
    object(
        json!({"message_id":identifier(),"at":{"type":"string","format":"date-time","maxLength":MAX_DEADLINE},
            "role":{"enum":["human","agent","system"]},
            "audience":{"enum":["external_conversation","internal_only","legacy_unclassified"]},
            "entry_kind":{"enum":["conversation","note","delegation","system_event"]},
            "author":{"type":"string","maxLength":MAX_TEXT},"subject":{"type":"string","maxLength":MAX_TEXT},
            "body":{"type":"string","maxLength":MAX_TEXT},"truncated":{"type":"boolean"}}),
        &[
            "message_id",
            "at",
            "role",
            "audience",
            "entry_kind",
            "author",
            "subject",
            "body",
            "truncated",
        ],
    )
}
pub(super) fn provenance_item() -> Value {
    object(
        json!({"id":identifier(),"source":identifier(),"content":text(MAX_TEXT)}),
        &["id", "source", "content"],
    )
}
/// One entry per destination, in destination order. Only an all-accepted invocation succeeds;
/// a failed receipt fails the step and never reaches downstream references.
pub(super) fn message_result() -> Value {
    let destination = object(
        json!({"kind":{"enum":["user","channel","reply"]},"id":uuid()}),
        &["kind", "id"],
    );
    let entry = object(
        json!({"destination":destination,"message_id":uuid(),"status":{"enum":["accepted","failed"]}}),
        &["destination", "message_id", "status"],
    );
    object(
        json!({"status":{"enum":["accepted","failed"]},"messages":array(entry,1,MAX_MESSAGE_DESTINATIONS)}),
        &["status", "messages"],
    )
}
pub(super) fn headers() -> Value {
    json!({"type":"object","maxProperties":MAX_HEADERS,"propertyNames":{"type":"string","minLength":1,"maxLength":MAX_IDENTIFIER,"pattern":"^[!#$%&'*+.^_`|~0-9A-Za-z-]+$"},"additionalProperties":{"type":"string","maxLength":MAX_TEXT}})
}
