# D1/r2 correction inspection

Inspection only; no behavior executed. Reuses R1 retained source evidence.

## src/adapters/persistence/account_identity.rs:215-278
SHA-256 2c31a8839149eef5562b60067beac8b1e80e49cdb6dd381fb7b1a32a8a4e07c7
```text
215: async fn adopt_outsider_places(
216:     connection: &mut sqlx::PgConnection,
217:     company_id: Uuid,
218:     person: PrincipalId,
219:     outsider: Uuid,
220: ) -> AppResult<()> {
221:     let still_reachable: bool = sqlx::query_scalar(
222:         r#"SELECT EXISTS (
223:                SELECT 1 FROM participant_identities
224:                WHERE company_id = $1 AND principal_id = $2
225:            )"#,
226:     )
227:     .bind(company_id)
228:     .bind(outsider)
229:     .fetch_one(&mut *connection)
230:     .await
231:     .map_err(AppError::from)?;
232:     if still_reachable {
233:         return Ok(());
234:     }
235:     // A grant the person already holds for the same channel, capability and provenance keeps the
236:     // person's row.
237:     sqlx::query(
238:         r#"INSERT INTO channel_principal_grants
239:                (company_id, channel_id, principal_id, capability, provenance, created_at)
240:            SELECT company_id, channel_id, $3, capability, provenance, created_at
241:            FROM channel_principal_grants
242:            WHERE company_id = $1 AND principal_id = $2
243:            ON CONFLICT (company_id, channel_id, principal_id, capability, provenance) DO NOTHING"#,
244:     )
245:     .bind(company_id)
246:     .bind(outsider)
247:     .bind(person.as_uuid())
248:     .execute(&mut *connection)
249:     .await
250:     .map_err(AppError::from)?;
251:     sqlx::query("DELETE FROM channel_principal_grants WHERE company_id = $1 AND principal_id = $2")
252:         .bind(company_id)
253:         .bind(outsider)
254:         .execute(&mut *connection)
255:         .await
256:         .map_err(AppError::from)?;
257:     sqlx::query(
258:         r#"INSERT INTO thread_principals
259:                (company_id, channel_id, thread_id, principal_id, role, created_at)
260:            SELECT company_id, channel_id, thread_id, $3, role, created_at
261:            FROM thread_principals
262:            WHERE company_id = $1 AND principal_id = $2
263:            ON CONFLICT (company_id, channel_id, thread_id, principal_id, role) DO NOTHING"#,
264:     )
265:     .bind(company_id)
266:     .bind(outsider)
267:     .bind(person.as_uuid())
268:     .execute(&mut *connection)
269:     .await
270:     .map_err(AppError::from)?;
271:     sqlx::query("DELETE FROM thread_principals WHERE company_id = $1 AND principal_id = $2")
272:         .bind(company_id)
273:         .bind(outsider)
274:         .execute(&mut *connection)
275:         .await
276:         .map_err(AppError::from)?;
277:     Ok(())
278: }
```

## src/adapters/persistence/workflow/member_dependents.rs:6-41
SHA-256 391e1a1e48050e730329c9c98af5f768f3794141201a6c096545a2ca5c8bab84
```text
6: pub(crate) async fn prelock_account_companies_on(
7:     db: &mut PgConnection,
8:     user: Uuid,
9: ) -> AppResult<Vec<CompanyId>> {
10:     let companies: Vec<Uuid> = sqlx::query_scalar(
11:         "SELECT company.id FROM companies AS company WHERE company.user_id=$1 \
12:          OR EXISTS (SELECT 1 FROM company_members AS member WHERE member.company_id=company.id AND member.user_id=$1) \
13:          OR EXISTS (SELECT 1 FROM principals AS principal WHERE principal.company_id=company.id AND principal.user_id=$1 AND principal.kind='person') \
14:          ORDER BY company.id FOR NO KEY UPDATE",
15:     )
16:     .bind(user)
17:     .fetch_all(db)
18:     .await?;
19:     Ok(companies.into_iter().map(CompanyId::new).collect())
20: }
21: 
22: /// Refresh final proof/grants under the same account/company ownership; no recipient accounts.
23: pub(crate) async fn refresh_account_dependents_on(
24:     db: &mut PgConnection,
25:     companies: &[CompanyId],
26: ) -> AppResult<()> {
27:     for company in companies {
28:         Box::pin(refresh_member_dependents_on(db, *company)).await?;
29:     }
30:     Ok(())
31: }
32: 
33: /// Called after the final membership/account state, under the already-owned company lock.
34: pub(crate) async fn refresh_member_dependents_on(
35:     db: &mut PgConnection,
36:     company: CompanyId,
37: ) -> AppResult<()> {
38:     Box::pin(follow_reviewers::refresh_on(db, company)).await?;
39:     Box::pin(crate::adapters::persistence::notification::refresh_person_escalations_on(db, company))
40:         .await
41: }
```

## src/adapters/persistence/notification/projection_lock.rs:15-75
SHA-256 dd294546bfedc0f3c4cbd543cff36c2db0b3e349866a3ba4811721894117941a
```text
15: pub(super) async fn lock_on(
16:     db: &mut PgConnection,
17:     event: &NotificationEvent,
18:     purpose: ProjectionPurpose,
19: ) -> AppResult<LockedProjection> {
20:     let preview = resolve_projection_on(db, event).await?;
21:     let mut accounts = Vec::with_capacity(2);
22:     if let ProjectionPurpose::Open { user_id } = purpose {
23:         accounts.push(user_id);
24:     }
25:     if let NotificationDisposition::Active(recipient) = &preview.disposition {
26:         accounts.push(recipient.user_id);
27:     }
28:     accounts.sort_unstable();
29:     accounts.dedup();
30:     let locked: Vec<Uuid> =
31:         sqlx::query_scalar("SELECT id FROM users WHERE id=ANY($1) ORDER BY id FOR SHARE")
32:             .bind(&accounts)
33:             .fetch_all(&mut *db)
34:             .await?;
35:     sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
36:         .bind(event.company_id)
37:         .execute(&mut *db)
38:         .await?;
39:     if locked != accounts {
40:         return Ok(LockedProjection::Stale);
41:     }
42:     match event.source_kind {
43:         NotificationSourceKind::Delivery => {
44:             sqlx::query(
45:                 "SELECT id FROM message_deliveries WHERE company_id=$1 AND id=$2 FOR SHARE",
46:             )
47:             .bind(event.company_id)
48:             .bind(event.source_id)
49:             .execute(&mut *db)
50:             .await?;
51:         }
52:         NotificationSourceKind::DecisionReviewer => {
53:             super::reviewer_notice::lock_source_on(db, event).await?
54:         }
55:         NotificationSourceKind::EscalationControl => {
56:             sqlx::query(
57:                 "SELECT id FROM workflow_escalation_cases WHERE company_id=$1 AND id=$2 FOR UPDATE",
58:             )
59:             .bind(event.company_id)
60:             .bind(event.source_id)
61:             .execute(&mut *db)
62:             .await?;
63:         }
64:         NotificationSourceKind::EscalationNotice => {
65:             super::escalation_notice::lock_source_on(db, event).await?
66:         }
67:     }
68:     sqlx::query("SELECT user_id FROM user_notification_preferences WHERE user_id=ANY($1) ORDER BY user_id FOR SHARE")
69:         .bind(&accounts).execute(&mut *db).await?;
70:     let current = resolve_projection_on(db, event).await?;
71:     if current != preview {
72:         return Ok(LockedProjection::Stale);
73:     }
74:     Ok(LockedProjection::Current(Box::new(current)))
75: }
```

