import hashlib,json,subprocess,os
from pathlib import Path
p=Path('plan/01-knowledge-access-levels/phases/p01/docs/logs')
expected=hashlib.sha384(Path('migrations/20260817000000_init_schema.sql').read_bytes()).hexdigest()
query="""SELECT json_build_object('database',current_database(),'user',current_user,'port',current_setting('port'),'data_directory',current_setting('data_directory'),'server_version',current_setting('server_version'),'checksum',(SELECT encode(checksum,'hex') FROM _sqlx_migrations WHERE success AND version=20260817000000),'level_columns',(SELECT count(*) FROM information_schema.columns WHERE table_schema='public' AND table_name IN ('company_members','company_invites') AND column_name='access_level' AND domain_name='access_level' AND is_nullable='NO'),'domain_constraint',(SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE contypid='public.access_level'::regtype AND contype='c'),'owner_constraint',(SELECT pg_get_constraintdef(oid) FROM pg_constraint WHERE conrelid='public.company_members'::regclass AND contype='c' AND pg_get_constraintdef(oid) LIKE '%access_level%'))"""
rows=[]
for db in ('open_bots','open_bots_test'):
 row=json.loads(subprocess.check_output(['/opt/homebrew/opt/postgresql@18/bin/psql','-X','-A','-t','-h','127.0.0.1','-p','55501','-U','mac02','-d',db,'-v','ON_ERROR_STOP=1','-c',query],text=True,env=dict(os.environ,PGOPTIONS='-c statement_timeout=5000')))
 assert row['port']=='55501' and row['user']=='mac02'
 assert row['data_directory']=='/Users/mac02/dev/openbot-recovery/.worktrees/pg-knowledge-p01'
 assert row['checksum']==expected and row['level_columns']==2
 assert '9' in row['owner_constraint'] and '9' in row['domain_constraint']
 rows.append(row)
(p/'WB1-r3-resource-identity.json').write_text(json.dumps({'expected_init_sha384':expected,'databases':rows},indent=2)+'\n')
print({'resource_checks':'PASS','database_count':len(rows),'init_sha384':expected})
