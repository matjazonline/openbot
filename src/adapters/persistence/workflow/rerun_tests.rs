//! Rerun links (07-D1 §D1.7): a manual start may name a top-level run of the same workflow that
//! the actor can see; every other original is refused before anything is written.
use super::*;

fn rerun(f: &AdmissionFixture, key: &str, original: RunId) -> AdmitWorkflowRequest {
    let mut request = f.request(key, f.manual());
    request.rerun_of = Some(original);
    request
}

/// `request` again under its key, trigger and input, naming `rerun_of` instead.
fn relinked(request: &AdmitWorkflowRequest, rerun_of: Option<RunId>) -> AdmitWorkflowRequest {
    AdmitWorkflowRequest {
        company_id: request.company_id,
        actor: request.actor,
        association: request.association,
        trigger: request.trigger.clone(),
        correlation_id: request.correlation_id,
        binding_id: request.binding_id,
        idempotency_key: request.idempotency_key.clone(),
        input: request.input.clone(),
        rerun_of,
        delivery_policy: crate::application::workflow::DeliveryPolicy::Deliver,
        expected_binding_revision: None,
    }
}

async fn admitted(f: &AdmissionFixture, request: AdmitWorkflowRequest) -> RunId {
    let command = f.prepare(request).await;
    match f.persistence().admit(&command).await.unwrap() {
        AdmissionResult::Created(run) => run,
        other => panic!("{other:?}"),
    }
}

/// Admission of `request` fails with exactly this error and writes nothing.
async fn refused(f: &AdmissionFixture, request: AdmitWorkflowRequest) -> AppError {
    let before = f.counts().await;
    let command = f.prepare(request).await;
    let error = f.persistence().admit(&command).await.unwrap_err();
    assert_eq!(f.counts().await, before, "a refused rerun writes nothing");
    error
}

fn data_map() -> Value {
    serde_json::from_str(&registry::example("data.map").unwrap().source).unwrap()
}

#[tokio::test]
async fn workflow_rerun_links_its_original_and_replays_only_the_same_link() {
    let f = AdmissionFixture::new().await;
    let original = admitted(&f, f.request("original", f.manual())).await;
    let request = rerun(&f, "rerun", original);
    let command = f.prepare(relinked(&request, request.rerun_of)).await;
    let AdmissionResult::Created(run) = f.persistence().admit(&command).await.unwrap() else {
        panic!("created")
    };
    let head = f
        .persistence()
        .head(f.binding.target.company, run)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(head.rerun_of, Some(original));
    let first = f
        .persistence()
        .head(f.binding.target.company, original)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.rerun_of, None);
    // The same command replays; the same key with another link, or none, conflicts.
    assert_eq!(
        f.persistence().admit(&command).await.unwrap(),
        AdmissionResult::Replayed(run)
    );
    let other = admitted(&f, f.request("other", f.manual())).await;
    for link in [None, Some(other), Some(run)] {
        let changed = f.prepare(relinked(&request, link)).await;
        assert_eq!(
            f.persistence().admit(&changed).await.unwrap(),
            AdmissionResult::Conflict,
            "{link:?}"
        );
    }
    // A rerun may itself be rerun: the link names a top-level run, whatever its own link.
    let again = admitted(&f, rerun(&f, "again", run)).await;
    assert_ne!(again, run);
}

#[tokio::test]
async fn workflow_rerun_refuses_hidden_originals_identically_and_writes_nothing() {
    let f = AdmissionFixture::new().await;
    let unknown = refused(&f, rerun(&f, "unknown", RunId::new(Uuid::new_v4()))).await;
    assert!(matches!(unknown, AppError::NotFound(_)), "{unknown:?}");
    // Another company's run is as unknown as a run that never existed.
    let foreign = f.foreign().await;
    let theirs = admitted(&foreign, foreign.request("theirs", foreign.manual())).await;
    let error = refused(&f, rerun(&f, "foreign", theirs)).await;
    assert_eq!(format!("{error:?}"), format!("{unknown:?}"));
    // A run in a channel the actor can no longer see is refused the same way, for a new rerun
    // and for the replay of one admitted while the channel was visible.
    let c = Conversation::new(&f).await;
    let principal = restrict_channel(&f, &c).await;
    let hidden = admitted(&f, c.request(&f, "hidden", c.message)).await;
    let visible = f.prepare(rerun(&f, "visible", hidden)).await;
    assert!(matches!(
        f.persistence().admit(&visible).await.unwrap(),
        AdmissionResult::Created(_)
    ));
    sqlx::query("DELETE FROM channel_principal_grants WHERE channel_id=$1 AND principal_id=$2")
        .bind(c.channel)
        .bind(principal)
        .execute(f.persistence().pool())
        .await
        .unwrap();
    let error = refused(&f, rerun(&f, "hidden-rerun", hidden)).await;
    assert_eq!(format!("{error:?}"), format!("{unknown:?}"));
    let before = f.counts().await;
    let error = f.persistence().admit(&visible).await.unwrap_err();
    assert_eq!(format!("{error:?}"), format!("{unknown:?}"));
    assert_eq!(f.counts().await, before);
}

#[tokio::test]
async fn workflow_rerun_refuses_child_and_other_workflow_originals() {
    // Another workflow of the same company: visible, but not this binding's workflow.
    let f = AdmissionFixture::new().await;
    let other = f.sibling(WorkflowId::new(Uuid::from_u128(2))).await;
    let theirs = admitted(&other, other.request("theirs", other.manual())).await;
    let error = refused(&f, rerun(&f, "other-workflow", theirs)).await;
    assert!(matches!(error, AppError::BadRequest(_)), "{error:?}");
    // A runtime child of this workflow's run is not a top-level original.
    let (chain, scopes) = AdmissionFixture::chain(data_map(), 1).await;
    let error = refused(&chain, rerun(&chain, "child", scopes[1].run)).await;
    assert!(matches!(error, AppError::BadRequest(_)), "{error:?}");
    // The root of that chain is a valid original.
    admitted(&chain, rerun(&chain, "root", scopes[0].run)).await;
}

#[tokio::test]
async fn workflow_rerun_is_only_a_manual_start() {
    let f = AdmissionFixture::new().await;
    let original = admitted(&f, f.request("original", f.manual())).await;
    let c = Conversation::new(&f).await;
    let mut request = c.request(&f, "message-rerun", c.message);
    request.rerun_of = Some(original);
    let before = f.counts().await;
    let result = WorkflowService::new(
        f.persistence().clone(),
        Capture::default(),
        NoRuntime,
        NoRuntime,
        Preflight,
    )
    .admit(request)
    .await;
    assert!(matches!(result, Err(AppError::BadRequest(_))), "{result:?}");
    assert_eq!(f.counts().await, before);
}
