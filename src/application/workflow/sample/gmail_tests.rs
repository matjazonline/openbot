use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

fn gmail_fixture(name: &str) -> Value {
    serde_json::from_str(match name {
        "read" => include_str!("../../../../samples/gmail-read-prepare.json"),
        "send" => include_str!("../../../../samples/gmail-reviewed-send.json"),
        _ => unreachable!(),
    })
    .unwrap()
}

#[test]
fn vault_authoring_gmail_examples_compile_and_execute_only_scripted_effects() {
    for name in ["read", "send"] {
        let fixture = gmail_fixture(name);
        let facts = if name == "read" {
            let mut facts = Fixture::AutonomousResponse.snapshots(company());
            facts.tools.clear();
            facts
        } else {
            DependencySnapshots::default()
        };
        let bundle = build(&fixture["source"], facts, vec![]);
        let report = execute(
            bundle.clone(),
            serde_json::from_value(fixture["sample"].clone()).unwrap(),
        )
        .unwrap();
        complete(&report);
        assert_eq!(report.usage.model_calls, u32::from(name == "read"));
        assert_eq!(report.effects.len(), 1);
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("access_token") && !serialized.contains("refresh_token"));
        if name == "read" {
            assert!(
                report
                    .output
                    .as_ref()
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .contains("review")
            );
        } else {
            assert_eq!(
                report.output.as_ref().unwrap()["body"]["id"],
                "sent-fixture"
            );
        }
        let mut unscripted: SampleRequest =
            serde_json::from_value(fixture["sample"].clone()).unwrap();
        unscripted.scripts.clear();
        assert_eq!(
            execute(bundle, unscripted).unwrap().status,
            SampleStatus::Stopped
        );
    }
}

#[test]
fn vault_authoring_reviewed_mime_is_valid_base64url_and_keeps_reply_headers() {
    let fixture = gmail_fixture("send");
    let raw = fixture["sample"]["input"]["raw"].as_str().unwrap();
    let bytes = URL_SAFE_NO_PAD.decode(raw).unwrap();
    assert_eq!(URL_SAFE_NO_PAD.encode(&bytes), raw);
    let mime = String::from_utf8(bytes).unwrap();
    for header in [
        "From: author@example.test\r\n",
        "To: recipient@example.test\r\n",
        "Subject: Re: Meeting\r\n",
        "Message-ID: <reply-fixture@example.test>\r\n",
        "In-Reply-To: <original-fixture@example.test>\r\n",
        "References: <original-fixture@example.test>\r\n",
    ] {
        assert!(mime.contains(header));
    }
    assert!(mime.contains("\r\n\r\nThe meeting is confirmed.\r\n"));
    assert_eq!(fixture["sample"]["input"]["thread_id"], "fixture-thread");
}
