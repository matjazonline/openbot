# 03PA-P02-D1-R1 reviewer source inspection; 2026-11-14
Inspection evidence only; no behavior executed.

## src/adapters/persistence/company_invite.rs:571-659
File SHA-256: ea23974a422f363a7251860fac3061e4bd07903c20d6a77cebd00f764b43ed07
```text
571:     async fn remove_member(&self, company_id: Uuid, user_id: Uuid) -> AppResult<()> {
572:         let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
573:         // Match dispatch and consent: account, then company, then membership and
574:         // principal. Never acquire a principal before a competing host owner.
575:         sqlx::query("SET LOCAL lock_timeout='5s'")
576:             .execute(&mut *transaction)
577:             .await?;
578:         sqlx::query("SELECT id FROM users WHERE id=$1 FOR SHARE")
579:             .bind(user_id)
580:             .fetch_optional(&mut *transaction)
581:             .await?;
582:         sqlx::query("SELECT id FROM companies WHERE id=$1 FOR NO KEY UPDATE")
583:             .bind(company_id)
584:             .fetch_optional(&mut *transaction)
585:             .await?;
586:         let notice_recipient = super::delivery::cancellation::member_notice_recipient_on(
587:             &mut transaction,
588:             company_id,
589:             user_id,
590:         )
591:         .await?;
592:         sqlx::query!(
593:             r#"UPDATE principals
594:                SET kind = 'external', user_id = NULL, updated_at = CURRENT_TIMESTAMP
595:                WHERE company_id = $1 AND user_id = $2
596:                  AND EXISTS (
597:                      SELECT 1 FROM company_members AS member
598:                      WHERE member.company_id = $1 AND member.user_id = $2
599:                        AND member.role <> 'owner'
600:                  )"#,
601:             company_id,
602:             user_id
603:         )
604:         .execute(&mut *transaction)
605:         .await
606:         .map_err(AppError::from)?;
607:         let removable = sqlx::query_scalar!(
608:             "SELECT id FROM company_members
609:              WHERE company_id = $1 AND user_id = $2 AND role <> 'owner'
610:              FOR UPDATE",
611:             company_id,
612:             user_id
613:         )
614:         .fetch_optional(&mut *transaction)
615:         .await
616:         .map_err(AppError::from)?;
617:         if removable.is_none() {
618:             // An owner, or nobody: removal is a no-op, and so was the demotion above.
619:             transaction.commit().await.map_err(AppError::from)?;
620:             return Ok(());
621:         }
622:         super::delivery::cancellation::cancel_member_notices_on(
623:             &mut transaction,
624:             company_id,
625:             notice_recipient,
626:         )
627:         .await?;
628:         // After the membership lock (memory E9): the user's namespace in this company goes.
629:         super::memory::retire_namespaces_on(
630:             &mut transaction,
631:             company_id,
632:             crate::application::memory::NamespaceOwner::User(user_id),
633:         )
634:         .await?;
635:         super::integrations::lifecycle::revoke_membership_on(
636:             &mut transaction,
637:             &crate::adapters::integrations::owner::owner_for_user(user_id),
638:             &crate::adapters::integrations::owner::company_binding(company_id),
639:         )
640:         .await
641:         .map_err(AppError::from)?;
642:         sqlx::query!(
643:             "DELETE FROM company_members
644:              WHERE company_id = $1 AND user_id = $2 AND role <> 'owner'",
645:             company_id,
646:             user_id
647:         )
648:         .execute(&mut *transaction)
649:         .await
650:         .map_err(AppError::from)?;
651:         Box::pin(super::workflow::refresh_member_dependents_on(
652:             &mut transaction,
653:             crate::domain::workflow::CompanyId::new(company_id),
654:         ))
655:         .await?;
656:         transaction.commit().await.map_err(AppError::from)?;
657: 
658:         Ok(())
659:     }
```

