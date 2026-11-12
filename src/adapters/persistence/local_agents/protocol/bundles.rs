//! A read-only use of an existing live connection; never acquires or renews a lease.
use super::*;
use crate::application::local_agents::harness::{NativeBundleRequest, NativeBundleResponse};

#[derive(sqlx::FromRow)]
struct AuthenticationRow {
    company_id: Uuid,
    machine_id: Uuid,
    owner: Uuid,
}

pub(super) async fn read(
    p: &PostgresPersistence,
    request: &NativeBundleRequest,
) -> AppResult<NativeBundleResponse> {
    let mut tx = begin(&p.pool).await?;
    let authority = authorize_on(&mut tx, request).await?;
    let bytes = authority.bundle.canonical_content()?;
    let digest = NativeBundleDigest::of(&bytes);
    tx.commit().await?;
    Ok(NativeBundleResponse { bytes, digest })
}

pub(super) struct NativeAuthority {
    pub company: Uuid,
    pub machine: MachineId,
    pub actor: Uuid,
    pub operation: LocalAgentOperation,
    pub session: crate::domain::local_agents::LocalAgentSession,
    pub bundle: crate::application::local_agents::harness::NativeBundle,
}

pub(super) async fn authorize_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &NativeBundleRequest,
) -> AppResult<NativeAuthority> {
    authorize_with_locks_on(
        tx,
        request,
        crate::adapters::persistence::workflow::row_locks::RowLocks::Wait,
    )
    .await
}

pub(super) async fn authorize_with_locks_on(
    tx: &mut Transaction<'_, Postgres>,
    request: &NativeBundleRequest,
    locks: crate::adapters::persistence::workflow::row_locks::RowLocks,
) -> AppResult<NativeAuthority> {
    // Observe identity without a machine lock, then fence it after actor/source/grant locks.
    let identity = request.identity;
    let authentication = sqlx::query_as::<_, AuthenticationRow>(
        "SELECT machine.company_id,machine.id AS machine_id,machine.connection_owner AS owner \
         FROM local_machines AS machine JOIN local_machine_credentials AS credential \
           ON credential.company_id=machine.company_id AND credential.machine_id=machine.id AND credential.generation=machine.credential_generation \
         WHERE credential.verifier=$1 AND credential.revoked_at IS NULL AND machine.installation_id=$2 \
           AND machine.credential_generation=$3 AND machine.connection_generation=$4 AND machine.state='enabled' \
           AND machine.connection_owner IS NOT NULL AND machine.connection_expires_at>clock_timestamp()")
        .bind(request.verifier.as_bytes().as_slice()).bind(identity.installation_id.as_uuid())
        .bind(identity.credential_generation.get() as i64).bind(identity.connection_generation.get() as i64)
        .fetch_optional(&mut **tx).await?.ok_or(AppError::InvalidCredentials)?;
    let machine_id = MachineId::new(authentication.machine_id).map_err(|_| invalid("machine"))?;
    let row = sqlx::query_as::<_,OperationDb>(&format!("SELECT {} FROM local_agent_operations WHERE company_id=$1 AND machine_id=$2 AND installation_id=$3 AND id=$4", operation_columns()))
        .bind(authentication.company_id).bind(authentication.machine_id).bind(identity.installation_id.as_uuid())
        .bind(request.operation.as_uuid()).fetch_optional(&mut **tx).await?.ok_or_else(missing)?;
    let operation = row.operation()?;
    if !operation.command().is_effect() {
        return Err(missing());
    }
    let actor = row.actor_id.ok_or_else(missing)?;
    crate::adapters::persistence::workflow::authority::authorize_company_with_locks(
        tx,
        CompanyId::new(authentication.company_id),
        WorkflowActor::authenticated(actor)?,
        locks,
    )
    .await?;
    commands::authorize_session_with_locks(
        tx,
        authentication.company_id,
        machine_id,
        actor,
        &operation,
        row.origin()?,
        locks,
    )
    .await?;
    let session = super::super::shared::session_on(
        tx,
        CompanyId::new(authentication.company_id),
        command_session(operation.command()).ok_or_else(missing)?,
    )
    .await?
    .ok_or_else(missing)?;
    let bundle = super::super::native::selected_with_locks_on(
        tx,
        authentication.company_id,
        actor,
        &session.harness,
        locks,
    )
    .await?
    .ok_or_else(missing)?;
    let fence = ConnectionFence {
        machine: MachineAuthentication {
            company_id: authentication.company_id,
            machine_id,
            installation_id: identity.installation_id,
            credential_generation: identity.credential_generation,
        },
        generation: identity.connection_generation,
        owner_id: authentication.owner,
    };
    lock_fence_with_locks(tx, fence, locks).await?;
    Ok(NativeAuthority {
        company: authentication.company_id,
        machine: machine_id,
        actor,
        operation,
        session,
        bundle,
    })
}
