//! Scoped provisioning/schedule fixture shared by the A2 regression cases.
use super::*;

pub(super) struct Links {
    pub(super) task: Uuid,
    pub(super) agent: Uuid,
    pub(super) thread: Uuid,
    pub(super) schedule: Uuid,
    pub(super) moved_channel: Uuid,
    pub(super) foreign_company: Uuid,
    pub(super) foreign_channel: Uuid,
    pub(super) foreign_agent: Uuid,
    pub(super) foreign_task: Uuid,
}

impl Links {
    pub(super) async fn seed(f: &JobFixture, db: &mut PgConnection) -> Self {
        let company = f.binding.target.company.as_uuid();
        let task = legacy(f, db).await;
        let agent = agent(f, db).await;
        let thread = Uuid::new_v4();
        fixtures::insert_record(
            db,
            "threads",
            &json!({"id":thread,"company_id":company,"channel_id":f.channel,"subject":"Schedule"}),
        )
        .await
        .unwrap();
        sqlx::query("UPDATE background_tasks SET thread_id = $2 WHERE id = $1")
            .bind(task)
            .bind(thread)
            .execute(&mut *db)
            .await
            .unwrap();
        let schedule = Uuid::new_v4();
        fixtures::insert_record(db, "channel_schedules", &json!({"id":schedule,"company_id":company,"channel_id":f.channel,"name":"Schedule","schedule_type":"one_off","subject_template":"Subject","prompt_template":"Prompt"})).await.unwrap();
        let moved_channel = Uuid::new_v4();
        fixtures::insert_record(db,"channels",&json!({"id":moved_channel,"company_id":company,"name":"Moved","enabled":false,"created_by":{"actor_type":"system","actor_id":null,"actor_name":"Test"}})).await.unwrap();
        let foreign_company = Uuid::new_v4();
        sqlx::query("INSERT INTO companies (id, user_id, name, slug) SELECT $2, user_id, 'Foreign', $2::text FROM companies WHERE id = $1").bind(company).bind(foreign_company).execute(&mut *db).await.unwrap();
        let foreign_channel = Uuid::new_v4();
        let foreign_agent = Uuid::new_v4();
        let provenance = json!({"actor_type":"system","actor_id":null,"actor_name":"Test"});
        fixtures::insert_record(db,"channels",&json!({"id":foreign_channel,"company_id":foreign_company,"name":"Foreign","enabled":false,"created_by":provenance})).await.unwrap();
        fixtures::insert_record(db,"agents",&json!({"id":foreign_agent,"company_id":foreign_company,"name":"Foreign","slug":"foreign","created_by":provenance})).await.unwrap();
        let foreign_task = Uuid::new_v4();
        fixtures::insert_record(db,"background_tasks",&json!({"id":foreign_task,"company_id":foreign_company,"channel_id":foreign_channel,"correlation_id":foreign_task,"task_type":"inbound_email"})).await.unwrap();
        Self {
            task,
            agent,
            thread,
            schedule,
            moved_channel,
            foreign_company,
            foreign_channel,
            foreign_agent,
            foreign_task,
        }
    }

    pub(super) fn provision(&self, f: &JobFixture) -> serde_json::Value {
        json!({"task_id":self.task,"request_hash":"request","agent_id":self.agent,"channel_id":f.channel})
    }

    pub(super) fn run(&self) -> serde_json::Value {
        json!({"id":Uuid::new_v4(),"schedule_id":self.schedule,"scheduled_for":chrono::Utc::now(),"schedule_snapshot":{},"thread_id":self.thread,"task_id":self.task,"materialization_status":"materialized"})
    }
}
