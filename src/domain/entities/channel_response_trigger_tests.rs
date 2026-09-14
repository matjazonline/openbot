use super::*;

#[test]
fn response_trigger_decision_table() {
    for trigger in [
        ChannelResponseTrigger::Always,
        ChannelResponseTrigger::Mentioned,
        ChannelResponseTrigger::MentionedOrReplyToAgent,
    ] {
        for role in [RecipientRole::To, RecipientRole::Cc] {
            // Columns: neither, direct reply only, mention only, both.
            let expected = match (trigger, role) {
                (ChannelResponseTrigger::Always, RecipientRole::To) => [true; 4],
                (ChannelResponseTrigger::Always, RecipientRole::Cc)
                | (ChannelResponseTrigger::Mentioned, _) => [false, false, true, true],
                (ChannelResponseTrigger::MentionedOrReplyToAgent, _) => [false, true, true, true],
            };
            for ((mentioned, replies_to_agent), expected) in
                [(false, false), (false, true), (true, false), (true, true)]
                    .into_iter()
                    .zip(expected)
            {
                assert_eq!(
                    trigger.answers(AnswerFacts {
                        role,
                        mentioned,
                        replies_to_agent
                    }),
                    expected,
                    "{trigger:?} {role:?}, mentioned={mentioned}, reply={replies_to_agent}"
                );
            }
        }
    }
}

#[test]
fn response_trigger_wire_values_are_exact_and_required() {
    for (trigger, wire) in [
        (ChannelResponseTrigger::Always, "always"),
        (ChannelResponseTrigger::Mentioned, "mentioned"),
        (
            ChannelResponseTrigger::MentionedOrReplyToAgent,
            "mentioned_or_reply",
        ),
    ] {
        assert_eq!(trigger.as_str(), wire);
        assert_eq!(wire.parse::<ChannelResponseTrigger>().unwrap(), trigger);
        assert_eq!(serde_json::to_value(trigger).unwrap(), wire);
        assert_eq!(
            serde_json::from_value::<ChannelResponseTrigger>(serde_json::json!(wire)).unwrap(),
            trigger
        );
    }
    for invalid in ["unknown", "", "Always", "mentioned_or_reply_to_agent"] {
        assert!(invalid.parse::<ChannelResponseTrigger>().is_err());
        assert!(
            serde_json::from_value::<ChannelResponseTrigger>(serde_json::json!(invalid)).is_err()
        );
    }
    let error = serde_json::from_value::<Channel>(serde_json::json!({
        "id": Uuid::new_v4(), "company_id": Uuid::new_v4(), "name": "Support",
        "slug": "support", "participant_emails": null, "access_mode": "team",
        "principal_grants": [], "agent_ids": null,
        "created_by": CreationProvenance::system(), "created_at": chrono::Utc::now()
    }))
    .unwrap_err();
    assert!(error.to_string().contains("response_trigger"));
}
