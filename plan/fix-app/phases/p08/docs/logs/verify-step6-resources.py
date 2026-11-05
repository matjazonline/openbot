from pathlib import Path
import hashlib,json,subprocess
p=Path('plan/fix-app/phases/p08/docs/logs')
expected=hashlib.sha384(Path('migrations/20260817000000_init_schema.sql').read_bytes()).hexdigest()
result={'expected_init_sha384':expected,'databases':[]}
for name in ['open_bots','open_bots_test']:
 url=f'postgres://mac02@127.0.0.1:55408/{name}'
 def query(sql):return subprocess.check_output(['/opt/homebrew/opt/postgresql@18/bin/psql',url,'-X','-v','ON_ERROR_STOP=1','-Atqc',sql],text=True).strip()
 identity=json.loads(query("SELECT json_build_object('database',current_database(),'user',current_user,'port',current_setting('port'),'data_directory',current_setting('data_directory'),'checksum',(SELECT encode(checksum,'hex') FROM _sqlx_migrations WHERE version=20260817000000 AND success))"))
 assert identity['database']==name and identity['user']=='mac02' and identity['port']=='55408' and identity['data_directory'].endswith('/.worktrees/pg-p08')
 assert identity['checksum']==expected
 retired=int(query("SELECT count(*) FROM pg_attribute WHERE NOT attisdropped AND ((attrelid='background_tasks'::regclass AND attname IN ('task_type','payload','queue_kind','last_error')) OR (attrelid='task_harness_runs'::regclass AND attname IN ('owner_kind','final_output_consumed_at')))"));assert retired==0
 required=query("SELECT attnotnull FROM pg_attribute WHERE attrelid='background_tasks'::regclass AND attname='workflow_execution_id'");assert required=='t'
 triggers=json.loads(query("SELECT json_agg(json_build_object('name',tgname,'definition',pg_get_triggerdef(oid))) FROM pg_trigger WHERE NOT tgisinternal AND tgrelid IN ('background_tasks'::regclass,'task_harness_runs'::regclass)"))
 for t in triggers:assert all(c not in t['definition'] for c in ['queue_kind','owner_kind','payload','task_type'])
 result['databases'].append(dict(identity=identity,retired_columns=retired,workflow_execution_id_required=True,triggers=triggers))
(p/'step6-resource-migration-identity.json').write_text(json.dumps(result,indent=2)+'\n')
print('Both private database identities, checksums, removed columns, required execution and trigger definitions PASS')