## src/adapters/persistence/participant.rs:271-323
File SHA-256: 742970ac947cab5c936b6e6b5b5fc3dee578525df0928ff3a8a092671a15bf6d
```text
271: pub(crate) async fn resolve_or_create_external_identity_on(
272:     connection: &mut sqlx::PgConnection,
273:     company_id: Uuid,
274:     observation: IdentityObservation,
275: ) -> AppResult<IdentityResolution> {
276:     lock_identity_key(connection, company_id, &observation.identity).await?;
277: 
278:     if let Some(existing) = load_resolution(connection, company_id, &observation.identity).await? {
279:         return Ok(existing);
280:     }
281: 
282:     let principal_id = PrincipalId::random();
283:     let identity_id = ParticipantIdentityId::random();
284:     let label = checked_label(
285:         observation.display_label.clone(),
286:         observation.identity.subject().as_str(),
287:     )?;
288:     let claims = serde_json::to_value(&observation.claim_metadata)
289:         .map_err(|error| AppError::Internal(error.to_string()))?;
290: 
291:     sqlx::query(
292:         r#"INSERT INTO principals (id, company_id, kind, display_label)
293:            VALUES ($1, $2, 'external', $3)"#,
294:     )
295:     .bind(principal_id.as_uuid())
296:     .bind(company_id)
297:     .bind(&label)
298:     .execute(&mut *connection)
299:     .await
300:     .map_err(AppError::from)?;
301:     sqlx::query(
302:         r#"INSERT INTO participant_identities
303:                (id, company_id, principal_id, transport, namespace, subject, display_label,
304:                 status, claim_metadata, provenance)
305:            VALUES ($1, $2, $3, $4, $5, $6, $7, 'observed', $8, $9)"#,
306:     )
307:     .bind(identity_id.as_uuid())
308:     .bind(company_id)
309:     .bind(principal_id.as_uuid())
310:     .bind(observation.identity.transport().as_str())
311:     .bind(observation.identity.namespace().as_str())
312:     .bind(observation.identity.subject().as_str())
313:     .bind(observation.display_label)
314:     .bind(claims)
315:     .bind(observation.provenance.as_str())
316:     .execute(&mut *connection)
317:     .await
318:     .map_err(AppError::from)?;
319: 
320:     load_resolution(connection, company_id, &observation.identity)
321:         .await?
322:         .ok_or_else(|| AppError::Internal("Created identity was not found.".into()))
323: }
```

## src/adapters/persistence/participant.rs:331-413
File SHA-256: 742970ac947cab5c936b6e6b5b5fc3dee578525df0928ff3a8a092671a15bf6d
```text
331: pub(crate) async fn create_person_principal_on(
332:     connection: &mut sqlx::PgConnection,
333:     company_id: Uuid,
334:     user_id: Uuid,
335:     display_label: &str,
336:     email: &str,
337: ) -> AppResult<PrincipalId> {
338:     let qualified = account_email_identity(email)?;
339:     lock_identity_key(connection, company_id, &qualified).await?;
340: 
341:     let existing: Option<(Uuid, String, Option<Uuid>)> = sqlx::query_as(
342:         r#"SELECT principal.id, principal.kind, principal.user_id
343:            FROM participant_identities AS identity
344:            JOIN principals AS principal
345:              ON (principal.company_id, principal.id) =
346:                 (identity.company_id, identity.principal_id)
347:            WHERE identity.company_id = $1 AND identity.transport = 'email'
348:              AND identity.namespace = $2 AND identity.subject = $3
349:            FOR UPDATE OF principal, identity"#,
350:     )
351:     .bind(company_id)
352:     .bind(qualified.namespace().as_str())
353:     .bind(qualified.subject().as_str())
354:     .fetch_optional(&mut *connection)
355:     .await
356:     .map_err(AppError::from)?;
357: 
358:     let principal_id = match existing {
359:         Some((id, kind, _)) if kind == "external" => {
360:             sqlx::query(
361:                 r#"UPDATE principals
362:                    SET kind = 'person', user_id = $3, display_label = $4,
363:                        updated_at = CURRENT_TIMESTAMP
364:                    WHERE company_id = $1 AND id = $2"#,
365:             )
366:             .bind(company_id)
367:             .bind(id)
368:             .bind(user_id)
369:             .bind(display_label)
370:             .execute(&mut *connection)
371:             .await
372:             .map_err(AppError::from)?;
373:             PrincipalId::new(id)
374:         }
375:         Some((id, kind, Some(existing_user_id)))
376:             if kind == "person" && existing_user_id == user_id =>
377:         {
378:             PrincipalId::new(id)
379:         }
380:         Some((_, kind, existing_user_id)) => {
381:             return Err(AppError::Conflict(format!(
382:                 "Email identity is already attached to a {kind} principal ({existing_user_id:?})."
383:             )));
384:         }
385:         None => {
386:             let id = PrincipalId::random();
387:             sqlx::query(
388:                 r#"INSERT INTO principals
389:                        (id, company_id, kind, user_id, display_label)
390:                    VALUES ($1, $2, 'person', $3, $4)"#,
391:             )
392:             .bind(id.as_uuid())
393:             .bind(company_id)
394:             .bind(user_id)
395:             .bind(display_label)
396:             .execute(&mut *connection)
397:             .await
398:             .map_err(AppError::from)?;
399:             id
400:         }
401:     };
402: 
403:     upsert_account_identity_on(
404:         connection,
405:         company_id,
406:         principal_id,
407:         &qualified,
408:         display_label,
409:     )
410:     .await?;
411:     ensure_person_address_on(connection, company_id, principal_id, display_label).await?;
412:     Ok(principal_id)
413: }
```

