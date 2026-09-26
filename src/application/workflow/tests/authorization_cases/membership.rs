use super::*;

#[tokio::test]
async fn current_manager_membership_controls_admit_and_cancel_before_reads() {
    let company_id = company();
    let version_id = version();
    let store = MemoryStore::new(owned(company_id, version_id, 128));
    let admitted_runs = admit_and_cancel_as_managers(&store, company_id, version_id).await;
    let count = store.state.lock().unwrap().runs.len();
    deny_members_before_reads(&store, company_id, version_id, admitted_runs[0]).await;
    store
        .state
        .lock()
        .unwrap()
        .access
        .remove(&(company_id, actor().user_id()));
    assert!(matches!(
        service(&store)
            .admit(admitted_request(
                company_id,
                version_id,
                "outsider",
                RelatedAssociation::Company
            ))
            .await,
        Err(AppError::NotFound(_))
    ));
    assert_eq!(store.state.lock().unwrap().runs.len(), count);
}

async fn admit_and_cancel_as_managers(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
) -> Vec<RunId> {
    let mut admitted_runs = Vec::new();
    for membership in [CompanyMembership::Owner, CompanyMembership::Admin] {
        set_membership(store, company_id, membership, None);
        let key = format!("manager-{membership:?}");
        let result = service(store)
            .admit(admitted_request(
                company_id,
                version_id,
                &key,
                RelatedAssociation::Company,
            ))
            .await
            .unwrap();
        let AdmissionResult::Created(run_id) = result else {
            panic!("expected admission")
        };
        admitted_runs.push(run_id);
        assert!(matches!(
            service(store)
                .cancel(CancelWorkflowRequest {
                    company_id,
                    actor: actor(),
                    run_id
                })
                .await
                .unwrap(),
            CancelResult::Applied { .. }
        ));
    }
    admitted_runs
}

async fn deny_members_before_reads(
    store: &MemoryStore,
    company_id: CompanyId,
    version_id: VersionId,
    run_id: RunId,
) {
    for membership in [CompanyMembership::Member, CompanyMembership::None] {
        set_membership(store, company_id, membership, None);
        {
            let mut state = store.state.lock().unwrap();
            state.fail_lookup = true;
            state.fail_head = true;
        }
        assert!(matches!(
            service(store)
                .admit(admitted_request(
                    company_id,
                    version_id,
                    "denied",
                    RelatedAssociation::Company
                ))
                .await,
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            service(store)
                .cancel(CancelWorkflowRequest {
                    company_id,
                    actor: actor(),
                    run_id
                })
                .await,
            Err(AppError::NotFound(_))
        ));
        {
            let mut state = store.state.lock().unwrap();
            state.fail_lookup = false;
            state.fail_head = false;
        }
    }
}
