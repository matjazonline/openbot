from pathlib import Path
import re,json
ROOT=Path('.')
def change(p,old,new,count=None):
 s=p.read_text(); n=s.count(old); assert n and (count is None or n==count),(str(p),old,n,count); p.write_text(s.replace(old,new))
p=Path('migrations/20260817000000_init_schema.sql');s=p.read_text()
for line in ["    task_type text NOT NULL,\n","    payload jsonb DEFAULT '{}'::jsonb NOT NULL,\n","    queue_kind text DEFAULT 'workflow'::text NOT NULL,\n","    owner_kind text NOT NULL DEFAULT 'workflow',\n"]:
 assert s.count(line)==1,(line,s.count(line));s=s.replace(line,'')
s=s.replace('    workflow_execution_id uuid,\n','    workflow_execution_id uuid NOT NULL,\n',1)
for name in ['background_tasks_payload_object_check','background_tasks_queue_kind_check','background_tasks_workflow_shape_check','task_harness_runs_token_owner']:
 s,n=re.subn(r'    CONSTRAINT '+name+r'[^\n]*\n','',s);assert n==1,name
s=s.replace("    CONSTRAINT background_tasks_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'processing'::text, 'completed'::text, 'failed'::text]))),", "    CONSTRAINT background_tasks_status_check CHECK ((status = ANY (ARRAY['pending'::text, 'processing'::text, 'completed'::text, 'failed'::text])))")
s=s.replace("        IF NEW.queue_kind<>'workflow' THEN RETURN NULL; END IF;\n",'')
s=s.replace("    IF NEW.owner_kind<>'workflow' THEN RETURN NEW; END IF;\n",'')
s=s.replace("    IF NEW.owner_kind <> 'workflow' THEN\n        RAISE EXCEPTION 'unknown harness owner' USING ERRCODE='23514';\n    END IF;\n",'')
s=s.replace('(NEW.queue_kind, NEW.workflow_execution_id) IS DISTINCT FROM\n       (OLD.queue_kind, OLD.workflow_execution_id)','NEW.workflow_execution_id IS DISTINCT FROM OLD.workflow_execution_id')
s=s.replace('NEW.agent_id,NEW.owner_kind,NEW.workflow_run_id','NEW.agent_id,NEW.workflow_run_id').replace('OLD.agent_id,OLD.owner_kind,OLD.workflow_run_id','OLD.agent_id,OLD.workflow_run_id')
s=s.replace("OR (owner_kind='workflow'\n            AND num_nonnulls",'OR (num_nonnulls')
s=s.replace("        owner_kind = 'workflow'\n            AND workflow_attempt > 0",'        workflow_attempt > 0')
s=s.replace(" WHERE (queue_kind = 'workflow'::text)", '').replace(" WHERE owner_kind='workflow'",'')
s=s.replace('UPDATE OF queue_kind, workflow_execution_id','UPDATE OF workflow_execution_id')
s=s.replace('workflow_execution_id, queue_kind ON','workflow_execution_id ON')
s=s.replace(" WHEN ((new.queue_kind = 'workflow'::text))",'')
s=s.replace(" WHEN (NEW.owner_kind='workflow' AND OLD.model_request_id IS NULL AND NEW.model_request_id IS NOT NULL)"," WHEN (OLD.model_request_id IS NULL AND NEW.model_request_id IS NOT NULL)")
s=s.replace(" WHEN (NEW.owner_kind='workflow')",'')
p.write_text(s)
# Remove only tautological predicates. All identity/fence/transition clauses survive.
pattern=r"(?:[A-Za-z_]+\.)?(?:queue_kind|owner_kind)\s*=\s*'workflow'(?:::\s*text)?"
paths=[p]+list(Path('src').rglob('*.rs'))
predicate_counts={}
for p in paths:
 s=p.read_text();before=s
 s=re.sub(r'\bAND\s+'+pattern,s and '',s)
 s=re.sub(pattern+r'\s+AND\s+','',s)
 s=re.sub(r'\bWHERE\s+'+pattern+r'(?=\s*(?:[)";]|FOR\b|GROUP\b|ORDER\b|LIMIT\b))','',s)
 if s!=before:p.write_text(s);predicate_counts[str(p)]=len(before)-len(s)