## src/adapters/persistence/account_identity.rs:52-145
File SHA-256: 2c31a8839149eef5562b60067beac8b1e80e49cdb6dd381fb7b1a32a8a4e07c7
```text
52: pub(crate) async fn rekey_account_identity_on(
53:     connection: &mut sqlx::PgConnection,
54:     user_id: Uuid,
55:     old_email: &str,
56:     new_email: &str,
57: ) -> AppResult<()> {
58:     let companies = super::workflow::prelock_account_companies_on(connection, user_id).await?;
59:     let persons: Vec<PersonPrincipal> = sqlx::query_as(
60:         r#"SELECT company_id, id FROM principals
61:            WHERE user_id = $1 AND kind = 'person'
62:            ORDER BY company_id"#,
63:     )
64:     .bind(user_id)
65:     .fetch_all(&mut *connection)
66:     .await
67:     .map_err(AppError::from)?;
68:     if persons.is_empty() {
69:         return Ok(());
70:     }
71:     // Registration accepts any plausible address, so the old one may be one no identity was ever
72:     // written for; there is then nothing of it to detach. The new one becomes an identity, so it
73:     // must parse, exactly as it must when a company is created.
74:     let old = account_email_identity(old_email).ok();
75:     let new = account_email_identity(new_email)?;
76:     if old.as_ref() == Some(&new) {
77:         // A confirmed same-address change can install previously missing proof.
78:         return super::workflow::refresh_account_dependents_on(connection, &companies).await;
79:     }
80:     let username: String = sqlx::query_scalar("SELECT username::text FROM users WHERE id = $1")
81:         .bind(user_id)
82:         .fetch_one(&mut *connection)
83:         .await
84:         .map_err(AppError::from)?;
85: 
86:     // Both identity locks are taken in one order everywhere, so two re-keys that touch the same
87:     // pair of addresses cannot wait on each other crosswise.
88:     let mut keys: Vec<&QualifiedIdentity> = old.iter().chain([&new]).collect();
89:     keys.sort_by(|left, right| left.subject().as_str().cmp(right.subject().as_str()));
90: 
91:     for person in persons {
92:         for key in &keys {
93:             lock_identity_key(connection, person.company_id, key).await?;
94:         }
95:         let principal = PrincipalId::new(person.id);
96:         if let Some(old) = &old {
97:             detach_old_address(connection, person.company_id, principal, old).await?;
98:         }
99:         adopt_new_address(connection, person.company_id, principal, &new, &username).await?;
100:     }
101:     super::workflow::refresh_account_dependents_on(connection, &companies).await
102: }
103: 
104: async fn identity_holder(
105:     connection: &mut sqlx::PgConnection,
106:     company_id: Uuid,
107:     identity: &QualifiedIdentity,
108: ) -> AppResult<Option<IdentityHolder>> {
109:     sqlx::query_as(
110:         r#"SELECT identity.id AS identity_id, principal.id AS principal_id, principal.kind
111:            FROM participant_identities AS identity
112:            JOIN principals AS principal
113:              ON (principal.company_id, principal.id) =
114:                 (identity.company_id, identity.principal_id)
115:            WHERE identity.company_id = $1 AND identity.transport = $2
116:              AND identity.namespace = $3 AND identity.subject = $4
117:            FOR UPDATE OF principal, identity"#,
118:     )
119:     .bind(company_id)
120:     .bind(identity.transport().as_str())
121:     .bind(identity.namespace().as_str())
122:     .bind(identity.subject().as_str())
123:     .fetch_optional(&mut *connection)
124:     .await
125:     .map_err(AppError::from)
126: }
127: 
128: /// Hand the old address to a new outsider principal, with the messages it sent.
129: ///
130: /// The identity and its messages move in one statement: `messages_authored_identity_author_fk`
131: /// requires a message's author to hold its authored identity, and it is checked at the end of the
132: /// statement, so two separate updates would each violate it.
133: async fn detach_old_address(
134:     connection: &mut sqlx::PgConnection,
135:     company_id: Uuid,
136:     person: PrincipalId,
137:     old: &QualifiedIdentity,
138: ) -> AppResult<()> {
139:     let Some(holder) = identity_holder(connection, company_id, old).await? else {
140:         return Ok(());
141:     };
142:     if holder.principal_id != person.as_uuid() {
143:         // Already somebody else's (an earlier change, or an outsider): nothing of ours to detach.
144:         return Ok(());
145:     }
```

