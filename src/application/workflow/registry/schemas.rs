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
pub(super) fn selector() -> Value {
    object(
        json!({"kind":{"enum":["user","group"]},"id":identifier()}),
        &["kind", "id"],
    )
}
pub(super) fn profile(facts: &CatalogueFacts) -> Value {
    let mut list = array(identifier(), 0, MAX_SELECTIONS);
    list["uniqueItems"] = json!(true);
    let inline = object(json!({"tools":list,"skills":list}), &["tools", "skills"]);
    if facts.profiles.is_empty() {
        inline
    } else {
        let mut names: Vec<_> = facts.profiles.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        json!({"oneOf":[inline,{"type":"string","enum":names}]})
    }
}
pub(super) fn content() -> Value {
    object(
        json!({"subject":text(MAX_TEXT),"body":text(MAX_TEXT)}),
        &["body"],
    )
}
pub(super) fn provenance_item() -> Value {
    object(
        json!({"id":identifier(),"source":identifier(),"content":text(MAX_TEXT)}),
        &["id", "source", "content"],
    )
}
pub(super) fn message_result() -> Value {
    object(
        json!({"message_id":identifier(),"status":{"enum":["accepted"]}}),
        &["message_id", "status"],
    )
}
pub(super) fn headers() -> Value {
    json!({"type":"object","maxProperties":MAX_HEADERS,"propertyNames":{"type":"string","minLength":1,"maxLength":MAX_IDENTIFIER,"pattern":"^[!#$%&'*+.^_`|~0-9A-Za-z-]+$"},"additionalProperties":{"type":"string","maxLength":MAX_TEXT}})
}
