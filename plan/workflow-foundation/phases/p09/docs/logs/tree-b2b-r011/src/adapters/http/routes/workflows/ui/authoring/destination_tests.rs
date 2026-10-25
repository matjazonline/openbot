use super::*;
use crate::adapters::http::routes::workflows::bindings::{BindingRequest, configure_binding};
use crate::application::workflow::{RelatedAssociation, RelatedChannelId};

#[tokio::test]
async fn binding_destinations_authorize_tenant_role_and_private_channel() {
    let Some(f) = Fixture::new().await else {
        return;
    };
    let workflow = Uuid::new_v4();
    let version = f
        .published(workflow, "Support", &blank_source(workflow))
        .await;
    let private = f.channel("Private", "private", true).await;
    let binding = Uuid::new_v4();
    configure_binding(
        &f.engine,
        f.company,
        WorkflowActor::authenticated(f.owner).unwrap(),
        binding,
        BindingRequest {
            expected_state_revision: None,
            association: RelatedAssociation::Channel(RelatedChannelId::new(private)),
            version,
            params: json!({}),
            resources: BTreeMap::new(),
            run_as: None,
            message_trigger: None,
        },
    )
    .await
    .unwrap();
    let response = binding_destination(f.ui(f.owner), Path(binding), f.query())
        .await
        .unwrap()
        .into_response();
    assert_eq!(
        response.headers()[header::LOCATION],
        format!(
            "/ui/workflows?company_id={}&workflow_id={workflow}&tab=bindings",
            f.company
        )
    );
    for (user, company_id) in [
        (f.member, f.company),
        (f.admin, f.company),
        (f.stranger, f.company),
        (f.stranger, f.other),
    ] {
        assert!(matches!(
            binding_destination(
                f.ui(user),
                Path(binding),
                Query(CompanyQuery { company_id })
            )
            .await,
            Err(AppError::NotFound(_))
        ));
    }
}
