//! Each historical proposal/feedback belongs to its own run, including higher-level ancestors.
use super::*;

pub(super) async fn project_on(
    tx: &mut Transaction<'_, Postgres>,
    company: CompanyId,
    actor: WorkflowActor,
    view: &mut DecisionView,
) -> AppResult<()> {
    let mut runs = view
        .history
        .iter()
        .map(|entry| entry.run.as_uuid())
        .collect::<Vec<_>>();
    runs.push(view.run.as_uuid());
    runs.sort();
    runs.dedup();
    // No payload query follows this fresh joint authority/stamp statement.
    let levels = crate::adapters::persistence::view_visibility::run_levels_on(
        &mut **tx, company, actor, &runs,
    )
    .await?;
    for entry in &mut view.history {
        let visibility = levels
            .get(&entry.run.as_uuid())
            .copied()
            .ok_or_else(invalid)?;
        if !visibility.is_visible() {
            entry.redact_payload();
        }
    }
    let visibility = levels
        .get(&view.run.as_uuid())
        .copied()
        .ok_or_else(invalid)?;
    view.project(visibility);
    Ok(())
}
