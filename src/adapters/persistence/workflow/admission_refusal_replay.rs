use super::*;

pub(in crate::adapters::persistence::workflow) enum RefusalReplay {
    Refused {
        refusal: RoleAdmissionRefusal,
        revision: BindingRevision,
    },
    Conflict,
}

#[derive(sqlx::FromRow)]
struct SavedRefusal {
    id: Uuid,
    binding_id: Uuid,
    binding_revision: i64,
    source_key: String,
    request_fingerprint: String,
    role_refusal: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct SavedCommand {
    run_id: Option<Uuid>,
    refusal_id: Option<Uuid>,
    trigger_id: Uuid,
}

pub(in crate::adapters::persistence::workflow) struct RefusalKey<'a> {
    pub company: CompanyId,
    pub binding: WorkflowBindingId,
    pub command: &'a IdempotencyKey,
    pub trigger: &'a TriggerRef,
    pub source: &'a admission_binding::SourceKey,
}

enum Selection {
    None,
    Refused {
        saved: Box<SavedRefusal>,
        command_exists: bool,
    },
    Conflict,
}

async fn select_on(db: &mut PgConnection, key: &RefusalKey<'_>) -> AppResult<Selection> {
    let command = sqlx::query_as::<_, SavedCommand>(
        "SELECT run_id,refusal_id,trigger_id FROM workflow_admission_commands \
         WHERE company_id=$1 AND command_key=$2",
    )
    .bind(key.company.as_uuid())
    .bind(key.command.as_str())
    .fetch_optional(&mut *db)
    .await?;
    if command.as_ref().is_some_and(|saved| saved.run_id.is_some()) {
        return Ok(Selection::None); // The successful replay owner validates these commands.
    }
    let refused = sqlx::query_as::<_, SavedRefusal>(
        "SELECT id,binding_id,binding_revision,source_key,request_fingerprint,role_refusal \
         FROM workflow_admission_refusals WHERE company_id=$1 \
         AND (($4::uuid IS NOT NULL AND id=$4) \
           OR ($4::uuid IS NULL AND binding_id=$2 AND source_key=$3))",
    )
    .bind(key.company.as_uuid())
    .bind(key.binding.as_uuid())
    .bind(key.source.as_str())
    .bind(command.as_ref().and_then(|saved| saved.refusal_id))
    .fetch_optional(&mut *db)
    .await?;
    let Some(refused) = refused else {
        return Ok(Selection::None);
    };
    if refused.binding_id != key.binding.as_uuid()
        || refused.source_key != key.source.as_str()
        || command
            .as_ref()
            .is_some_and(|saved| saved.trigger_id != key.trigger.trigger_id().as_uuid())
    {
        return Ok(Selection::Conflict);
    }
    Ok(Selection::Refused {
        saved: Box::new(refused),
        command_exists: command.is_some(),
    })
}

fn saved_revision(saved: &SavedRefusal) -> AppResult<BindingRevision> {
    BindingRevision::new(u64::try_from(saved.binding_revision).map_err(|_| invalid())?)
        .ok_or_else(invalid)
}

pub(in crate::adapters::persistence::workflow) async fn saved_binding_on(
    db: &mut PgConnection,
    key: RefusalKey<'_>,
) -> AppResult<Option<Arc<binding::ConfiguredBinding>>> {
    match select_on(db, &key).await? {
        Selection::None => Ok(None),
        Selection::Conflict => Err(conflict()),
        Selection::Refused { saved, .. } => {
            let state = binding_rows::read_binding_at(
                db,
                key.company,
                key.binding,
                Some(saved_revision(&saved)?),
            )
            .await?
            .ok_or_else(invalid)?;
            Ok(Some(state.configuration))
        }
    }
}

pub(in crate::adapters::persistence::workflow) async fn resolve_on(
    db: &mut PgConnection,
    command: &PreparedAdmission,
    source: &admission_binding::SourceKey,
) -> AppResult<Option<RefusalReplay>> {
    let selection = select_on(
        db,
        &RefusalKey {
            company: command.company_id(),
            binding: command.binding().id(),
            command: command.idempotency_key(),
            trigger: command.trigger(),
            source,
        },
    )
    .await?;
    let Selection::Refused {
        saved,
        command_exists,
    } = selection
    else {
        return Ok(match selection {
            Selection::Conflict => Some(RefusalReplay::Conflict),
            _ => None,
        });
    };
    if saved.request_fingerprint != fingerprint(command)? {
        return Ok(Some(RefusalReplay::Conflict));
    }
    if !command_exists {
        insert_command_on(db, command, AdmissionRefusalId::new(saved.id)).await?;
    }
    let revision = saved_revision(&saved)?;
    let refusal = serde_json::from_value(saved.role_refusal).map_err(|_| invalid())?;
    Ok(Some(RefusalReplay::Refused { refusal, revision }))
}

pub(super) async fn insert_command_on(
    db: &mut PgConnection,
    command: &PreparedAdmission,
    refusal: AdmissionRefusalId,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO workflow_admission_commands(company_id,command_key,refusal_id,trigger_id) \
         VALUES ($1,$2,$3,$4)",
    )
    .bind(command.company_id().as_uuid())
    .bind(command.idempotency_key().as_str())
    .bind(refusal.as_uuid())
    .bind(command.trigger().trigger_id().as_uuid())
    .execute(db)
    .await?;
    Ok(())
}
