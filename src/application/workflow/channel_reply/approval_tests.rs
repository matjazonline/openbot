use super::*;

#[test]
fn channel_reply_completion_policy_roundtrips_only_static_user_reviewers() {
    let policy = ReplyApprovalPolicy::new(vec![Uuid::new_v4()], 60).unwrap();
    let wire = serde_json::to_value(&policy).unwrap();
    assert_eq!(
        serde_json::from_value::<ReplyApprovalPolicy>(wire.clone()).unwrap(),
        policy
    );
    for edit in [
        json!({"reviewers":[{"kind":"agent","id":Uuid::new_v4()}],"review_timeout_seconds":60}),
        json!({"reviewers":[{"kind":"user","id":Uuid::new_v4()}],"deadline":"2099-01-01T00:00:00Z"}),
        json!({"reviewers":[],"review_timeout_seconds":60}),
        json!({"reviewers":[{"kind":"user","id":Uuid::nil()}],"review_timeout_seconds":60}),
    ] {
        assert!(serde_json::from_value::<ReplyApprovalPolicy>(edit).is_err());
    }
}

#[test]
fn channel_reply_completion_policy_enforces_review_and_duration_bounds() {
    let user = Uuid::new_v4();
    assert!(ReplyApprovalPolicy::new(vec![user, user], 60).is_err());
    assert!(ReplyApprovalPolicy::new((0..17).map(|_| Uuid::new_v4()).collect(), 60).is_err());
    for seconds in [0, 86_401, u32::MAX] {
        assert!(ReplyApprovalPolicy::new(vec![user], seconds).is_err());
    }
    for seconds in [1, 86_400] {
        ReplyApprovalPolicy::new(vec![user], seconds).unwrap();
    }
}

#[test]
fn channel_reply_completion_policy_materializes_relative_to_each_response_and_caps_run() {
    let policy = ReplyApprovalPolicy::new(vec![Uuid::new_v4()], 60).unwrap();
    let now = DateTime::parse_from_rfc3339("2030-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let run_deadline = now + chrono::Duration::seconds(30);
    let spec = policy.materialize(now, run_deadline).unwrap();
    let (_, deadline) = crate::application::workflow::decisions::reviewer_deadline(&spec).unwrap();
    assert_eq!(deadline, run_deadline);
    let later = now + chrono::Duration::days(365);
    let later_spec = policy
        .materialize(later, later + chrono::Duration::hours(1))
        .unwrap();
    let (_, later_deadline) =
        crate::application::workflow::decisions::reviewer_deadline(&later_spec).unwrap();
    assert_eq!(later_deadline, later + chrono::Duration::seconds(60));
    assert_eq!(spec["deadline"], run_deadline.to_rfc3339());
    assert!(policy.materialize(now, now).is_err());
}
