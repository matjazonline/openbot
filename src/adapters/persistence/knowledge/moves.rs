use super::*;

pub(super) async fn move_on(
    tx: &mut Transaction<'_, Postgres>,
    command: MoveKnowledgeDocument,
) -> AppResult<()> {
    let permissions = permissions::load(tx, command.scope).await?;
    let anchor = rows::one_visible(
        tx,
        DeleteKnowledgeLayer {
            scope: command.scope,
            document_id: command.document_id,
            expected_revision: command.expected_revision,
        },
        &permissions,
    )
    .await?;
    // Moving out of an archived unit is allowed; moving into one is not (Plan08 02C.1).
    permissions
        .require_write(
            tx,
            KnowledgeUploadTarget {
                scope: command.scope,
                unit: anchor.unit_id,
            },
        )
        .await?;
    permissions
        .require_write_into(
            tx,
            KnowledgeUploadTarget {
                scope: command.scope,
                unit: command.target,
            },
        )
        .await?;
    if anchor.revision != command.expected_revision {
        return Err(AppError::Conflict(
            "The file changed; refresh before moving it".into(),
        ));
    }
    let permitted: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM knowledge_documents AS layer
         JOIN knowledge_documents AS anchor ON anchor.company_id=layer.company_id AND anchor.name_key=layer.name_key AND anchor.unit_id=layer.unit_id
         WHERE anchor.company_id=$1 AND anchor.id=$2 AND layer.access_level>$3)",
    ).bind(command.scope.company.as_uuid()).bind(command.document_id).bind(permissions.actor.access_level.as_i16())
        .fetch_one(&mut **tx).await.map_err(authority::database)?;
    if !permitted {
        return Err(whole_document_refused());
    }
    if anchor.unit_id == command.target {
        return Ok(());
    }
    let collision: Option<bool> = sqlx::query_scalar(
        "SELECT bool_or(target.access_level<=$4) FROM knowledge_documents AS target
         JOIN knowledge_documents AS anchor ON anchor.company_id=target.company_id AND anchor.name_key=target.name_key
         WHERE anchor.company_id=$1 AND anchor.id=$2 AND target.unit_id=$3",
    ).bind(command.scope.company.as_uuid()).bind(command.document_id).bind(command.target.as_uuid()).bind(permissions.actor.access_level.as_i16())
        .fetch_one(&mut **tx).await.map_err(authority::database)?;
    if collision == Some(false) {
        return Err(whole_document_refused());
    }
    if collision == Some(true) {
        return Err(AppError::Conflict(
            "A document with this name already exists in the target department".into(),
        ));
    }
    sqlx::query(
        "UPDATE knowledge_documents AS layer SET unit_id=$3,revision=layer.revision+1,updated_at=clock_timestamp()
         FROM knowledge_documents AS anchor WHERE anchor.company_id=$1 AND anchor.id=$2
         AND layer.company_id=anchor.company_id AND layer.unit_id=anchor.unit_id AND layer.name_key=anchor.name_key",
    ).bind(command.scope.company.as_uuid()).bind(command.document_id).bind(command.target.as_uuid())
        .execute(&mut **tx).await.map_err(authority::database)?;
    Ok(())
}

fn whole_document_refused() -> AppError {
    AppError::Forbidden("The whole document cannot be moved with your current access".into())
}