## src/adapters/persistence/thread/mod.rs:235-275
File SHA-256: 74ac4636c9459bf2a61d3224f1d055790c871ece20c0594ad86bb14431cb7d38
```text
235: pub(super) async fn insert_thread_principals(
236:     transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
237:     company_id: Uuid,
238:     channel_id: Uuid,
239:     thread_id: Uuid,
240:     intents: &[ThreadPrincipalIntent],
241: ) -> AppResult<()> {
242:     let mut seen = HashSet::new();
243:     for intent in intents {
244:         if !seen.insert((intent.identity.clone(), intent.role)) {
245:             continue;
246:         }
247:         let provenance = IdentityProvenance::TransportIngress;
248:         let resolved = resolve_or_create_external_identity_on(
249:             transaction,
250:             company_id,
251:             IdentityObservation {
252:                 identity: intent.identity.clone(),
253:                 display_label: None,
254:                 claim_metadata: IdentityClaimMetadata::observation(),
255:                 provenance,
256:             },
257:         )
258:         .await?;
259:         sqlx::query(
260:             r#"INSERT INTO thread_principals
261:                    (company_id, channel_id, thread_id, principal_id, role)
262:                VALUES ($1, $2, $3, $4, $5)
263:                ON CONFLICT DO NOTHING"#,
264:         )
265:         .bind(company_id)
266:         .bind(channel_id)
267:         .bind(thread_id)
268:         .bind(resolved.principal.id.as_uuid())
269:         .bind(intent.role.as_str())
270:         .execute(&mut **transaction)
271:         .await
272:         .map_err(AppError::from)?;
273:     }
274:     Ok(())
275: }
```

## src/adapters/persistence/thread/inbound.rs:115-160
File SHA-256: 03bad590e5899377e6fb84a2a73cfdb57c09f261d9b4d65f97a82e6e0e20a216
```text
115: async fn commit_as_on(
116:     tx: &mut Transaction<'_, Postgres>,
117:     request: &InboundCommitRequest,
118:     author: InboundAuthor,
119: ) -> AppResult<InboundCommitOutcome> {
120:     if request.associations.is_empty() {
121:         return Err(AppError::BadRequest(
122:             "An inbound commit must name at least one thread association".into(),
123:         ));
124:     }
125:     hold_channel_settings(tx, request).await?;
126:     let binding_ids = verify_association_bindings(tx, request).await?;
127:     lock_provider_keys(tx, request, &binding_ids).await?;
128: 
129:     // Asked first, and under every relevant lock. A redelivery must open no thread: the old path
130:     // created the thread before it knew whether the message was new, so a provider retry left an
131:     // empty conversation behind and then failed associating the message it already held.
132:     let existing = existing_message_mappings(tx, request, &binding_ids).await?;
133:     if !existing.is_empty() {
134:         return recognise_redelivery(tx, request, existing, author).await;
135:     }
136: 
137:     let threads = resolve_threads(tx, request).await?;
138:     bind_conversations(tx, request, &threads).await?;
139: 
140:     let write = message_write(request, threads[0].thread_id, author)?;
141:     let stored = store_canonical(tx, request, &write).await?;
142:     prove_internal_relay(tx, request, stored.id).await?;
143: 
144:     associate_threads(tx, request, &threads, stored.id).await?;
145:     map_provider_message(tx, request, &threads, &stored).await?;
146:     // After the associations, whose rows the intents' foreign key names, and before the fan-out:
147:     // keeping the statement order of the whole commit fixed is what stops two concurrent inbound
148:     // commits on one thread taking their locks in opposite orders.
149:     let intent_ids = create_intents(tx, request, &threads, stored.id).await?;
150:     let delivery_ids = create_deliveries(tx, request).await?;
151:     complete_claimed_event(tx, request).await?;
152: 
153:     Ok(InboundCommitOutcome {
154:         disposition: CommitDisposition::Created,
155:         message_id: stored.id,
156:         thread_ids: threads.iter().map(|thread| thread.thread_id).collect(),
157:         intent_ids,
158:         delivery_ids,
159:     })
160: }
```

