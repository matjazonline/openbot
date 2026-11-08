#!/usr/bin/env python3
import datetime, difflib, hashlib, json, os, subprocess, tarfile, tempfile, time
from pathlib import Path
root=Path.cwd(); logs=Path(__file__).resolve().parent
m=json.loads((logs/'CHS-P03-B1-R1-manifest.json').read_text())
container=Path(tempfile.mkdtemp(prefix='openbot-chs-p03-review-delete-'))
worktree=container/'worktree'
setup=subprocess.run(['git','worktree','add','--detach',str(worktree),m['head']],cwd=root,text=True,capture_output=True,check=True)
(logs/'review-delete-setup.log').write_text(setup.stdout+setup.stderr)
with tarfile.open(logs/m['source_archive']) as archive: archive.extractall(worktree,filter='data')
for name in m['deleted_files']:
    f=worktree/name
    if f.exists(): f.unlink()
for name,want in m['source_files'].items():
    f=worktree/name; actual=hashlib.sha256(f.read_bytes()).hexdigest() if f.exists() else None
    assert actual==want,name
file=worktree/'src/adapters/persistence/workflow/channel_reply_intent_schema_tests.rs'
before=file.read_text()
addition=r"""

#[tokio::test]
async fn channel_reply_review_diagnostic_suppressed_decision_delete() {
    let f = FinalFixture::new("channel.no_reply", false).await;
    f.advance().await;
    let before = f.state().await;
    let deletion = sqlx::query("DELETE FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid())
        .execute(f.p.pool()).await;
    let after = f.state().await;
    eprintln!("suppressed deletion={deletion:?} before={before} after={after}");
    assert!(deletion.is_err(), "succeeded root lost its durable suppression: {after}");
}

#[tokio::test]
async fn channel_reply_review_diagnostic_failed_decision_delete() {
    let f = FinalFixture::new("data.map", false).await;
    f.advance().await;
    let claim = f.p.claim_replies(ReplyPublicationWorker(Uuid::new_v4()), ReplyPublicationPage::try_from(1).unwrap()).await.unwrap()[0];
    assert!(f.p.release_reply(claim, ReplyPublicationRetry::Terminal(ReplyPublicationFailure::AuthorityRevoked)).await.unwrap());
    let before = f.state().await;
    let deletion = sqlx::query("DELETE FROM channel_reply_decisions WHERE company_id=$1 AND run_id=$2")
        .bind(f.scope.company.as_uuid()).bind(f.scope.run.as_uuid())
        .execute(f.p.pool()).await;
    let after = f.state().await;
    eprintln!("failed deletion={deletion:?} before={before} after={after}");
    assert!(deletion.is_err(), "succeeded root lost its durable failed publication: {after}");
}
"""
file.write_text(before+addition)
(logs/'review-delete-diagnostic.diff').write_text(''.join(difflib.unified_diff(before.splitlines(True),(before+addition).splitlines(True),fromfile='a/'+str(file.relative_to(worktree)),tofile='b/'+str(file.relative_to(worktree)))))
env=os.environ.copy();env.pop('ALLOW_MISSING_DATABASE_URL',None)
settings={'DATABASE_URL':'postgres://mac02@127.0.0.1:65037/openbot_chs','TEST_DATABASE_URL':'postgres://mac02@127.0.0.1:65037/openbot_chs_test','SQLX_OFFLINE':'true','CARGO_INCREMENTAL':'1','CARGO_BUILD_JOBS':'4','RUST_MIN_STACK':'2097152','CARGO_TARGET_DIR':str(root/'target')}
env.update(settings)
command=['scripts/test-network-isolation.sh','cargo','test','--locked','--offline','--lib','channel_reply_review_diagnostic_','--','--nocapture']
start=datetime.datetime.now(datetime.timezone.utc).isoformat(); clock=time.monotonic()
resource={'worktree':str(worktree),'container':str(container),'preserve_until_review_complete':True,'source_digest':m['digest'],'diagnostic_patch':'review-delete-diagnostic.diff','database_pair':'openbot_chs/openbot_chs_test','target':str(root/'target')}
(logs/'review-delete-resources.json').write_text(json.dumps(resource,indent=2)+'\n')
with (logs/'review-delete-tests-01.log').open('w') as stream:
    result=subprocess.run(command,cwd=worktree,env=env,stdout=stream,stderr=subprocess.STDOUT)
receipt={**resource,'command':command,'environment':settings,'started':start,'seconds':round(time.monotonic()-clock,3),'exit_code':result.returncode,'log':'review-delete-tests-01.log'}
(logs/'review-delete-tests-01.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt));print('\n'.join((logs/'review-delete-tests-01.log').read_text().splitlines()[-45:]))