# Balanced SQL lists: remove a constant column and exactly its value together.
def split_list(text):
 result=[];start=0;depth=0;quoted=False;i=0
 while i<len(text):
  c=text[i]
  if c=="'":
   if quoted and i+1<len(text) and text[i+1]=="'":i+=2;continue
   quoted=not quoted
  elif not quoted:
   if c=='(':depth+=1
   elif c==')':depth-=1
   elif c==',' and depth==0:result.append(text[start:i].strip());start=i+1
  i+=1
 result.append(text[start:].strip());return result
insert_changes=[]
for p in Path('src').rglob('*.rs'):
 if p.name=='job_schema_tests.rs':continue
 s=p.read_text()
 def rewrite(m):
  sql=m.group()[1:-1];sql=re.sub(r'\\\n\s*',' ',sql)
  head=re.match(r'(INSERT INTO (?:public\.)?(background_tasks|task_harness_runs)\s*\()([^)]*)(\)\s*)(VALUES\s*\(|SELECT\s+)',sql,re.I)
  if not head:return m.group()
  columns=split_list(head[3]); remove=[i for i,v in enumerate(columns) if v in {'task_type','queue_kind','payload','owner_kind'}]
  if not remove:return m.group()
  start=head.end();tail=sql[start:]
  if head[5].startswith('VALUES'):
   depth=1;quoted=False;end=None
   for i,c in enumerate(tail):
    if c=="'":quoted=not quoted
    elif not quoted:
     if c=='(':depth+=1
     elif c==')':
      depth-=1
      if depth==0:end=i;break
   assert end is not None
  else:
   found=re.search(r'\bFROM\b',tail,re.I);assert found;end=found.start()
  values=split_list(tail[:end]);assert len(columns)==len(values),(p,columns,values)
  removed_params=set()
  for i in remove:
   if re.fullmatch(r'\$\d+',values[i]):removed_params.add(int(values[i][1:]))
  new_values=[v for i,v in enumerate(values) if i not in remove]
  newsql=head[1]+', '.join(v for i,v in enumerate(columns) if i not in remove)+head[4]+head[5]+', '.join(new_values)+(' ' if head[5].startswith('SELECT') else '')+tail[end:]
  for n in removed_params:
   assert not re.search(r'\$'+str(n)+r'\b',newsql),(p,n,newsql)
  newsql=re.sub(r'\$(\d+)',lambda q:'$'+str(int(q[1])-sum(n<int(q[1]) for n in removed_params)),newsql)
  insert_changes.append(dict(file=str(p),removed_columns=[columns[i] for i in remove],removed_parameters=sorted(removed_params),sql=newsql))
  return '"'+newsql+'"'
 s=re.sub(r'"INSERT INTO (?:public\.)?(?:background_tasks|task_harness_runs)\b(?:\\.|[^"\\])*"',rewrite,s)
 s=re.sub(r'\s*\.bind\((?:crate::application::workflow::activation::)?job_payload\([^()]*\)\)','',s)
 if s!=p.read_text():p.write_text(s)
Path('plan/fix-app/phases/p08/docs/logs/step6-transform-report.json').write_text(json.dumps(dict(predicates=predicate_counts,inserts=insert_changes),indent=2)+'\n')
# Runtime readers use the exact native execution identity already fenced in SQL.
b=Path('src/adapters/persistence/workflow')
for name in ['lease.rs','batch.rs']:
 p=b/name;s=p.read_text();s=s.replace('    pub payload: Value,\n','').replace('    payload: Value,\n','').replace('lock_expires_at,payload,$4','lock_expires_at,$4').replace('SELECT payload, status,','SELECT status,')
 s=re.sub(r'    if decode_job_payload\(job.payload(?:\.clone\(\))?\)\? != (?:scope|request).execution \{\n        return (?:Ok\(None\)|Err\(invalid\(\)\))\;\n    }\n','',s)
 p.write_text(s)