## src/adapters/persistence/user_lifecycle.rs:74-129
File SHA-256: efe86f7b67bb44a6db8a456a293c53d5424058288fd5ba85a67a18eb805b77ee
```text
74: pub(in crate::adapters::persistence) async fn finish_account_deletion_on(
75:     tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
76:     user: Uuid,
77:     locks: AccountLocks,
78: ) -> AppResult<()> {
79:     let AccountLocks {
80:         owned: companies,
81:         memberships,
82:     } = locks;
83:     for company in &memberships {
84:         let company = *company;
85:         lifecycle::revoke_membership_on(&mut *tx, &owner_for_user(user), &company_binding(company))
86:             .await
87:             .map_err(AppError::from)?;
88:         let notice_recipient =
89:             super::delivery::cancellation::member_notice_recipient_on(tx, company, user).await?;
90:         super::delivery::cancellation::cancel_member_notices_on(tx, company, notice_recipient)
91:             .await?;
92:         super::memory::retire_namespaces_on(
93:             &mut *tx,
94:             company,
95:             crate::application::memory::NamespaceOwner::User(user),
96:         )
97:         .await?;
98:     }
99:     sqlx::query("UPDATE principals SET kind='external',user_id=NULL,updated_at=clock_timestamp() WHERE user_id=$1")
100:         .bind(user).execute(&mut **tx).await.map_err(AppError::from)?;
101:     lifecycle::retire_owner_on(tx, &owner_for_user(user))
102:         .await
103:         .map_err(AppError::from)?;
104:     for company in companies {
105:         lifecycle::revoke_company_on(tx, &client_namespace(), &company_binding(company))
106:             .await
107:             .map_err(AppError::from)?;
108:         super::memory::retire_company_on(&mut *tx, company).await?;
109:         sqlx::query("DELETE FROM companies WHERE id=$1 AND user_id=$2")
110:             .bind(company)
111:             .bind(user)
112:             .execute(&mut **tx)
113:             .await
114:             .map_err(AppError::from)?;
115:     }
116:     sqlx::query("DELETE FROM users WHERE id=$1")
117:         .bind(user)
118:         .execute(&mut **tx)
119:         .await
120:         .map_err(AppError::from)?;
121:     for company in memberships {
122:         Box::pin(super::workflow::refresh_member_dependents_on(
123:             tx,
124:             crate::domain::workflow::CompanyId::new(company),
125:         ))
126:         .await?;
127:     }
128:     Ok(())
129: }
```

