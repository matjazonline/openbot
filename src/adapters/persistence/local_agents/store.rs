use super::*;

#[async_trait]
impl LocalMachineStore for PostgresPersistence {
    async fn create_pairing(&self, write: PairingCreate) -> AppResult<PairingAttempt> {
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, write.company_id, write.actor_id).await?;
        let active: i64 = sqlx::query_scalar("SELECT count(*) FROM local_pairings WHERE company_id=$1 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at>clock_timestamp()")
            .bind(write.company_id).fetch_one(&mut *tx).await?;
        if active >= MAX_PAIRING_CODES as i64 {
            return Err(AppError::Conflict(
                "five pairing codes are already outstanding".into(),
            ));
        }
        if let Some(machine) = write.replace_machine {
            revoke(&mut tx, write.company_id, machine).await?;
        }
        let row = sqlx::query_as::<_,PairingDb>(&format!("INSERT INTO local_pairings (id,company_id,actor_id,code_hash,expires_at) VALUES ($1,$2,$3,$4,clock_timestamp()+make_interval(secs=>$5)) RETURNING {PAIRING_COLUMNS}"))
            .bind(write.id.as_uuid()).bind(write.company_id).bind(write.actor_id).bind(write.code_hash.as_bytes().as_slice())
            .bind(PAIRING_TTL_SECONDS as f64).fetch_one(&mut *tx).await?;
        let result = row.projection()?;
        tx.commit().await?;
        Ok(result)
    }

    async fn list_pairings(
        &self,
        company: Uuid,
        after: Option<PairingId>,
    ) -> AppResult<Vec<PairingAttempt>> {
        let mut tx = begin(&self.pool).await?;
        let rows = sqlx::query_as::<_,PairingDb>(&format!("SELECT {PAIRING_COLUMNS} FROM local_pairings WHERE company_id=$1 AND ($2::uuid IS NULL OR id>$2) ORDER BY id LIMIT $3"))
            .bind(company).bind(after.map(PairingId::as_uuid)).bind(MAX_PAGE_RECORDS as i64).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        rows.iter().map(PairingDb::projection).collect()
    }

    async fn cancel_pairing(&self, actor: Uuid, company: Uuid, id: PairingId) -> AppResult<()> {
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, company, actor).await?;
        let changed = sqlx::query("UPDATE local_pairings SET revoked_at=COALESCE(revoked_at,clock_timestamp()) WHERE company_id=$1 AND id=$2 AND consumed_at IS NULL")
            .bind(company).bind(id.as_uuid()).execute(&mut *tx).await?;
        if changed.rows_affected() == 0 {
            return Err(missing());
        }
        tx.commit().await?;
        Ok(())
    }

    async fn reserve_pairing_budget(&self, keys: &[AuthAttemptKey]) -> AppResult<bool> {
        if keys.is_empty() || keys.len() > 3 {
            return Err(AppError::BadRequest("invalid pairing budget".into()));
        }
        let mut tx = begin(&self.pool).await?;
        // One short transaction serializes cardinality and each identity's fixed window.
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended('openbot.local-pair-budgets',0))",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM local_pair_budgets WHERE identity_key IN (SELECT identity_key FROM local_pair_budgets WHERE window_ends_at<=clock_timestamp() ORDER BY window_ends_at,identity_key LIMIT 100)")
            .execute(&mut *tx).await?;
        let mut admitted = true;
        for key in keys {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM local_pair_budgets WHERE identity_key=$1)",
            )
            .bind(key.as_bytes().as_slice())
            .fetch_one(&mut *tx)
            .await?;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM local_pair_budgets")
                .fetch_one(&mut *tx)
                .await?;
            if !exists && count >= 4096 {
                admitted = false;
                break;
            }
            let result = sqlx::query_scalar::<_,i16>("INSERT INTO local_pair_budgets (identity_key,attempts,window_ends_at) VALUES ($1,1,clock_timestamp()+INTERVAL '2 minutes') \
                ON CONFLICT (identity_key) DO UPDATE SET attempts=CASE WHEN local_pair_budgets.window_ends_at<=clock_timestamp() THEN 1 ELSE local_pair_budgets.attempts+1 END, \
                window_ends_at=CASE WHEN local_pair_budgets.window_ends_at<=clock_timestamp() THEN clock_timestamp()+INTERVAL '2 minutes' ELSE local_pair_budgets.window_ends_at END \
                WHERE local_pair_budgets.window_ends_at<=clock_timestamp() OR local_pair_budgets.attempts<12 RETURNING attempts")
                .bind(key.as_bytes().as_slice()).fetch_optional(&mut *tx).await?;
            if result.is_none() {
                admitted = false;
                break;
            }
        }
        // Refusals spend prior reservations too; a wrong code cannot roll its source budget back.
        tx.commit().await?;
        Ok(admitted)
    }

    async fn claim_pairing(&self, claim: &PairingClaim) -> AppResult<Option<PairingReceipt>> {
        claim.validate()?;
        let hash = claim.code.verifier();
        let mut tx = begin(&self.pool).await?;
        let company = sqlx::query_scalar::<_, Uuid>(
            "SELECT company_id FROM local_pairings WHERE code_hash=$1",
        )
        .bind(hash.as_bytes().as_slice())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(company) = company else {
            return Ok(None);
        };
        let locked =
            sqlx::query_scalar::<_, Uuid>("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
                .bind(company)
                .fetch_optional(&mut *tx)
                .await?;
        if locked.is_none() {
            return Ok(None);
        }
        let row = sqlx::query_as::<_,PairingDb>(&format!("SELECT {PAIRING_COLUMNS} FROM local_pairings WHERE company_id=$1 AND code_hash=$2 FOR UPDATE"))
            .bind(company).bind(hash.as_bytes().as_slice()).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.revoked_at.is_some() {
            return Ok(None);
        }
        if row.consumed_at.is_some() {
            if !row.matches(claim) {
                return Ok(None);
            }
            let result = receipt(
                &mut tx,
                company,
                row.machine_id.ok_or_else(|| invalid("consumed pairing"))?,
            )
            .await?;
            tx.commit().await?;
            return Ok(result);
        }
        let Some(actor) = row.actor_id else {
            return Ok(None);
        };
        match authorize(&mut tx, company, actor).await {
            Ok(()) => {}
            Err(AppError::NotFound(_) | AppError::Forbidden(_)) => return Ok(None),
            Err(error) => return Err(error),
        }
        let live: bool = sqlx::query_scalar("SELECT $1::timestamptz>clock_timestamp()")
            .bind(row.expires_at)
            .fetch_one(&mut *tx)
            .await?;
        if !live {
            return Ok(None);
        }
        let machine = Uuid::new_v4();
        if !insert_claim(&mut tx, company, machine, row.id, claim).await? {
            return Ok(None);
        }
        let result = receipt(&mut tx, company, machine).await?;
        tx.commit().await?;
        Ok(result)
    }

    async fn pairing_status(
        &self,
        installation: InstallationId,
        claim: ClaimVerifier,
    ) -> AppResult<Option<PairingReceipt>> {
        let mut tx = begin(&self.pool).await?;
        let row = sqlx::query_as::<_,(Uuid,Uuid)>("SELECT company_id,machine_id FROM local_pairings WHERE installation_id=$1 AND claim_verifier=$2 AND consumed_at IS NOT NULL AND revoked_at IS NULL")
            .bind(installation.as_uuid()).bind(claim.as_bytes().as_slice()).fetch_optional(&mut *tx).await?;
        let result = match row {
            Some((company, machine)) => receipt(&mut tx, company, machine).await?,
            None => None,
        };
        tx.commit().await?;
        Ok(result)
    }

    async fn list_machines(
        &self,
        company: Uuid,
        after: Option<MachineId>,
    ) -> AppResult<Vec<LocalMachine>> {
        let mut tx = begin(&self.pool).await?;
        let rows=sqlx::query_as::<_,MachineDb>(&format!("SELECT {MACHINE_COLUMNS} FROM local_machines AS machine WHERE company_id=$1 AND ($2::uuid IS NULL OR id>$2) ORDER BY id LIMIT $3"))
            .bind(company).bind(after.map(MachineId::as_uuid)).bind(MAX_PAGE_RECORDS as i64).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        rows.into_iter().map(MachineDb::entity).collect()
    }

    async fn change_machine(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
        change: MachineChange,
    ) -> AppResult<LocalMachine> {
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, company, actor).await?;
        let row = lock_machine(&mut tx, company, id).await?;
        match change {
            MachineChange::Revoke => revoke(&mut tx, company, id).await?,
            MachineChange::Enable | MachineChange::Disable => {
                if row.state == "revoked" {
                    return Err(AppError::Conflict(
                        "revoked machines require a new pairing".into(),
                    ));
                }
                let state = match change {
                    MachineChange::Enable => "enabled",
                    _ => "disabled",
                };
                sqlx::query("UPDATE local_machines SET state=$3 WHERE company_id=$1 AND id=$2")
                    .bind(company)
                    .bind(id.as_uuid())
                    .bind(state)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        let result = lock_machine(&mut tx, company, id).await?.entity()?;
        tx.commit().await?;
        Ok(result)
    }

    async fn set_projects(
        &self,
        actor: Uuid,
        company: Uuid,
        id: MachineId,
        projects: &[CloudroomProjectId],
    ) -> AppResult<()> {
        let unique: std::collections::HashSet<_> = projects.iter().collect();
        if projects.len() > MAX_PROJECTS || unique.len() != projects.len() {
            return Err(AppError::BadRequest("invalid project selection".into()));
        }
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, company, actor).await?;
        if lock_machine(&mut tx, company, id).await?.state == "revoked" {
            return Err(missing());
        }
        if !projects.is_empty() {
            let catalog = current_catalog(&mut tx, company, id).await?;
            if projects
                .iter()
                .any(|project| !catalog.projects.contains(project))
            {
                return Err(AppError::BadRequest("project is not advertised".into()));
            }
        }
        sqlx::query("DELETE FROM local_machine_projects WHERE company_id=$1 AND machine_id=$2")
            .bind(company)
            .bind(id.as_uuid())
            .execute(&mut *tx)
            .await?;
        let values: Vec<&str> = projects.iter().map(CloudroomProjectId::as_str).collect();
        sqlx::query("INSERT INTO local_machine_projects (company_id,machine_id,project_id) SELECT $1,$2,project FROM unnest($3::text[]) AS project")
            .bind(company).bind(id.as_uuid()).bind(values).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn save_profile(&self, actor: Uuid, profile: &LocalAgentProfile) -> AppResult<()> {
        validate_name(&profile.name)?;
        let mut tx = begin(&self.pool).await?;
        authorize(&mut tx, profile.company_id, actor).await?;
        let selection = &profile.selection;
        let machine = lock_machine(&mut tx, profile.company_id, selection.machine_id).await?;
        if machine.state != "enabled" {
            return Err(AppError::Conflict("machine is not enabled".into()));
        }
        let allowed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM local_machine_projects WHERE company_id=$1 AND machine_id=$2 AND project_id=$3)")
            .bind(profile.company_id).bind(selection.machine_id.as_uuid()).bind(selection.project_id.as_str()).fetch_one(&mut *tx).await?;
        if !allowed {
            return Err(AppError::BadRequest("project is not enabled".into()));
        }
        current_catalog(&mut tx, profile.company_id, selection.machine_id)
            .await?
            .validate_selection(selection)
            .map_err(|error| AppError::BadRequest(error.into()))?;
        sqlx::query("INSERT INTO local_agent_profiles (company_id,id,machine_id,name,selection) VALUES ($1,$2,$3,$4,$5) \
            ON CONFLICT (company_id,id) DO UPDATE SET machine_id=EXCLUDED.machine_id,name=EXCLUDED.name,selection=EXCLUDED.selection")
            .bind(profile.company_id).bind(profile.id.as_uuid()).bind(selection.machine_id.as_uuid()).bind(&profile.name).bind(encode(selection)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn acquire_connection(
        &self,
        verifier: CredentialVerifier,
        installation: InstallationId,
        owner: Uuid,
    ) -> AppResult<Option<ConnectionFence>> {
        if owner.is_nil() {
            return Err(AppError::BadRequest("invalid connection owner".into()));
        }
        let mut tx = begin(&self.pool).await?;
        let machine = sqlx::query_as::<_,MachineDb>(&format!("SELECT {MACHINE_COLUMNS} FROM local_machines AS machine JOIN local_machine_credentials AS credential \
            ON credential.company_id=machine.company_id AND credential.machine_id=machine.id AND credential.generation=machine.credential_generation \
            WHERE credential.verifier=$1 AND credential.revoked_at IS NULL AND machine.installation_id=$2 AND machine.state<>'revoked' FOR UPDATE OF machine"))
            .bind(verifier.as_bytes().as_slice()).bind(installation.as_uuid()).fetch_optional(&mut *tx).await?;
        let Some(machine) = machine else {
            return Ok(None);
        };
        let generation = machine
            .connection_generation
            .checked_add(1)
            .ok_or_else(|| AppError::Conflict("connection generation exhausted".into()))?;
        let entity = machine.entity()?;
        sqlx::query("UPDATE local_machines SET last_connection_issue=NULL,connection_generation=$3,connection_owner=$4,connection_expires_at=clock_timestamp()+make_interval(secs=>$5),last_seen_at=clock_timestamp() WHERE company_id=$1 AND id=$2")
            .bind(entity.company_id).bind(entity.id.as_uuid()).bind(generation).bind(owner).bind(CONNECTION_STALE_SECONDS as f64).execute(&mut *tx).await?;
        let fence = ConnectionFence {
            machine: MachineAuthentication {
                company_id: entity.company_id,
                machine_id: entity.id,
                installation_id: entity.installation_id,
                credential_generation: entity.credential_generation,
            },
            generation: ConnectionGeneration::try_from(generation as u64)
                .map_err(|_| invalid("connection generation"))?,
            owner_id: owner,
        };
        tx.commit().await?;
        Ok(Some(fence))
    }

    async fn renew_connection(&self, fence: ConnectionFence) -> AppResult<bool> {
        let mut tx = begin(&self.pool).await?;
        let result = sqlx::query("UPDATE local_machines AS machine SET connection_expires_at=clock_timestamp()+make_interval(secs=>$7),last_seen_at=clock_timestamp() \
            WHERE company_id=$1 AND id=$2 AND installation_id=$3 AND credential_generation=$4 AND connection_generation=$5 AND connection_owner=$6 \
            AND connection_expires_at>clock_timestamp() AND state<>'revoked' \
            AND EXISTS(SELECT 1 FROM local_machine_credentials AS credential WHERE credential.company_id=machine.company_id AND credential.machine_id=machine.id AND credential.generation=machine.credential_generation AND credential.revoked_at IS NULL)")
            .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid())
            .bind(fence.machine.credential_generation.get() as i64).bind(fence.generation.get() as i64).bind(fence.owner_id).bind(CONNECTION_STALE_SECONDS as f64)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(result.rows_affected() == 1)
    }

    async fn release_connection(&self, fence: ConnectionFence) -> AppResult<()> {
        let mut tx = begin(&self.pool).await?;
        sqlx::query("UPDATE local_machines SET connection_owner=NULL,connection_expires_at=NULL WHERE company_id=$1 AND id=$2 AND installation_id=$3 AND credential_generation=$4 AND connection_generation=$5 AND connection_owner=$6")
            .bind(fence.machine.company_id).bind(fence.machine.machine_id.as_uuid()).bind(fence.machine.installation_id.as_uuid())
            .bind(fence.machine.credential_generation.get() as i64).bind(fence.generation.get() as i64).bind(fence.owner_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

async fn insert_claim(
    db: &mut PgConnection,
    company: Uuid,
    machine: Uuid,
    pairing: Uuid,
    claim: &PairingClaim,
) -> AppResult<bool> {
    let reserved = sqlx::query("INSERT INTO local_installations (company_id,installation_id) VALUES ($1,$2) ON CONFLICT (installation_id) DO NOTHING")
        .bind(company).bind(claim.installation_id.as_uuid()).execute(&mut *db).await?;
    if reserved.rows_affected() == 0 {
        return Ok(false);
    }
    sqlx::query(
        "INSERT INTO local_machines (id,company_id,installation_id,name) VALUES ($1,$2,$3,$4)",
    )
    .bind(machine)
    .bind(company)
    .bind(claim.installation_id.as_uuid())
    .bind(&claim.name)
    .execute(&mut *db)
    .await?;
    let credential = sqlx::query("INSERT INTO local_machine_credentials (company_id,machine_id,generation,verifier) VALUES ($1,$2,1,$3) ON CONFLICT (verifier) DO NOTHING")
        .bind(company).bind(machine).bind(claim.credential_verifier.as_bytes().as_slice()).execute(&mut *db).await?;
    if credential.rows_affected() == 0 {
        return Ok(false);
    }
    let consumed = sqlx::query("UPDATE local_pairings SET consumed_at=clock_timestamp(),installation_id=$3,machine_id=$4,claim_verifier=$5,credential_verifier=$6 \
        WHERE company_id=$1 AND id=$2 AND consumed_at IS NULL AND revoked_at IS NULL AND expires_at>clock_timestamp()")
        .bind(company).bind(pairing).bind(claim.installation_id.as_uuid()).bind(machine).bind(claim.claim_verifier.as_bytes().as_slice())
        .bind(claim.credential_verifier.as_bytes().as_slice()).execute(db).await;
    match consumed {
        Ok(result) => Ok(result.rows_affected() == 1),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => Ok(false),
        Err(error) => Err(error.into()),
    }
}
