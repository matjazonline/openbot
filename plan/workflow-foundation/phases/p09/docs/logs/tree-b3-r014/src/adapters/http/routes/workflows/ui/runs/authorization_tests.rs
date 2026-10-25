//! Reconciliation and execution reads with real inaccessible identities.
use super::*;

/// Valid action/execution identities remain inaccessible through foreign company paths and a
/// private channel. The owner then uses the very same fields successfully as a positive control.
#[tokio::test]
async fn reconciliation_and_step_refuse_foreign_and_hidden_channel_identities() {
    use crate::adapters::persistence::workflow::reconciliation_fixture::{
        HttpForm, uncertain_http,
    };
    for kind in [HttpForm::Slot, HttpForm::PrivateChannelSlot] {
        let action = uncertain_http(kind).await;
        let engine: Engine = Arc::new(WorkflowEngine::for_tests(action.persistence.clone()));
        let owner_ui = || ui_over(&action.persistence, &engine, action.owner);
        let shown = text(
            pane(
                owner_ui(),
                Path(action.run),
                pane_query(action.company, None),
            )
            .await,
        )
        .await
        .unwrap();
        let fields = [
            "execution_id",
            "invocation_id",
            "argument_digest",
            "marker_id",
            "target",
            "expected_revision",
            "command_key",
        ]
        .map(|name| (name, field_value(&shown, name)));
        let mut submitted: Vec<(&str, &str)> = fields
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        submitted.push(("note", "No confirmation from vendor yet."));
        let (stranger, other) = owner_and_company(&action.persistence, "Foreign annotation").await;
        let mut denied = vec![(stranger, other), (stranger, action.company)];
        if matches!(kind, HttpForm::PrivateChannelSlot) {
            denied.push((
                teammate(&action.persistence, action.company, "admin").await,
                action.company,
            ));
        }
        for (viewer, company_id) in denied {
            let ui = || ui_over(&action.persistence, &engine, viewer);
            let refused = reconcile(
                ui(),
                Path(action.run),
                Query(CompanyQuery { company_id }),
                form(&submitted),
            )
            .await;
            assert!(
                matches!(refused, Err(WorkflowError::App(AppError::NotFound(_)))),
                "{refused:?}"
            );
            let refused = step(
                ui(),
                Path((action.run, action.execution)),
                Query(CompanyQuery { company_id }),
            )
            .await;
            assert!(matches!(refused, Err(AppError::NotFound(_))), "{refused:?}");
        }
        let count = || async {
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workflow_action_evidence WHERE company_id = $1 AND run_id = $2")
                .bind(action.company).bind(action.run).fetch_one(action.persistence.pool()).await.unwrap()
        };
        assert_eq!(count().await, 0);
        let accepted = reconcile(
            owner_ui(),
            Path(action.run),
            Query(CompanyQuery {
                company_id: action.company,
            }),
            form(&submitted),
        )
        .await
        .unwrap();
        assert!(accepted.0.contains("Recorded:"));
        assert_eq!(count().await, 1);
    }
}
