//! Shared payload visibility for escalation notices and commands; association authority is separate.
use super::*;

#[derive(sqlx::FromRow)]
struct ScopeRow {
    source_kind: String,
    run_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    thread_id: Option<Uuid>,
    correlation_id: Option<Uuid>,
    source_key: Option<String>,
}

enum PayloadSource {
    Run(RunId),
    Admission(String),
}

pub(crate) struct CaseScope {
    company: CompanyId,
    payload: PayloadSource,
    pub run_id: Option<Uuid>,
    pub channel_id: Option<Uuid>,
    pub thread_id: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CasePayloadAccess {
    Visible,
    Restricted,
    Unavailable,
}

pub(crate) async fn case_scope_on(
    db: &mut PgConnection,
    company: CompanyId,
    case: EscalationId,
) -> AppResult<Option<CaseScope>> {
    let row=sqlx::query_as::<_,ScopeRow>("SELECT escalation.source_kind,escalation.run_id,run.correlation_id, \
        COALESCE(run.channel_id,refusal.channel_id) AS channel_id,COALESCE(run.thread_id,refusal.thread_id) AS thread_id,refusal.source_key \
        FROM workflow_escalation_cases AS escalation \
        LEFT JOIN workflow_runs AS run ON run.company_id=escalation.company_id AND run.id=escalation.run_id \
        LEFT JOIN workflow_admission_refusals AS refusal ON refusal.company_id=escalation.company_id AND refusal.id=escalation.admission_refusal_id \
        WHERE escalation.company_id=$1 AND escalation.id=$2")
        .bind(company.as_uuid()).bind(case.as_uuid()).fetch_optional(db).await?;
    let Some(row) = row else { return Ok(None) };
    let payload = match row.source_kind.as_str() {
        "run" | "empty_reviewers" => {
            PayloadSource::Run(RunId::new(row.run_id.ok_or_else(invalid)?))
        }
        "admission" => PayloadSource::Admission(row.source_key.ok_or_else(invalid)?),
        _ => return Err(invalid()),
    };
    Ok(Some(CaseScope {
        company,
        payload,
        run_id: row.run_id,
        channel_id: row.channel_id,
        thread_id: row.thread_id,
        correlation_id: row.correlation_id,
    }))
}

/// Callers first prove current membership and association access. This final joint query uses
/// the actual source stamp; neither an old role assignment nor a historical actor grants access.
pub(crate) async fn payload_access_on(
    db: &mut PgConnection,
    scope: &CaseScope,
    viewer: WorkflowActor,
) -> AppResult<CasePayloadAccess> {
    use crate::adapters::persistence::view_visibility;
    let (identity, levels) = match &scope.payload {
        PayloadSource::Run(run) => {
            let identity = run.as_uuid();
            (
                identity,
                view_visibility::run_levels_on(db, scope.company, viewer, &[identity]).await?,
            )
        }
        PayloadSource::Admission(source) => {
            match admission_binding::SourceKey::payload_source(source)? {
                admission_binding::AdmissionPayloadSource::Public => {
                    return Ok(CasePayloadAccess::Visible);
                }
                admission_binding::AdmissionPayloadSource::Message(message) => (
                    message,
                    view_visibility::message_levels_on(db, scope.company, viewer, &[message])
                        .await?,
                ),
            }
        }
    };
    Ok(match levels.get(&identity) {
        None => CasePayloadAccess::Unavailable,
        Some(visibility) if visibility.is_visible() => CasePayloadAccess::Visible,
        Some(_) => CasePayloadAccess::Restricted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workflow_escalation_payload_source_validates_identity_before_using_a_public_stamp() {
        let id = Uuid::new_v4();
        for body in [
            serde_json::json!(["manual", id]),
            serde_json::json!(["schedule", id, id]),
        ] {
            assert!(matches!(
                admission_binding::SourceKey::payload_source(&format!("v1:{body}")),
                Ok(admission_binding::AdmissionPayloadSource::Public)
            ));
        }
        assert!(
            matches!(admission_binding::SourceKey::payload_source(&format!("v1:{}",serde_json::json!(["message",id]))),Ok(admission_binding::AdmissionPayloadSource::Message(message)) if message==id)
        );
        for body in [
            serde_json::json!(["message", "broken"]),
            serde_json::json!(["manual", 0]),
            serde_json::json!(["schedule", id, "broken"]),
            serde_json::json!(["child", id]),
            serde_json::json!({}),
        ] {
            assert!(
                admission_binding::SourceKey::payload_source(&format!("v1:{body}")).is_err(),
                "{body}"
            );
        }
    }
}
