use super::*;
use crate::adapters::persistence::test_support::{own_database, wait_until_a_backend_is_blocked};
use crate::application::workflow::WorkflowActor;
use crate::domain::workflow::TypeName;
use crate::entities::skill_bundle::SkillFile;

#[derive(Clone, Copy)]
enum CopyPath {
    Standalone,
    LibraryAgent,
}

struct CopyRequest {
    company: Uuid,
    actor: Uuid,
    source: Skill,
    library_agent: Option<Uuid>,
}

fn write(slug: &str, revision: &str) -> SkillWrite {
    let mut write = skill_write(slug);
    write.instructions = format!("Instructions {revision}");
    write.files = Some(vec![
        SkillFile::from_bytes("scripts/check.sh".into(), revision.as_bytes(), true).unwrap(),
    ]);
    write.required_tools = Some(vec![TypeName::parse(format!("tool_{revision}")).unwrap()]);
    write
}

async fn request(p: &PostgresPersistence, path: CopyPath) -> CopyRequest {
    let company = company(p, "coherent-copy").await;
    let actor = sqlx::query_scalar("SELECT user_id FROM companies WHERE id=$1")
        .bind(company)
        .fetch_one(p.pool())
        .await
        .unwrap();
    let source = SkillManagementPersistence::create_library(p, write("coherent-copy", "a"))
        .await
        .unwrap();
    let library_agent = match path {
        CopyPath::Standalone => None,
        CopyPath::LibraryAgent => {
            let mut input = agent_write("library-copy");
            input.skill_ids = vec![source.id];
            Some(AgentPersistence::create_library(p, input).await.unwrap().id)
        }
    };
    CopyRequest {
        company,
        actor,
        source,
        library_agent,
    }
}

async fn copy(p: &PostgresPersistence, request: &CopyRequest) -> Skill {
    if let Some(agent) = request.library_agent {
        let copied = p
            .create_agent_from_library(
                request.company,
                WorkflowActor::authenticated(request.actor).unwrap(),
                agent,
                agent_write("company-copy"),
            )
            .await
            .unwrap();
        p.load_capabilities(request.company, copied.id)
            .await
            .unwrap()
            .unwrap()
            .skills
            .remove(0)
    } else {
        p.copy_library_to_company(
            request.company,
            request.source.id,
            CreationProvenance::system(),
        )
        .await
        .unwrap()
    }
}

#[tokio::test]
async fn skill_bundle_competing_editor_and_both_copy_paths_keep_one_revision_during_slug_retries() {
    for path in [CopyPath::Standalone, CopyPath::LibraryAgent] {
        let Some(database) = own_database().await else {
            return;
        };
        let p = PostgresPersistence::new(database.pool.clone());
        let request = request(&p, path).await;
        let mut blocker = database.pool.begin().await.unwrap();
        insert_skill(
            &mut *blocker,
            Some(request.company),
            &skill_write("coherent-copy"),
        )
        .await
        .unwrap();
        let editor = async {
            // Observe the actual INSERT's unique-index lock wait after its initial source read.
            wait_until_a_backend_is_blocked(&database.pool).await;
            p.update_library(request.source.id, write("coherent-copy", "b"))
                .await
                .unwrap();
            blocker.commit().await.unwrap();
        };
        // Box the competing persistence seams so the test does not retain their SQL futures inline.
        let (copied, ()) = tokio::join!(Box::pin(copy(&p, &request)), Box::pin(editor));
        assert_eq!(copied.slug.as_str(), "coherent-copy-2");
        assert_eq!(copied.instructions, request.source.instructions);
        assert_eq!(copied.files, request.source.files);
        let tools: Vec<String> =
            sqlx::query_scalar("SELECT required_tools FROM skills WHERE company_id=$1 AND id=$2")
                .bind(request.company)
                .bind(copied.id)
                .fetch_one(&database.pool)
                .await
                .unwrap();
        assert_eq!(tools, vec!["tool_a"]);
        let source = p.get_library(request.source.id).await.unwrap().unwrap();
        assert_eq!(source.instructions, "Instructions b");
        assert_eq!(source.files[0].bytes().unwrap(), b"b");
    }
}
