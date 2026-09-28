//! Bounded MCP 2025-11-25 content contracts; URIs remain inert data.
//! https://modelcontextprotocol.io/specification/2025-11-25/schema#content
use super::*;

fn string(max: usize) -> Value {
    json!({"type":"string","maxLength":max})
}
fn base64() -> Value {
    json!({"type":"string","maxLength":MAX_TEXT,"pattern":"^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$"})
}
pub(super) fn metadata() -> Value {
    // Arbitrary protocol extension values still pass the aggregate runtime JSON budget.
    json!({"type":"object","maxProperties":MAX_HEADERS,"propertyNames":{"maxLength":MAX_IDENTIFIER}})
}
fn annotations() -> Value {
    object(
        json!({
            "audience":array(json!({"enum":["user","assistant"]}),0,2),
            "priority":{"type":"number","minimum":0,"maximum":1},
            "lastModified":{"type":"string","format":"date-time","maxLength":MAX_DEADLINE}
        }),
        &[],
    )
}
fn block(properties: Value, required: &[&str]) -> Value {
    let mut schema = object(properties, required);
    schema["properties"]["annotations"] = annotations();
    schema["properties"]["_meta"] = metadata();
    schema
}
fn resource(field: &str, payload: Value) -> Value {
    let mut properties =
        json!({"uri":text(MAX_PATH),"mimeType":string(MAX_IDENTIFIER),"_meta":metadata()});
    properties[field] = payload;
    object(properties, &["uri", field])
}
fn icons() -> Value {
    array(
        object(
            json!({
                "src":text(MAX_TEXT), "mimeType":string(MAX_IDENTIFIER),
                "sizes":array(string(MAX_IDENTIFIER),0,MAX_SELECTIONS),
                "theme":{"enum":["light","dark"]}
            }),
            &["src"],
        ),
        0,
        MAX_SELECTIONS,
    )
}
pub(super) fn content() -> Value {
    array(
        json!({"oneOf":[
            block(json!({"type":{"const":"text"},"text":string(MAX_TEXT)}), &["type","text"]),
            block(json!({"type":{"enum":["image","audio"]},"data":base64(),"mimeType":text(MAX_IDENTIFIER)}), &["type","data","mimeType"]),
            block(json!({"type":{"const":"resource"},"resource":{"oneOf":[resource("text",string(MAX_TEXT)),resource("blob",base64())]}}), &["type","resource"]),
            block(json!({"type":{"const":"resource_link"},"icons":icons(),"uri":text(MAX_PATH),"name":string(MAX_IDENTIFIER),"title":string(MAX_TEXT),"description":string(MAX_TEXT),"mimeType":string(MAX_IDENTIFIER),"size":{"type":"number","minimum":0}}), &["type","uri","name"])
        ]}),
        0,
        MAX_ITEMS,
    )
}