## src/adapters/persistence/notification/projection.rs:91-151
File SHA-256: 5aa65ba5888e72e040aa1681201aa610281fdba0fe2f116c402d19c3174c8f8d
```text
91: 
92: /// What a notification event is about (p08 D4.3). After B4 the only source is a workflow
93: /// delivery: it resolves to its run through the destination, and its run actor is responsible
94: /// while they are a person of the company. A delivery with no run (a person's mailbox send) is no
95: /// source at all, so it notifies nobody.
96: async fn source_snapshot(
97:     connection: &mut PgConnection,
98:     event: &NotificationEvent,
99: ) -> AppResult<Option<SourceSnapshot>> {
100:     match (event.source_kind, event.action_kind) {
101:         (NotificationSourceKind::Delivery, NotificationActionKind::DeliveryFailure) => {
102:             let row = sqlx::query_as::<_, DeliverySourceRow>(
103:                 r#"SELECT delivery.channel_id, destination.thread_id, run.id AS run_id,
104:                           delivery.correlation_id, delivery.status,
105:                           actor.id AS actor_principal_id
106:                      FROM message_deliveries AS delivery
107:                      JOIN workflow_message_destinations AS destination
108:                        ON destination.company_id = delivery.company_id
109:                       AND destination.delivery_id = delivery.id
110:                      JOIN workflow_runs AS run
111:                        ON run.company_id = destination.company_id AND run.id = destination.run_id
112:                      LEFT JOIN principals AS actor
113:                        ON actor.company_id = run.company_id AND actor.user_id = run.actor_id
114:                       AND actor.kind = 'person'
115:                     WHERE delivery.company_id = $1 AND delivery.id = $2
116:                     "#,
117:             )
118:             .bind(event.company_id)
119:             .bind(event.source_id)
120:             .fetch_optional(&mut *connection)
121:             .await
122:             .map_err(AppError::from)?;
123:             Ok(row.map(|row| SourceSnapshot {
124:                 channel_id: Some(row.channel_id),
125:                 thread_id: Some(row.thread_id),
126:                 run_id: Some(row.run_id),
127:                 correlation_id: Some(row.correlation_id),
128:                 responsible_principal_id: row.actor_principal_id,
129:                 target: NotificationTarget::DeliveryRecovery,
130:                 lifecycle: if matches!(row.status.as_str(), "dead_letter" | "outcome_unknown") {
131:                     if row.actor_principal_id.is_some() {
132:                         SourceLifecycle::Active
133:                     } else {
134:                         SourceLifecycle::Withdrawn
135:                     }
136:                 } else {
137:                     SourceLifecycle::Resolved
138:                 },
139:             }))
140:         }
141:         (NotificationSourceKind::DecisionReviewer, NotificationActionKind::DecisionReview) => {
142:             super::reviewer_notice::source_on(connection, event).await
143:         }
144:         (NotificationSourceKind::EscalationNotice, NotificationActionKind::EscalationReview) => {
145:             super::escalation_notice::source_on(connection, event).await
146:         }
147:         _ => Err(AppError::BadRequest(
148:             "Invalid notification source/action pair".into(),
149:         )),
150:     }
151: }
```

## src/domain/entities/notification.rs:21-114
File SHA-256: aff2db4c86b62ec226c4720e871598ed293bfd241ea60d43a1c1f9fa8ef8dce7
```text
21: pub enum NotificationSourceKind {
22:     Delivery,
23:     DecisionReviewer,
24:     EscalationControl,
25:     EscalationNotice,
26: }
27: 
28: impl NotificationSourceKind {
29:     pub const fn as_str(self) -> &'static str {
30:         match self {
31:             Self::Delivery => "delivery",
32:             Self::DecisionReviewer => "decision_reviewer",
33:             Self::EscalationControl => "escalation_control",
34:             Self::EscalationNotice => "escalation_notice",
35:         }
36:     }
37: }
38: 
39: impl FromStr for NotificationSourceKind {
40:     type Err = String;
41: 
42:     fn from_str(value: &str) -> Result<Self, Self::Err> {
43:         match value {
44:             "delivery" => Ok(Self::Delivery),
45:             "decision_reviewer" => Ok(Self::DecisionReviewer),
46:             "escalation_control" => Ok(Self::EscalationControl),
47:             "escalation_notice" => Ok(Self::EscalationNotice),
48:             _ => Err(format!("invalid notification source kind '{value}'")),
49:         }
50:     }
51: }
52: 
53: impl fmt::Display for NotificationSourceKind {
54:     fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
55:         formatter.write_str(self.as_str())
56:     }
57: }
58: 
59: #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
60: #[serde(rename_all = "snake_case")]
61: pub enum NotificationActionKind {
62:     DeliveryFailure,
63:     DecisionReview,
64:     EscalationControl,
65:     EscalationReview,
66: }
67: 
68: impl NotificationActionKind {
69:     pub const ALL: [Self; 4] = [
70:         Self::DeliveryFailure,
71:         Self::DecisionReview,
72:         Self::EscalationControl,
73:         Self::EscalationReview,
74:     ];
75: 
76:     pub const fn as_str(self) -> &'static str {
77:         match self {
78:             Self::DeliveryFailure => "delivery_failure",
79:             Self::DecisionReview => "decision_review",
80:             Self::EscalationControl => "escalation_control",
81:             Self::EscalationReview => "escalation_review",
82:         }
83:     }
84: 
85:     pub const fn label(self) -> &'static str {
86:         match self {
87:             Self::DeliveryFailure => "Delivery needs failure recovery",
88:             Self::DecisionReview => "Workflow decision needs your review",
89:             Self::EscalationControl => "Advance workflow escalation",
90:             Self::EscalationReview => "Workflow escalation needs your attention",
91:         }
92:     }
93: 
94:     pub const fn email_preference(self, preferences: NotificationPreferences) -> bool {
95:         match self {
96:             Self::DeliveryFailure => preferences.delivery_failure_email_enabled,
97:             Self::DecisionReview | Self::EscalationReview => preferences.decision_email_enabled,
98:             Self::EscalationControl => false,
99:         }
100:     }
101: }
102: 
103: impl FromStr for NotificationActionKind {
104:     type Err = String;
105: 
106:     fn from_str(value: &str) -> Result<Self, Self::Err> {
107:         match value {
108:             "delivery_failure" => Ok(Self::DeliveryFailure),
109:             "decision_review" => Ok(Self::DecisionReview),
110:             "escalation_control" => Ok(Self::EscalationControl),
111:             "escalation_review" => Ok(Self::EscalationReview),
112:             _ => Err(format!("invalid notification action kind '{value}'")),
113:         }
114:     }
```

