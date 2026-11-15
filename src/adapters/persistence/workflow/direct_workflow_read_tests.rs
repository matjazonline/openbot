//! Real manual admission on a personal target; the terminal-state write below is a read-model
//! fixture, not proof of the workflow failure producer.
use super::*;
use crate::application::use_cases::company_invite::CompanyInvitePersistence;
use crate::application::{attention::AttentionPersistence, run_counts::RunCountsReader};
use crate::entities::{
    address::AddressTarget,
    attention::{AttentionQuery, AttentionSourceKind, AttentionView},
    transport::PrincipalId,
};

struct PrivateWork {
    f: AdmissionFixture,
    user: Uuid,
    teammate: Uuid,
    principal: PrincipalId,
    run: RunId,
}

impl PrivateWork {
    async fn new() -> Self {
        let f = AdmissionFixture::new().await;
        let user = super::super::message_tests::support::member(&f.binding.fixture, "member").await;
        let teammate =
            super::super::message_tests::support::member(&f.binding.fixture, "admin").await;
        let principal = PrincipalId::new(f.binding.fixture.principal(user).await);
        let mut request = f.request("private-work", f.manual());
        request.association = RelatedAssociation::Principal(principal);
        let command = f.prepare(request).await;
        let AdmissionResult::Created(run) = f.persistence().admit(&command).await.unwrap() else {
            panic!("the owner can start a workflow in a person's private inbox");
        };
        Self {
            f,
            user,
            teammate,
            principal,
            run,
        }
    }

    fn company(&self) -> Uuid {
        self.f.binding.target.company.as_uuid()
    }

    async fn query(&self, user: Uuid) -> AttentionQuery<'static> {
        AttentionQuery {
            company_id: self.company(),
            principal_id: PrincipalId::new(self.f.binding.fixture.principal(user).await),
            visible_channel_ids: &[],
            manager: true,
            company_items: true,
            direct_items: true,
            view: AttentionView::TeamWork,
            cursor: None,
            limit: 100,
        }
    }

    async fn assert_attention(&self, user: Uuid, expected: usize) {
        let query = self.query(user).await;
        let summary = self
            .f
            .persistence()
            .operational_summary(self.company(), &[], query.principal_id)
            .await
            .unwrap();
        let page = self.f.persistence().list_attention(query).await.unwrap();
        assert_eq!(page.items.len(), expected);
        assert_eq!(summary.failed_run_count, expected as i64);
        for item in page.items {
            assert_eq!(item.source_kind, AttentionSourceKind::RunFailure);
            assert_eq!(item.run_id, Some(self.run.as_uuid()));
            assert_eq!(item.target, Some(AddressTarget::Principal(self.principal)));
            assert!(item.href.unwrap().contains(&self.run.to_string()));
        }
    }
}

#[tokio::test]
async fn direct_workflow_attention_summary_and_counts_never_treat_personal_work_as_company_scope() {
    let private = PrivateWork::new().await;
    let p = private.f.persistence();
    let owner = private.f.binding.target.actor.user_id();
    for (user, expected) in [(owner, 1), (private.user, 1), (private.teammate, 0)] {
        let counts = p.run_counts(private.company(), &[], user).await.unwrap();
        assert_eq!(counts.company.pending, expected);
        assert!(counts.channels.is_empty());
    }
    // Preserve the admitted private association and seed only the terminal read-model state.
    sqlx::query("UPDATE workflow_runs AS run SET state='failed', finished_at=CURRENT_TIMESTAMP, terminal_execution_id=(SELECT execution.id FROM workflow_executions AS execution WHERE execution.company_id=run.company_id AND execution.run_id=run.id) WHERE run.company_id=$1 AND run.id=$2")
        .bind(private.company()).bind(private.run.as_uuid()).execute(p.pool()).await.unwrap();
    private.assert_attention(owner, 1).await;
    private.assert_attention(private.user, 1).await;
    private.assert_attention(private.teammate, 0).await;
    let mut department = private.query(owner).await;
    department.direct_items = false;
    assert!(p.list_attention(department).await.unwrap().items.is_empty());

    p.remove_member(private.company(), private.user)
        .await
        .unwrap();
    private.assert_attention(owner, 1).await;
    // Removal retires the same historical principal; it grants no current reader authority.
    let former = AttentionQuery {
        principal_id: private.principal,
        ..private.query(owner).await
    };
    assert!(p.list_attention(former).await.unwrap().items.is_empty());
    private.assert_attention(private.teammate, 0).await;
}