p=b/'activation.rs';s=p.read_text();s=s.replace('let payload: Value = sqlx::query_scalar(', 'let _: Uuid = sqlx::query_scalar(').replace('SELECT payload FROM background_tasks','SELECT workflow_execution_id FROM background_tasks');s=s.replace('    if decode_job_payload(payload)? != request.execution {\n        return Err(invalid());\n    }\n','');p.write_text(s)
p=b/'completion.rs';s=p.read_text().replace('let payload: Option<Value>','let execution: Option<Uuid>').replace('SELECT payload FROM background_tasks','SELECT workflow_execution_id FROM background_tasks').replace('let Some(payload) = payload','let Some(_) = execution');s=s.replace('    if decode_job_payload(payload)? != scope.execution {\n        return Ok(false);\n    }\n','');p.write_text(s)
p=b/'waits.rs';s=p.read_text().replace('let payload:Option<Value>','let execution:Option<Uuid>').replace('SELECT payload FROM background_tasks','SELECT workflow_execution_id FROM background_tasks');s=s.replace('''    match payload {
        Some(payload) => {
            if decode_job_payload(payload)? == scope.execution {
                Ok(run)
            } else {
                Ok(None)
            }
        }
        _ => Ok(None),
    }''','''    Ok(execution.and(run))''');p.write_text(s)
p=Path('src/application/workflow/activation.rs');s=p.read_text();start=s.index('#[derive(Serialize, Deserialize)]');end=s.index('/// Include optional/defaulted',start);s=s[:start]+s[end:];s=s.replace('use serde::{Deserialize, Serialize};\n','');start=s.index('\n#[cfg(test)]\nmod tests {');s=s[:start]+'\n';p.write_text(s)
# A dashboard has no use for the retired type discriminator.
for p in [Path('src/domain/entities/dashboard.rs'),Path('src/adapters/persistence/dashboard.rs'),Path('src/adapters/http/pages/dashboard.rs')]:
 s=p.read_text();s=re.sub(r'^\s*(?:pub task_type: String,|task_type: row.try_get\("task_type"\).map_err\(AppError::from\)\?,|task_type: "email_agent_dispatch".into\(\),|task.task_type,)\n','',s,flags=re.M);s=s.replace("figures are the workflow queue's (`queue_kind = 'workflow'`) and the tokens are the model usage","figures are the workflow queue's and the tokens are the model usage");p.write_text(s)
# Native harness fixtures use required ownership, not a deleted kind column.
p=b/'harness_owner_tests.rs';s=p.read_text().replace(',"owner_kind":"workflow"','').replace('        ("owner_kind", json!("legacy")),\n','').replace('        "owner_kind=\'legacy\'",\n','');p.write_text(s)
p=b/'action_reconciliation_claim_sql_candidate_support.rs';s=p.read_text();s=s.replace('''            assert_eq!(
                added["payload"],
                crate::application::workflow::activation::job_payload(scope.execution)
            );''','''            assert_eq!(added["workflow_execution_id"], json!(scope.execution.as_uuid()));''');p.write_text(s)
p=b/'job_schema_tests.rs';s=p.read_text();s=s.replace('''    async fn insert(
        &self,
        db: &mut PgConnection,
        payload: serde_json::Value,
    )''','''    async fn insert(&self, db: &mut PgConnection)''');s=s.replace("(id, company_id, channel_id, correlation_id, task_type, queue_kind, workflow_execution_id, payload, thread_id) VALUES ($1, $2, $3, $4, 'workflow_execution', 'workflow', $5, $6, $7)","(id, company_id, channel_id, correlation_id, workflow_execution_id, thread_id) VALUES ($1, $2, $3, $4, $5, $6)").replace('.bind(self.execution).bind(payload).bind(self.thread)','.bind(self.execution).bind(self.thread)');start=s.index('    fn payload(&self)');end=s.index('    async fn hypothetical',start);s=s[:start]+s[end:];s=s.replace('f.insert(&mut db, f.payload())','f.insert(&mut db)').replace('f.insert(&mut tx, f.payload())','f.insert(&mut tx)')
