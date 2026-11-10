from pathlib import Path
p=Path('migrations/20260817000000_init_schema.sql');s=p.read_text()
def section(table, change):
 global s
 a=s.index('CREATE TABLE public.'+table+' (');b=s.index('\n);',a);part=s[a:b];new=change(part);assert new!=part,table;s=s[:a]+new+s[b:]
def live(part):
 for field in ('workflow_id','version_id','run_id','execution_id','root_run_id'):
  old='    '+field+' uuid,';assert part.count(old)==1;part=part.replace(old,'    '+field+' uuid NOT NULL,')
 old='    CHECK (num_nonnulls(workflow_id, version_id, run_id, execution_id, root_run_id) = 5),\n';assert old in part;return part.replace(old,'')
section('model_usage_calls',live)
old='CREATE INDEX model_usage_calls_run ON public.model_usage_calls(company_id,run_id) WHERE run_id IS NOT NULL;';assert old in s;s=s.replace(old,'CREATE INDEX model_usage_calls_run ON public.model_usage_calls(company_id,run_id);')
old='AND NEW.run_id IS NOT NULL AND task.workflow_execution_id=NEW.execution_id';assert s.count(old)==1;s=s.replace(old,'AND task.workflow_execution_id=NEW.execution_id')
def harness(part):
 for field,kind in [('workflow_run_id','uuid'),('workflow_execution_id','uuid'),('workflow_generation','uuid'),('workflow_worker_id','uuid'),('workflow_attempt','integer'),('workflow_prepared','jsonb')]:
  old='    '+field+' '+kind+',';assert part.count(old)==1;part=part.replace(old,'    '+field+' '+kind+' NOT NULL,')
 old='''            AND num_nonnulls(workflow_run_id,workflow_execution_id,workflow_generation,
                workflow_worker_id,workflow_attempt,workflow_prepared) = 6
''';assert old in part;return part.replace(old,'')
section('task_harness_runs',harness)
section('users',lambda part:part.replace('    password_hash text NOT NULL,','    password_hash text,'))
p.write_text(s)
for name in ('src/domain/entities/user.rs','src/adapters/persistence/user.rs'):
 p=Path(name);s=p.read_text();assert s.count('pub password_hash: String,')==1;p.write_text(s.replace('pub password_hash: String,','pub password_hash: Option<String>,'))
p=Path('src/adapters/persistence/user.rs');s=p.read_text();a=s.index('    async fn create_external_user(');b=s.index('\n    async fn get_by_external_subject',a);part=s[a:b]
old='''        // Not an Argon2 hash, and password authentication checks the method table before parsing.
        let password_hash = format!("{}-only:{id}", identity.provider.as_str());
''';assert old in part;part=part.replace(old,'');part=part.replace('VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP)','VALUES ($1, $2, $3, NULL, CURRENT_TIMESTAMP)').replace('        .bind(password_hash)\n','');p.write_text(s[:a]+part+s[b:])
p=Path('src/application/use_cases/user/mod.rs');s=p.read_text();old='''        let is_valid = self
            .verify_password(password.expose_secret(), &user.password_hash, work_lifetime)
            .await?;''';new='''        let Some(hash) = user.password_hash.as_deref() else {
            self.verify_password(password.expose_secret(), DUMMY_PASSWORD_HASH, work_lifetime)
                .await?;
            return Err(AppError::InvalidCredentials);
        };
        let is_valid = self
            .verify_password(password.expose_secret(), hash, work_lifetime)
            .await?;''';assert old in s;p.write_text(s.replace(old,new))
p=Path('src/application/use_cases/user/profile.rs');s=p.read_text();old='''        if !self
            .verify_password(
                change.current.expose_secret(),
                &user.password_hash,
                work_lifetime.clone(),
            )''';new='''        let hash = user.password_hash.as_deref().ok_or(AppError::InvalidCredentials)?;
        if !self
            .verify_password(
                change.current.expose_secret(),
                hash,
                work_lifetime.clone(),
            )''';assert old in s;p.write_text(s.replace(old,new))
p=Path('src/adapters/persistence/model_usage/report.rs');s=p.read_text();assert 'workflow_id: Option<Uuid>,' in s;p.write_text(s.replace('workflow_id: Option<Uuid>,','workflow_id: Uuid,'))
p=Path('src/application/services/model_usage/aggregate.rs');s=p.read_text();s=s.replace('pub workflow: Option<Uuid>,','pub workflow: Uuid,').replace('Workflow(Option<Uuid>),','Workflow(Uuid),').replace('query.workflow.is_some_and(|id| Some(id) != call.workflow)','query.workflow.is_some_and(|id| id != call.workflow)').replace('GroupKey::Workflow(workflow) => group.workflow = workflow,','GroupKey::Workflow(workflow) => group.workflow = Some(workflow),').replace('            workflow: None,','            workflow: Uuid::nil(),').replace('row.workflow = Some(Uuid::from_u128(index + 1));','row.workflow = Uuid::from_u128(index + 1);');p.write_text(s)
p=Path('plan/fix-app/08-schema-cleanup.md');s=p.read_text();old='''  and drop the redundant `run_id IS NOT NULL` from its index and guard trigger. If 06 decided to
  keep usage rows after their runs are deleted, skip this item and record why.''';new='''  and drop the redundant `run_id IS NOT NULL` from its index and guard trigger. Phase 06 retains
  usage in separate immutable `model_usage_history`, without run/execution/root/job references;
  this tightening applies to live calls and must not be skipped (coordination C010).''';assert old in s;p.write_text(s.replace(old,new))
print('Required ownership and nullable password core changes applied; fixtures/checks next')
