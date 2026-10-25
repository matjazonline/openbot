//! The versions tab: its usage list and the compare between two sources.
use super::super::super::bindings::Visibility;
use super::*;

impl Ui {
    pub(super) async fn versions_body(
        &self,
        company: Uuid,
        detail: &DefinitionDetail,
        banner: Option<&str>,
        compare_html: Option<&str>,
    ) -> AppResult<String> {
        let company_id = super::company(company);
        let usage = self
            .engine
            .authoring_reads()
            .workflow_usage(company_id, detail.summary.workflow)
            .await?;
        let mut visibility = Visibility::new(&self.engine, company_id, self.actor()?);
        let mut visible = Vec::new();
        let mut hidden = 0;
        for binding in usage.bindings {
            if visibility.allows(binding.binding.association).await? {
                visible.push(binding);
            } else {
                hidden += 1;
            }
        }
        visibility.report("workflow-usage");
        Ok(pages::workflow_versions(&pages::VersionsView {
            company_id: company,
            workflow: detail.summary.workflow.as_uuid(),
            draft_revision: detail.summary.revision,
            versions: &detail.versions,
            versions_truncated: detail.versions_truncated,
            usage: &visible,
            usage_truncated: usage.truncated,
            hidden_bindings: hidden,
            banner,
            compare_html,
        }))
    }
}

/// GET /ui/workflows/{workflow}/compare — a line diff between two of: the draft, a version.
pub(super) async fn compare(
    ui: Ui,
    Path(workflow): Path<Uuid>,
    Query(query): Query<CompareQuery>,
) -> AppResult<Html<String>> {
    let company = ui.company(query.company_id).await?.id;
    let detail = ui.detail(company, workflow).await?;
    let side = |name: &str| -> AppResult<Option<Uuid>> {
        match name {
            "draft" => Ok(None),
            other => other
                .parse()
                .map(Some)
                .map_err(|_| AppError::BadRequest("Compare names the draft or a version".into())),
        }
    };
    let mut sources = Vec::with_capacity(2);
    for name in [&query.from, &query.to] {
        sources.push(match side(name)? {
            None => (
                format!("draft revision {}", detail.summary.revision),
                detail.source.clone(),
            ),
            Some(version) => {
                let version = ui
                    .engine
                    .authoring_reads()
                    .version_source(
                        super::company(company),
                        WorkflowId::new(workflow),
                        VersionId::new(version),
                    )
                    .await?
                    .ok_or_else(|| AppError::NotFound("Workflow version not found".into()))?;
                (
                    format!(
                        "version {} (draft revision {})",
                        pages::short_id(version.version.as_uuid()),
                        version.draft_revision
                    ),
                    version.source,
                )
            }
        });
    }
    let (to_label, to_source) = sources.pop().expect("two sides");
    let (from_label, from_source) = sources.pop().expect("two sides");
    Ok(Html(pages::diff_block(
        &line_diff(&from_source, &to_source),
        &from_label,
        &to_label,
    )))
}