start=s.index('#[tokio::test]\nasync fn workflow_job_schema_enforces_exact_ids_and_scoped_execution');s=s[:start]+'''#[tokio::test]
async fn workflow_job_schema_enforces_exact_ids_and_scoped_execution() {
    let mut f = JobFixture::new().await;
    let original = f.execution;
    f.execution = Uuid::new_v4();
    let mut tx = f.hypothetical().await;
    constraint(f.insert(&mut tx).await.unwrap_err(), "background_tasks_workflow_execution_fk");
    tx.rollback().await.unwrap();
    f.execution = original;
    let mut tx = f.hypothetical().await;
    let id = f.insert(&mut tx).await.unwrap();
    let saved: Uuid = sqlx::query_scalar("SELECT workflow_execution_id FROM background_tasks WHERE id=$1")
        .bind(id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(saved, f.execution);
    let error = sqlx::query("UPDATE background_tasks SET workflow_execution_id=$2 WHERE id=$1")
        .bind(id).bind(Uuid::new_v4()).execute(&mut *tx).await.unwrap_err();
    assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("23514"));
    assert!(error.to_string().contains("queue identity is immutable"));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn workflow_job_schema_rejects_foreign_execution_and_missing_owner() {
    let f = JobFixture::new().await;
    let p = &f.binding.fixture.persistence;
    let company = CompanyPersistence::create(p, f.binding.target.actor.user_id(), CompanyWrite {
        name: "Other".into(), slug: "other".into(), ..Default::default()
    }).await.unwrap();
    let channel = ChannelPersistence::create(p, company.id, ChannelWrite {
        name: "Other".into(), slug: "other".into(), ..Default::default()
    }).await.unwrap();
    let mut tx = f.hypothetical().await;
    let error = sqlx::query("INSERT INTO background_tasks (id,company_id,channel_id,correlation_id,workflow_execution_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4()).bind(company.id).bind(channel.id).bind(Uuid::new_v4())
        .bind(f.execution).execute(&mut *tx).await.unwrap_err();
    constraint(error, "background_tasks_workflow_execution_fk");
    tx.rollback().await.unwrap();
    let required: bool = sqlx::query_scalar("SELECT attnotnull FROM pg_attribute WHERE attrelid='background_tasks'::regclass AND attname='workflow_execution_id'")
        .fetch_one(p.pool()).await.unwrap();
    assert!(required);
    let retired: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_attribute WHERE NOT attisdropped AND ((attrelid='background_tasks'::regclass AND attname IN ('task_type','payload','queue_kind')) OR (attrelid='task_harness_runs'::regclass AND attname='owner_kind'))")
        .fetch_one(p.pool()).await.unwrap();
    assert_eq!(retired, 0);
    let mut tx = f.hypothetical().await;
    let id = f.insert(&mut tx).await.unwrap();
    let error = sqlx::query("UPDATE background_tasks SET workflow_execution_id=NULL WHERE id=$1")
        .bind(id).execute(&mut *tx).await.unwrap_err();
    assert!(error.to_string().contains("queue identity is immutable"));
    tx.rollback().await.unwrap();
}
''';p.write_text(s)
# The shared JobFixture no longer accepts a redundant payload.
for p in Path('src/adapters/persistence/workflow').rglob('*.rs'):
 s=p.read_text().replace('f.insert(&mut db, f.payload())','f.insert(&mut db)').replace('f.insert(&mut tx, f.payload())','f.insert(&mut tx)')
 p.write_text(s)
# Keep the association rejection aimed at a real foreign execution, rather than a removed kind.
p=b/'job_association_tests.rs';s=p.read_text().replace("(id, company_id, correlation_id, task_type) VALUES (gen_random_uuid(), $1, gen_random_uuid(), 'inbound_email')", "(id, company_id, correlation_id, workflow_execution_id) VALUES (gen_random_uuid(), $1, gen_random_uuid(), gen_random_uuid())").replace('// Every job is a workflow job now (p08 D4.8), so one naming no execution is refused as one.','// A job must name an execution owned by this company.');p.write_text(s)
