//! A real occurrence claim does not confer its run-as person's conversation access.
use super::*;
use crate::application::ports::participant::PrincipalAccessPersistence;
use crate::application::workflow::schedule_admission::{
    OccurrenceClaim, ScheduleOccurrences, ScheduleWorkerId,
};
use crate::application::workflow::{LifecycleAuthorizer, WorkflowAuthorization, WorkflowOperation};
use crate::entities::company_member::CompanyAccessRole;
use crate::use_cases::user::UserPersistence;

#[tokio::test]
async fn delegated_admin_schedule_claim_requires_human_view_before_both_admission_boundaries() {
    let f = AdmissionFixture::new().await;
    let c = Conversation::new(&f).await;
    let p = f.persistence();
    let company = f.binding.target.company;
    let actor = f.binding.target.actor;
    let occurrence = Occurrence::new(&f, company, c.channel).await;
    let worker = ScheduleWorkerId(Uuid::new_v4());
    let claimed = p
        .claim_occurrence(
            company,
            occurrence.occurrence,
            worker,
            chrono::Utc::now() + chrono::Duration::seconds(60),
        )
        .await
        .unwrap()
        .unwrap();
    let claim = OccurrenceClaim {
        company,
        occurrence: claimed.id,
        worker,
        generation: claimed.materialization_generation,
    };
    let request = || {
        let mut request = occurrence.request(&f, "private-schedule-claim");
        request.trigger = TriggerRef::new(
            company,
            TriggerId::new(claimed.id),
            TriggerSource::Schedule {
                schedule_id: ScheduleId::new(claimed.schedule_id),
                occurrence_id: ScheduleOccurrenceId::new(claimed.id),
            },
        )
        .unwrap();
        request.admission_claim = Some(AdmissionClaim::Schedule(claim));
        request
    };
    // Capture an otherwise valid command before revocation, so the direct adapter
    // assertion cannot be satisfied by a missing schedule, claim or invalid input.
    let prepared = f.prepare(request()).await;
    lose_human_view(&f, c.channel).await;
    let authorizer = LifecycleAuthorizer::new(p.clone(), p.clone(), p.clone());
    authorizer
        .authorize(
            company,
            actor,
            prepared.association(),
            WorkflowOperation::ManageBinding,
        )
        .await
        .expect("the person still manages this desk");
    let service = WorkflowService::new(p.clone(), p.clone(), NoRuntime, NoRuntime, authorizer);
    let before = f.counts().await;
    assert!(matches!(
        service.admit(request()).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(f.counts().await, before);
    assert!(matches!(
        p.admit(&prepared).await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(f.counts().await, before, "no run, job, event or admission");
    restore_human_view(&f, c.channel).await;
    assert!(matches!(
        service.admit(request()).await.unwrap(),
        AdmissionResult::Created(_)
    ));
}

async fn lose_human_view(f: &AdmissionFixture, channel: Uuid) {
    let p = f.persistence();
    let suffix = Uuid::new_v4().simple().to_string();
    let owner = p
        .create_user(&suffix, &format!("{suffix}@example.test"), "hash")
        .await
        .unwrap();
    crate::adapters::persistence::test_support::transfer_company_ownership(
        p.pool(),
        f.binding.target.company.as_uuid(),
        owner.id,
        CompanyAccessRole::Admin,
    )
    .await;
    crate::adapters::persistence::test_support::restrict_channel_to_configured_grants(
        p.pool(),
        channel,
    )
    .await;
    let context = p
        .access_context_for_user(
            f.binding.target.company.as_uuid(),
            f.binding.target.actor.user_id(),
        )
        .await
        .unwrap()
        .unwrap();
    let channel = ChannelPersistence::get_by_id(p, channel)
        .await
        .unwrap()
        .unwrap();
    assert!(!channel.viewer_access(context));
}

async fn restore_human_view(f: &AdmissionFixture, channel: Uuid) {
    sqlx::query(
        "INSERT INTO channel_principal_grants(company_id,channel_id,principal_id,capability,provenance) \
         SELECT $1,$2,id,'view','configured_allowlist' FROM principals \
         WHERE company_id=$1 AND user_id=$3 AND kind='person'",
    )
    .bind(f.binding.target.company.as_uuid())
    .bind(channel)
    .bind(f.binding.target.actor.user_id())
    .execute(f.persistence().pool())
    .await
    .unwrap();
    // Configured-email-derived grants retain their independent address proof fence.
    sqlx::query("UPDATE users SET email_verified_at=CURRENT_TIMESTAMP WHERE id=$1")
        .bind(f.binding.target.actor.user_id())
        .execute(f.persistence().pool())
        .await
        .unwrap();
}
