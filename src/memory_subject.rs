//! Operator-only entry point. DATABASE_URL and the existing managed provisioner
//! credential are its authority; this command is never exposed as a model tool.
use open_bots::{
    adapters::{memory_service::MemoryServiceHttp, persistence::PostgresPersistence},
    application::memory::{MemoryProvisioner, MemorySubjectScope, TenantExternalId},
    domain::workflow::CompanyId,
    entities::transport::PrincipalId,
    infra::memory_config::MemoryServiceConfig,
};
use serde::Serialize;
use std::io::Write;
use std::time::Duration;

const COMMAND_DEADLINE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectOperation {
    List,
    Purge,
}

struct SubjectCommand {
    operation: SubjectOperation,
    company: CompanyId,
    subject: PrincipalId,
}

#[derive(Default, Serialize)]
struct Progress {
    namespaces: u64,
    listed: u64,
    deleted: u64,
}

pub async fn run(
    persistence: &PostgresPersistence,
    operation: SubjectOperation,
    company: CompanyId,
    subject: PrincipalId,
) -> anyhow::Result<()> {
    let config = MemoryServiceConfig::from_env()
        .map_err(anyhow::Error::msg)?
        .ok_or_else(|| anyhow::anyhow!("Memory service is not configured"))?;
    let service = MemoryServiceHttp::new(&config)?;
    let mut output = std::io::BufWriter::new(std::io::stdout());
    let mut progress = Progress::default();
    let result = tokio::time::timeout(
        COMMAND_DEADLINE,
        walk(
            persistence,
            &service,
            SubjectCommand {
                operation,
                company,
                subject,
            },
            &mut progress,
            &mut output,
        ),
    )
    .await;
    let error = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error.to_string()),
        Err(_) => Some("command deadline reached".into()),
    };
    writeln!(
        &mut output,
        "{}",
        serde_json::json!({"company":company.as_uuid(),"subject":subject.as_uuid(),
        "complete":error.is_none(),"progress":progress,"error":error})
    )?;
    output.flush()?;
    if error.is_some() {
        anyhow::bail!(
            "Subject cleanup incomplete; rerun the same command to finish remaining records"
        );
    }
    Ok(())
}

async fn walk(
    persistence: &PostgresPersistence,
    service: &dyn MemoryProvisioner,
    command: SubjectCommand,
    progress: &mut Progress,
    output: &mut impl Write,
) -> anyhow::Result<()> {
    let mut after = None;
    loop {
        let namespaces = persistence
            .memory_unit_namespace_page(command.company, after)
            .await?;
        if namespaces.is_empty() {
            return Ok(());
        }
        for namespace in namespaces {
            walk_namespace(
                service,
                command.operation,
                MemorySubjectScope {
                    tenant: TenantExternalId(command.company.as_uuid()),
                    namespace,
                    subject: command.subject,
                },
                progress,
                output,
            )
            .await?;
            progress.namespaces += 1;
            after = Some(namespace.0);
        }
    }
}

async fn walk_namespace(
    service: &dyn MemoryProvisioner,
    operation: SubjectOperation,
    scope: MemorySubjectScope,
    progress: &mut Progress,
    output: &mut impl Write,
) -> anyhow::Result<()> {
    let mut after = None;
    loop {
        let page = service.list_subject(scope, after).await?;
        progress.listed += page.items.len() as u64;
        let ids: Vec<_> = page.items.iter().map(|item| item.observation_id).collect();
        match operation {
            SubjectOperation::List => writeln!(
                output,
                "{}",
                serde_json::json!({"namespace":scope.namespace,"items":page.items})
            )?,
            SubjectOperation::Purge if !ids.is_empty() => {
                let deleted = service.delete_subject(scope, &ids).await?;
                progress.deleted += deleted.len() as u64;
                writeln!(
                    output,
                    "{}",
                    serde_json::json!({"namespace":scope.namespace,"deleted":deleted})
                )?;
            }
            SubjectOperation::Purge => {}
        }
        let Some(next) = page.next_after else {
            return Ok(());
        };
        after = Some(next);
    }
}