## migrations/20260817000000_init_schema.sql:7851-7868
File SHA-256: 48d206ebc1b805ee41a30a2a93a5bb586ba12d9dc4101c472a3acb6f8406fc89
```text
7851: -- Name: channel_bindings_canonical_deployment_idx; Type: INDEX; Schema: public; Owner: -
7852: --
7853: 
7854: CREATE UNIQUE INDEX channel_bindings_canonical_deployment_idx ON public.channel_bindings USING btree (company_id, channel_id, transport) WHERE ((installation_id IS NULL) AND (status = ANY (ARRAY['active'::text, 'paused'::text])));
7855: 
7856: 
7857: --
7858: -- Name: channel_bindings_channel_idx; Type: INDEX; Schema: public; Owner: -
7859: --
7860: 
7861: CREATE INDEX channel_bindings_channel_idx ON public.channel_bindings USING btree (company_id, channel_id, transport, status);
7862: 
7863: 
7864: --
7865: -- Name: channel_bindings_deployment_endpoint_idx; Type: INDEX; Schema: public; Owner: -
7866: --
7867: 
7868: CREATE UNIQUE INDEX channel_bindings_deployment_endpoint_idx ON public.channel_bindings USING btree (transport, namespace, external_endpoint_key) WHERE ((installation_id IS NULL) AND (status = ANY (ARRAY['active'::text, 'paused'::text])));
```

## migrations/20260817000000_init_schema.sql:15587-15602
File SHA-256: 48d206ebc1b805ee41a30a2a93a5bb586ba12d9dc4101c472a3acb6f8406fc89
```text
15587: -- A principal demoted out of person or agent (removed teammate, deleted account, deleted agent)
15588: -- loses its address in the same transaction, which frees the slug.
15589: CREATE FUNCTION public.addresses_follow_principal_kind() RETURNS trigger LANGUAGE plpgsql AS $$
15590: BEGIN
15591:     IF OLD.kind IN ('person', 'agent') AND NEW.kind NOT IN ('person', 'agent') THEN
15592:         DELETE FROM public.addresses
15593:          WHERE company_id = NEW.company_id AND principal_id = NEW.id;
15594:     END IF;
15595:     RETURN NULL;
15596: END $$;
15597: CREATE TRIGGER addresses_follow_principal_kind AFTER UPDATE OF kind ON public.principals
15598:     FOR EACH ROW EXECUTE FUNCTION public.addresses_follow_principal_kind();
15599: 
15600: -- Removing a teammate, deleting an agent or deleting an account demotes the principal to
15601: -- `external` rather than deleting it; it leaves every department with it.
15602: CREATE FUNCTION public.department_principal_demoted() RETURNS trigger LANGUAGE plpgsql AS $$
```
