//! Runtime argument bounds before any intent or marker (04.11-D2 §1.1, F003).
use super::*;
use serde_json::json;

fn send(content: Value, destinations: Value) -> AppResult<MessageArguments> {
    MessageArguments::parse(
        MessageKind::Send,
        &json!({"content":content,"destinations":destinations}),
    )
}
fn users(count: usize) -> Value {
    json!(
        (0..count)
            .map(|i| json!({"kind":"user","id":Uuid::from_u128(i as u128 + 1)}))
            .collect::<Vec<_>>()
    )
}

#[test]
fn message_registry_runtime_arguments_refuse_out_of_bound_inputs() {
    let body = json!({"subject":"Status","body":"hello"});
    send(body.clone(), users(20)).unwrap();
    assert!(send(body.clone(), users(21)).is_err());
    assert!(send(body.clone(), users(0)).is_err());
    let one = Uuid::from_u128(7);
    assert!(
        send(
            body.clone(),
            json!([{"kind":"user","id":one},{"kind":"user","id":one}])
        )
        .is_err()
    );
    assert!(send(body.clone(), json!([{"kind":"group","id":one}])).is_err());
    // Same id as a user and as a channel names two different destinations.
    send(
        body.clone(),
        json!([{"kind":"user","id":one},{"kind":"channel","id":one}]),
    )
    .unwrap();
    // A body that is empty once sanitized is refused; whitespace is not text.
    assert!(send(json!({"body":"   \n\t"}), users(1)).is_err());
    // Subjects are bounded in bytes: 998 bytes pass, 999 and 998 code points of '€' do not.
    let exact = format!("{}é", "€".repeat(332));
    send(json!({"subject":exact,"body":"hello"}), users(1)).unwrap();
    assert!(
        send(
            json!({"subject":format!("{exact}a"),"body":"hello"}),
            users(1)
        )
        .is_err()
    );
    assert!(send(json!({"subject":"€".repeat(998),"body":"hello"}), users(1)).is_err());
    // A reply names exactly one source message.
    assert!(
        MessageArguments::parse(
            MessageKind::Reply,
            &json!({"content":body,"source_message":"not-a-uuid"})
        )
        .is_err()
    );
}

#[test]
fn message_registry_trusted_snapshot_is_exact() {
    let company = CompanyId::new(Uuid::from_u128(9));
    let mut snapshot = builtin_message_snapshot(company, MessageKind::Send).unwrap();
    assert!(is_trusted_message_snapshot(&snapshot).unwrap());
    snapshot.policy.policy_revision = 2;
    assert!(!is_trusted_message_snapshot(&snapshot).unwrap());
    let mut widened = builtin_message_snapshot(company, MessageKind::Reply).unwrap();
    widened.contract.output_schema = json!(true);
    assert!(!is_trusted_message_snapshot(&widened).unwrap());
    assert!(is_builtin_message_tool("message.send") && is_builtin_message_tool("message.reply"));
    assert!(!is_builtin_message_tool("message.group"));
}
