//! Native storage fixtures create an unsealed owner before inserting deliberately authored
//! manifest content. Production admission now always seals a manifest and cannot serve this role.
use super::*;
use crate::adapters::persistence::workflow::admission_write::{
    FirstJob, ParentLink, RunDeadline, RunRow,
};
use crate::application::workflow::binding::ConfiguredBinding;

struct NativeRun<'a> {
    binding: &'a ConfiguredBinding,
    parent: Option<ParentLink>,
    trigger: TriggerRef,
}

pub(super) async fn unsealed_chain() -> (AdmissionFixture, Vec<ActivationRequest>) {
    let leaf = serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap();
    let f = AdmissionFixture::calling(leaf).await;
    let command = f.prepare(f.request("native-owner", f.manual())).await;
    let root = insert(
        &f,
        NativeRun {
            binding: command.binding(),
            parent: None,
            trigger: f.manual(),
        },
    )
    .await;
    let version = *command.binding().bundle().children().keys().next().unwrap();
    let configured = children::child_binding(command.binding(), version, json!({}))
        .unwrap()
        .unwrap();
    let source = TriggerSource::Child {
        parent: crate::domain::workflow::ChildCause::Step(
            crate::domain::workflow::ChildStepRef::new(
                crate::domain::workflow::ExecutionRef::new(
                    root.company,
                    root.run,
                    root.execution,
                    StepId::parse("start").unwrap(),
                ),
                crate::domain::workflow::ChildOrdinal::new(1).unwrap(),
            ),
        ),
    };
    let child = insert(
        &f,
        NativeRun {
            binding: &configured,
            parent: Some(ParentLink {
                run: root.run,
                execution: root.execution,
            }),
            trigger: TriggerRef::new(root.company, TriggerId::new(Uuid::new_v4()), source).unwrap(),
        },
    )
    .await;
    (f, vec![root, child])
}

pub(super) async fn unsealed_root(f: &AdmissionFixture, key: &str) -> ActivationRequest {
    let command = f.prepare(f.request(key, f.manual())).await;
    insert(
        f,
        NativeRun {
            binding: command.binding(),
            parent: None,
            trigger: f.manual(),
        },
    )
    .await
}

async fn insert(f: &AdmissionFixture, owner: NativeRun<'_>) -> ActivationRequest {
    let company = f.binding.target.company;
    let actor = f.binding.target.actor;
    let run = RunId::new(Uuid::new_v4());
    let mut tx = f.persistence().pool().begin().await.unwrap();
    let request = f.prepare(f.request("native-levels", f.manual())).await;
    let levels = knowledge::admission_levels_on(&mut tx, &request)
        .await
        .unwrap();
    let deadline = if let Some(parent) = owner.parent {
        let deadline =
            sqlx::query_scalar("SELECT deadline FROM workflow_runs WHERE company_id=$1 AND id=$2")
                .bind(company.as_uuid())
                .bind(parent.run.as_uuid())
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        RunDeadline::Inherited(deadline)
    } else {
        RunDeadline::AdmissionWindow
    };
    admission_write::insert_run(
        &mut tx,
        &RunRow {
            company,
            knowledge: levels,
            run,
            binding: owner.binding,
            association: RelatedAssociation::Company,
            actor_id: actor.user_id(),
            trigger: owner.trigger.trigger_id(),
            correlation: Uuid::new_v4(),
            input: &json!({"value":1}),
            deadline,
            parent: owner.parent,
            rerun_of: None,
            delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
        },
    )
    .await
    .unwrap();
    admission_write::insert_first_job(
        &mut tx,
        &FirstJob {
            company,
            run,
            execution: ExecutionId::new(Uuid::new_v4()),
            entry: &owner.binding.bundle().compiled().graph().definition().entry,
            association: RelatedAssociation::Company,
            correlation: Uuid::new_v4(),
        },
    )
    .await
    .unwrap();
    admission_write::insert_admission(
        &mut tx,
        company,
        owner.binding.id(),
        &admission_binding::SourceKey::from_trigger(&owner.trigger),
        run,
    )
    .await
    .unwrap();
    if let Some(parent) = owner.parent {
        sqlx::query("INSERT INTO workflow_waits(company_id,run_id,execution_id,id,reason,deadline,created_at,child_run_id,child_ordinal) SELECT $1,$2,$3,$4,'child_run',deadline,clock_timestamp(),$5,1 FROM workflow_runs WHERE company_id=$1 AND id=$2")
            .bind(company.as_uuid()).bind(parent.run.as_uuid()).bind(parent.execution.as_uuid()).bind(Uuid::new_v4()).bind(run.as_uuid())
            .execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    super::super::super::child_support::entry(f, run).await
}
