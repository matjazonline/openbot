use super::*;
use crate::application::thread_activity::{ThreadRunActivity, ThreadRuns};

#[test]
fn conversations_identify_each_run_and_its_own_reply() {
    let company = mailbox_company();
    let channel = mailbox_channel(company.id);
    let thread = mailbox_thread(channel.id);
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let mut runs = ThreadRuns {
        inspectable: Default::default(),
        items: vec![
            ThreadRunActivity {
                run_id: first,
                title: "<img src=x onerror=alert(1)>".into(),
                activity: ThreadActivity::Working,
            },
            ThreadRunActivity {
                run_id: second,
                title: "Review".into(),
                activity: ThreadActivity::AwaitingReview,
            },
        ],
        has_more: true,
        message_runs: vec![],
    };
    let mut inbound = mailbox_message(thread.id, "Question");
    inbound.run_ids = vec![first, second];
    let mut reply_one = mailbox_message(thread.id, "First answer");
    reply_one.direction = MessageDirection::Outbound;
    reply_one.run_ids = vec![first];
    let mut reply_two = reply_one.clone();
    reply_two.canonical_id = crate::entities::message::CanonicalMessageId::new(Uuid::new_v4());
    reply_two.body = "Second answer".into();
    reply_two.run_ids = vec![second];
    let messages = [inbound, reply_one, reply_two];
    for membership in [
        CompanyMembership::Owner,
        CompanyMembership::Admin,
        CompanyMembership::Member,
    ] {
        runs.inspectable = if membership.manages_company_operations() {
            [first, second].into_iter().collect()
        } else {
            Default::default()
        };
        let html = message_pane(&MessagePane {
            company_id: company.id,
            channel: &channel,
            thread: &thread,
            messages: &messages,
            viewer_email: &mailbox_account_email(),
            activity: &runs,
            error: None,
            viewer_membership: membership,
        });
        assert!(html.contains(&format!("data-run-id=\"{first}\"")));
        assert!(html.contains(&format!("data-run-id=\"{second}\"")));
        assert!(html.contains("Agent working") && html.contains("Awaiting review"));
        assert!(html.contains("&lt;img src=x") && !html.contains("<img src=x"));
        assert!(html.contains("Older runs are not shown"));
        assert_eq!(
            html.contains("href=\"/ui/workflow-runs?"),
            membership.manages_company_operations()
        );
        for (index, expected, other) in [(1, first, second), (2, second, first)] {
            let attribution = message_run_attribution(
                &messages[index],
                company.id,
                RunLinkAccess::Authorized(&runs.inspectable),
            );
            assert!(attribution.contains("Written by"));
            assert!(attribution.contains(&expected.to_string()));
            assert!(!attribution.contains(&other.to_string()));
        }
    }
}

#[test]
fn a_visible_reply_does_not_offer_a_link_to_an_inaccessible_producer() {
    let thread = Uuid::new_v4();
    let run = Uuid::new_v4();
    let mut reply = mailbox_message(thread, "Answer from another channel");
    reply.direction = MessageDirection::Outbound;
    reply.run_ids = vec![run];
    let allowed = Default::default();
    let html = message_run_attribution(&reply, Uuid::new_v4(), RunLinkAccess::Authorized(&allowed));
    assert!(html.contains("Written by"));
    assert!(html.contains(&run.to_string()));
    assert!(!html.contains("href="));
}
