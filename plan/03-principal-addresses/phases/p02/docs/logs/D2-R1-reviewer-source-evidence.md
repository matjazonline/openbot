### src/adapters/persistence/notification/projection_lock.rs:15-75
```
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

### src/adapters/persistence/notification/control.rs:23-43
```
23: pub(super) async fn owned_projection_on(
24:     db: &mut Transaction<'_, Postgres>,
25:     commit: &NotificationProjectionCommit<'_>,
26: ) -> AppResult<OwnedProjection> {
27:     let Some(hint) = read_claimed_event(db, commit.fence).await? else {
28:         return Ok(OwnedProjection::LeaseLost);
29:     };
30:     let prepared =
31:         projection_lock::lock_on(db, &hint, projection_lock::ProjectionPurpose::Project).await?;
32:     let Some(event) = locked_event(db, commit.fence).await? else {
33:         return Ok(OwnedProjection::LeaseLost);
34:     };
35:     match prepared {
36:         projection_lock::LockedProjection::Current(current)
37:             if current.event == event && *current == *commit.expected =>
38:         {
39:             Ok(OwnedProjection::Current(current))
40:         }
41:         _ => Ok(OwnedProjection::Stale),
42:     }
43: }
```

### src/adapters/persistence/notification/projection.rs:32-90
```
32: pub(super) async fn resolve_projection_on(
33:     connection: &mut PgConnection,
34:     event: &NotificationEvent,
35: ) -> AppResult<NotificationProjection> {
36:     if let Some(disposition) = escalation_notice::agent_disposition_on(connection, event).await? {
37:         return Ok(NotificationProjection {
38:             event: event.clone(),
39:             disposition,
40:         });
41:     }
42:     if event.source_kind == NotificationSourceKind::EscalationControl {
43:         return Ok(NotificationProjection {
44:             event: event.clone(),
45:             disposition: NotificationDisposition::Control,
46:         });
47:     }
48:     let Some(source) = source_snapshot(connection, event).await? else {
49:         return Ok(NotificationProjection {
50:             event: event.clone(),
51:             disposition: NotificationDisposition::Withdrawn,
52:         });
53:     };
54:     let newer_exists: bool = sqlx::query_scalar(
55:         r#"SELECT EXISTS (
56:                SELECT 1 FROM notification_events AS newer
57:                 WHERE newer.company_id = $1 AND newer.source_kind = $2
58:                   AND newer.source_id = $3 AND newer.action_kind = $4
59:                   AND newer.source_generation > $5
60:            )"#,
61:     )
62:     .bind(event.company_id)
63:     .bind(event.source_kind.as_str())
64:     .bind(event.source_id)
65:     .bind(event.action_kind.as_str())
66:     .bind(i64::try_from(event.source_generation).unwrap_or(i64::MAX))
67:     .fetch_one(&mut *connection)
68:     .await
69:     .map_err(AppError::from)?;
70:     let disposition = if newer_exists {
71:         NotificationDisposition::Withdrawn
72:     } else {
73:         match source.lifecycle {
74:             SourceLifecycle::Resolved => NotificationDisposition::Resolved,
75:             SourceLifecycle::Withdrawn => NotificationDisposition::Withdrawn,
76:             SourceLifecycle::Active => match source.responsible_principal_id {
77:                 Some(principal_id) => recipient_for_source(connection, event, source, principal_id)
78:                     .await?
79:                     .map(Box::new)
80:                     .map(NotificationDisposition::Active)
81:                     .unwrap_or(NotificationDisposition::Withdrawn),
82:                 None => NotificationDisposition::Withdrawn,
83:             },
84:         }
85:     };
86:     Ok(NotificationProjection {
87:         event: event.clone(),
88:         disposition,
89:     })
90: }
```

### src/adapters/persistence/notification/projection.rs:153-218
```
153: async fn recipient_for_source(
154:     connection: &mut PgConnection,
155:     event: &NotificationEvent,
156:     source: SourceSnapshot,
157:     principal_id: Uuid,
158: ) -> AppResult<Option<NotificationRecipient>> {
159:     #[derive(sqlx::FromRow)]
160:     struct RecipientRow {
161:         user_id: Uuid,
162:         email: String,
163:         company_label: String,
164:         channel_label: String,
165:         delivery_failure_email_enabled: bool,
166:         decision_email_enabled: bool,
167:     }
168:     let row = sqlx::query_as::<_, RecipientRow>(
169:         r#"SELECT principal.user_id, account.email,
170:                   company.name AS company_label, COALESCE(channel.name,'Company workflow') AS channel_label,
171:                   COALESCE(preference.delivery_failure_email_enabled, TRUE)
172:                       AS delivery_failure_email_enabled,
173:                   COALESCE(preference.decision_email_enabled, TRUE)
174:                       AS decision_email_enabled
175:              FROM principals AS principal
176:              JOIN users AS account ON account.id = principal.user_id
177:              JOIN company_members AS member
178:                ON member.company_id = principal.company_id
179:               AND member.user_id = principal.user_id
180:              JOIN companies AS company ON company.id = principal.company_id
181:              LEFT JOIN channels AS channel
182:                ON channel.company_id = principal.company_id AND channel.id = $3
183:              LEFT JOIN user_notification_preferences AS preference
184:                ON preference.user_id = account.id
185:             WHERE principal.company_id = $1 AND principal.id = $2
186:               AND principal.kind = 'person'
187:               AND notification_principal_can_view($1, $3, $2)
188:             "#,
189:     )
190:     .bind(event.company_id)
191:     .bind(principal_id)
192:     .bind(source.channel_id)
193:     .fetch_optional(&mut *connection)
194:     .await
195:     .map_err(AppError::from)?;
196:     Ok(row.map(|row| {
197:         let preferences = NotificationPreferences {
198:             delivery_failure_email_enabled: row.delivery_failure_email_enabled,
199:             decision_email_enabled: row.decision_email_enabled,
200:         };
201:         NotificationRecipient {
202:             principal_id: PrincipalId::new(principal_id),
203:             user_id: row.user_id,
204:             email: EmailAddress::from(row.email),
205:             company_label: row.company_label,
206:             channel_label: row.channel_label,
207:             channel_id: source.channel_id,
208:             thread_id: source.thread_id,
209:             run_id: source.run_id,
210:             correlation_id: source
211:                 .correlation_id
212:                 .map(Into::into)
213:                 .unwrap_or_else(|| event.id.as_uuid().into()),
214:             email_enabled: event.action_kind.email_preference(preferences),
215:             target: source.target,
216:         }
217:     }))
218: }
```

### src/adapters/persistence/notification/projection.rs:284-340
```
284:     let NotificationDisposition::Active(recipient) = &projection.disposition else {
285:         return Ok(result);
286:     };
287:     let inserted_id: Uuid = sqlx::query_scalar(
288:         r#"INSERT INTO notifications (
289:                id, company_id, recipient_user_id, recipient_principal_id, event_id,
290:                source_kind, source_id, action_kind, source_generation, channel_id
291:            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
292:            ON CONFLICT ON CONSTRAINT notifications_identity_key DO UPDATE
293:              SET recipient_principal_id = EXCLUDED.recipient_principal_id,
294:                  event_id = EXCLUDED.event_id, channel_id = EXCLUDED.channel_id,
295:                  state = 'active', state_changed_at = CURRENT_TIMESTAMP,
296:                  updated_at = CURRENT_TIMESTAMP
297:            RETURNING id"#,
298:     )
299:     .bind(projection.event.notification_id.as_uuid())
300:     .bind(projection.event.company_id)
301:     .bind(recipient.user_id)
302:     .bind(recipient.principal_id.as_uuid())
303:     .bind(projection.event.id.as_uuid())
304:     .bind(projection.event.source_kind.as_str())
305:     .bind(projection.event.source_id)
306:     .bind(projection.event.action_kind.as_str())
307:     .bind(i64::try_from(projection.event.source_generation).unwrap_or(i64::MAX))
308:     .bind(recipient.channel_id)
309:     .fetch_one(&mut **tx)
310:     .await
311:     .map_err(AppError::from)?;
312:     result.created = 1;
313: 
314:     if projection.should_email() {
315:         let delivery = email.ok_or_else(|| {
316:             AppError::Internal("Enabled notification email was not composed".into())
317:         })?;
318:         let creation =
319:             crate::adapters::persistence::delivery::enqueue::insert_standalone_delivery_on(
320:                 tx, delivery,
321:             )
322:             .await?;
323:         let delivery_id = match creation {
324:             DeliveryCreation::Created(id) | DeliveryCreation::Absorbed(id) => id,
325:         };
326:         sqlx::query(
327:             r#"UPDATE notifications SET email_delivery_id = $2, updated_at = CURRENT_TIMESTAMP
328:                 WHERE id = $1 AND email_delivery_id IS NULL"#,
329:         )
330:         .bind(inserted_id)
331:         .bind(delivery_id.as_uuid())
332:         .execute(&mut **tx)
333:         .await
334:         .map_err(AppError::from)?;
335:     } else if email.is_some() {
336:         return Err(AppError::Internal(
337:             "A suppressed notification email was supplied for projection".into(),
338:         ));
339:     }
340:     Ok(result)
```

### src/adapters/persistence/notification.rs:330-445
```
330:     async fn list_notifications(
331:         &self,
332:         company_id: Uuid,
333:         user_id: Uuid,
334:         recipient_principal_id: PrincipalId,
335:         visible_channel_ids: &[Uuid],
336:     ) -> AppResult<NotificationPage> {
337:         let rows = sqlx::query_as::<_, NotificationRow>(
338:             r#"SELECT notification.id, notification.company_id, company.name AS company_label,
339:                       notification.channel_id, COALESCE(channel.name,'Company workflow') AS channel_label,
340:                       notification.source_kind, notification.source_id,
341:                       notification.action_kind, notification.source_generation,
342:                       notification.state, notification.read_at, notification.created_at,
343:                       notification.state_changed_at,
344:                       COUNT(*) FILTER (
345:                           WHERE notification.state = 'active' AND notification.read_at IS NULL
346:                       ) OVER () AS unread_count
347:                  FROM notifications AS notification
348:                  JOIN companies AS company ON company.id = notification.company_id
349:                  LEFT JOIN channels AS channel
350:                    ON channel.company_id = notification.company_id
351:                   AND channel.id = notification.channel_id
352:                 WHERE notification.company_id = $1
353:                   AND notification.recipient_user_id = $2
354:                   AND notification.recipient_principal_id = $3
355:                   AND (notification.channel_id IS NULL OR notification.channel_id = ANY($4))
356:                   AND notification_principal_can_view(
357:                       notification.company_id, notification.channel_id,
358:                       notification.recipient_principal_id
359:                   )
360:                 ORDER BY (notification.state = 'active') DESC,
361:                          notification.created_at DESC, notification.id DESC
362:                 LIMIT $5"#,
363:         )
364:         .bind(company_id)
365:         .bind(user_id)
366:         .bind(recipient_principal_id.as_uuid())
367:         .bind(visible_channel_ids)
368:         .bind(crate::application::notification::NOTIFICATION_LIST_LIMIT)
369:         .fetch_all(&self.pool)
370:         .await
371:         .map_err(AppError::from)?;
372:         let unread_count = rows
373:             .first()
374:             .map(|row| nonnegative_u64(row.unread_count, "unread notifications"))
375:             .transpose()?
376:             .unwrap_or(0);
377:         Ok(NotificationPage {
378:             items: rows
379:                 .into_iter()
380:                 .map(TryInto::try_into)
381:                 .collect::<AppResult<Vec<_>>>()?,
382:             unread_count,
383:         })
384:     }
385: 
386:     async fn notification_company_for_user(
387:         &self,
388:         user_id: Uuid,
389:         notification_id: NotificationId,
390:     ) -> AppResult<Option<Uuid>> {
391:         sqlx::query_scalar(
392:             r#"SELECT notification.company_id
393:                  FROM notifications AS notification
394:                  JOIN principals AS principal
395:                    ON principal.company_id = notification.company_id
396:                   AND principal.id = notification.recipient_principal_id
397:                 WHERE notification.id = $1 AND notification.recipient_user_id = $2
398:                   AND principal.user_id = $2"#,
399:         )
400:         .bind(notification_id.as_uuid())
401:         .bind(user_id)
402:         .fetch_optional(&self.pool)
403:         .await
404:         .map_err(AppError::from)
405:     }
406: 
407:     async fn notification_href(
408:         &self,
409:         company_id: Uuid,
410:         user_id: Uuid,
411:         recipient_principal_id: PrincipalId,
412:         visible_channel_ids: &[Uuid],
413:         notification_id: NotificationId,
414:     ) -> AppResult<String> {
415:         let event_row = sqlx::query_as::<_, NotificationEventDb>(&format!(
416:             r#"SELECT {EVENT_COLUMNS}
417:                  FROM notifications AS notification
418:                  JOIN notification_events AS event ON event.id = notification.event_id
419:                 WHERE notification.id = $1 AND notification.company_id = $2
420:                   AND notification.recipient_user_id = $3
421:                   AND notification.recipient_principal_id = $4
422:                   AND (notification.channel_id IS NULL OR notification.channel_id = ANY($5))
423:                   AND notification.state = 'active'"#
424:         ))
425:         .bind(notification_id.as_uuid())
426:         .bind(company_id)
427:         .bind(user_id)
428:         .bind(recipient_principal_id.as_uuid())
429:         .bind(visible_channel_ids)
430:         .fetch_optional(&self.pool)
431:         .await
432:         .map_err(AppError::from)?
433:         .ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
434:         let event = event_row.event()?;
435:         let projection = self.resolve_notification_projection(&event).await?;
436:         let NotificationDisposition::Active(recipient) = projection.disposition else {
437:             return Err(AppError::NotFound(
438:                 "This notification is no longer actionable.".into(),
439:             ));
440:         };
441:         if recipient.user_id != user_id || recipient.principal_id != recipient_principal_id {
442:             return Err(AppError::NotFound("Notification not found.".into()));
443:         }
444:         Ok(source_href(&event, &recipient))
445:     }
```

### src/adapters/persistence/notification.rs:447-529
```
447:     async fn open_notification(
448:         &self,
449:         company_id: Uuid,
450:         user_id: Uuid,
451:         recipient_principal_id: PrincipalId,
452:         visible_channel_ids: &[Uuid],
453:         notification_id: NotificationId,
454:     ) -> AppResult<String> {
455:         let mut tx = self.pool.begin().await.map_err(AppError::from)?;
456:         let event_row = sqlx::query_as::<_, NotificationEventDb>(&format!(
457:             r#"SELECT {EVENT_COLUMNS}
458:                  FROM notifications AS notification
459:                  JOIN notification_events AS event ON event.id = notification.event_id
460:                 WHERE notification.id = $1 AND notification.company_id = $2
461:                   AND notification.recipient_user_id = $3
462:                   AND notification.recipient_principal_id = $4
463:                   AND (notification.channel_id IS NULL OR notification.channel_id = ANY($5))
464:                   AND notification.state = 'active'"#
465:         ))
466:         .bind(notification_id.as_uuid())
467:         .bind(company_id)
468:         .bind(user_id)
469:         .bind(recipient_principal_id.as_uuid())
470:         .bind(visible_channel_ids)
471:         .fetch_optional(&mut *tx)
472:         .await
473:         .map_err(AppError::from)?
474:         .ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
475:         let event = event_row.event()?;
476:         let projection_lock::LockedProjection::Current(projection) = projection_lock::lock_on(
477:             &mut tx,
478:             &event,
479:             projection_lock::ProjectionPurpose::Open { user_id },
480:         )
481:         .await?
482:         else {
483:             return Err(AppError::Conflict(
484:                 "Notification authority changed; reopen it".into(),
485:             ));
486:         };
487:         let found:Option<Uuid>=sqlx::query_scalar("SELECT id FROM notifications WHERE company_id=$1 AND id=$2 AND recipient_user_id=$3 AND recipient_principal_id=$4 AND state='active' FOR UPDATE")
488:             .bind(company_id).bind(notification_id.as_uuid()).bind(user_id).bind(recipient_principal_id.as_uuid()).fetch_optional(&mut *tx).await?;
489:         found.ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
490:         let projection = *projection;
491:         let NotificationDisposition::Active(recipient) = projection.disposition else {
492:             let state = match projection.disposition {
493:                 NotificationDisposition::Resolved => "resolved",
494:                 NotificationDisposition::Withdrawn => "withdrawn",
495:                 NotificationDisposition::Active(_) => unreachable!(),
496:                 NotificationDisposition::Control | NotificationDisposition::Agent { .. } => {
497:                     return Err(AppError::NotFound("notification".into()));
498:                 }
499:             };
500:             sqlx::query(
501:                 r#"UPDATE notifications SET state = $2, state_changed_at = CURRENT_TIMESTAMP,
502:                           updated_at = CURRENT_TIMESTAMP
503:                     WHERE id = $1 AND state = 'active'"#,
504:             )
505:             .bind(notification_id.as_uuid())
506:             .bind(state)
507:             .execute(&mut *tx)
508:             .await
509:             .map_err(AppError::from)?;
510:             tx.commit().await.map_err(AppError::from)?;
511:             return Err(AppError::NotFound(
512:                 "This notification is no longer actionable.".into(),
513:             ));
514:         };
515:         if recipient.user_id != user_id || recipient.principal_id != recipient_principal_id {
516:             return Err(AppError::NotFound("Notification not found.".into()));
517:         }
518:         sqlx::query(
519:             r#"UPDATE notifications
520:                   SET read_at = COALESCE(read_at, CURRENT_TIMESTAMP), updated_at = CURRENT_TIMESTAMP
521:                 WHERE id = $1"#,
522:         )
523:         .bind(notification_id.as_uuid())
524:         .execute(&mut *tx)
525:         .await
526:         .map_err(AppError::from)?;
527:         tx.commit().await.map_err(AppError::from)?;
528:         Ok(source_href(&event, &recipient))
529:     }
```

### src/adapters/persistence/company_invite.rs:252-288
```
252:     async fn set_member_access_level(
253:         &self,
254:         change: MemberLevelChange,
255:     ) -> AppResult<Option<CompanyMember>> {
256:         use crate::entities::access_level::LevelTarget;
257:         let scope = TeamScope {
258:             actor_id: change.actor_id,
259:             company_id: change.company_id,
260:         };
261:         let mut tx = authority::start(&self.pool, scope).await?;
262:         let team = authority::lock_team(&mut tx, scope, Some(change.member_user_id)).await?;
263:         let Some(target) = team.member(change.member_user_id) else {
264:             return Ok(None);
265:         };
266:         let current = if change.member_user_id == team.owner_id {
267:             LevelTarget::Owner
268:         } else {
269:             LevelTarget::Assignable(target.actor()?.access_level)
270:         };
271:         team.assign(current, change.access_level)?;
272:         sqlx::query("UPDATE company_members SET access_level=$3::smallint WHERE company_id=$1 AND user_id=$2 AND role<>'owner'")
273:             .bind(scope.company_id).bind(change.member_user_id).bind(change.access_level.as_i16())
274:             .execute(&mut *tx).await.map_err(authority::database)?;
275:         let row = sqlx::query_as::<_, CompanyMemberDb>(authority::MEMBER_SELECT)
276:             .bind(scope.company_id)
277:             .bind(change.member_user_id)
278:             .fetch_optional(&mut *tx)
279:             .await
280:             .map_err(authority::database)?;
281:         Box::pin(super::workflow::refresh_member_dependents_on(
282:             &mut tx,
283:             crate::domain::workflow::CompanyId::new(scope.company_id),
284:         ))
285:         .await?;
286:         tx.commit().await.map_err(authority::database)?;
287:         row.map(TryInto::try_into).transpose()
288:     }
```

### src/adapters/persistence/company_invite.rs:358-437
```
358:     async fn accept_pending_invite(
359:         &self,
360:         invite_id: Uuid,
361:         user_id: Uuid,
362:         user_email: &str,
363:     ) -> AppResult<Option<CompanyInvite>> {
364:         let member_id = Uuid::new_v4();
365:         let mut transaction = self.pool.begin().await.map_err(AppError::from)?;
366:         if !lock_verified_account_email(&mut transaction, user_id, user_email).await? {
367:             return Ok(None);
368:         }
369:         let db = sqlx::query_as::<_, CompanyInviteDb>(
370:             r#"
371:             WITH accepted AS (
372:                 UPDATE company_invites
373:                 SET status = 'accepted'
374:                 WHERE id = $1 AND status = 'pending' AND email = $2
375:                 RETURNING id, company_id, email, role, access_level::smallint AS access_level, status, created_at
376:             ), membership AS (
377:                 -- An invite never rewrites the owner's row: accepting one addressed to the owner
378:                 -- leaves them the owner (the database also refuses an owner row that disagrees
379:                 -- with companies.user_id). A data-modifying CTE runs whether or not it is read, so
380:                 -- the invite still reads back accepted when this writes nothing.
381:                 INSERT INTO company_members (id, company_id, user_id, role, access_level)
382:                 SELECT $3, company_id, $4, role, access_level
383:                 FROM accepted
384:                 ON CONFLICT (company_id, user_id)
385:                 DO UPDATE SET role = EXCLUDED.role WHERE company_members.role <> 'owner'
386:                 RETURNING company_id
387:             )
388:             SELECT accepted.id, accepted.company_id, company.name AS company_name, accepted.email,
389:                    accepted.role, accepted.access_level::smallint AS access_level, accepted.status, accepted.created_at
390:             FROM accepted
391:             JOIN companies AS company ON company.id = accepted.company_id
392:             "#,
393:         )
394:         .bind(invite_id)
395:         .bind(user_email)
396:         .bind(member_id)
397:         .bind(user_id)
398:         .fetch_optional(&mut *transaction)
399:         .await
400:         .map_err(AppError::from)?;
401: 
402:         if let Some(invite) = &db {
403:             let username: String =
404:                 sqlx::query_scalar("SELECT username::text FROM users WHERE id = $1")
405:                     .bind(user_id)
406:                     .fetch_one(&mut *transaction)
407:                     .await
408:                     .map_err(AppError::from)?;
409:             create_person_principal_on(
410:                 &mut transaction,
411:                 invite.company_id,
412:                 user_id,
413:                 &username,
414:                 user_email,
415:             )
416:             .await?;
417:         }
418:         let result = if db.is_some() {
419:             db
420:         } else {
421:             sqlx::query_as::<_, CompanyInviteDb>(
422:                 r#"SELECT invite.id, invite.company_id, company.name AS company_name, invite.email,
423:                           invite.role, invite.access_level::smallint AS access_level, invite.status, invite.created_at
424:                    FROM company_invites AS invite
425:                    JOIN companies AS company ON company.id = invite.company_id
426:                    WHERE invite.id = $1 AND invite.email = $2 AND invite.status = 'accepted'"#,
427:             )
428:             .bind(invite_id)
429:             .bind(user_email)
430:             .fetch_optional(&mut *transaction)
431:             .await
432:             .map_err(AppError::from)?
433:         };
434:         transaction.commit().await.map_err(AppError::from)?;
435: 
436:         result.map(TryInto::try_into).transpose()
437:     }
```

### src/adapters/persistence/company_invite.rs:571-659
```
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

### src/adapters/http/routes/notifications.rs:134-182
```
134: async fn open_notification(
135:     State(state): State<AppState>,
136:     viewer: Viewer,
137:     Path(notification_id): Path<Uuid>,
138: ) -> AppResult<Redirect> {
139:     let notification_id = notification_id.into();
140:     let company_id = state
141:         .notifications
142:         .notification_company_for_user(viewer.user_id, notification_id)
143:         .await?
144:         .ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
145:     let context = notification_read_context(&state, &viewer, company_id).await?;
146:     let href = state
147:         .notifications
148:         .open_notification(
149:             company_id,
150:             viewer.user_id,
151:             context.principal_id,
152:             &context.visible_channel_ids,
153:             notification_id,
154:         )
155:         .await?;
156:     Ok(Redirect::to(&href))
157: }
158: 
159: async fn follow_notification(
160:     State(state): State<AppState>,
161:     viewer: Viewer,
162:     Path(notification_id): Path<Uuid>,
163: ) -> AppResult<Redirect> {
164:     let notification_id = notification_id.into();
165:     let company_id = state
166:         .notifications
167:         .notification_company_for_user(viewer.user_id, notification_id)
168:         .await?
169:         .ok_or_else(|| AppError::NotFound("Notification not found.".into()))?;
170:     let context = notification_read_context(&state, &viewer, company_id).await?;
171:     let href = state
172:         .notifications
173:         .notification_href(
174:             company_id,
175:             viewer.user_id,
176:             context.principal_id,
177:             &context.visible_channel_ids,
178:             notification_id,
179:         )
180:         .await?;
181:     Ok(Redirect::to(&href))
182: }
```

### migrations/20260817000000_init_schema.sql:1021-1062
```
1021: CREATE FUNCTION public.notification_recheck_membership() RETURNS trigger
1022:     LANGUAGE plpgsql
1023:     AS $$
1024: DECLARE
1025:     changed_company UUID := COALESCE(NEW.company_id, OLD.company_id);
1026:     changed_user UUID := COALESCE(NEW.user_id, OLD.user_id);
1027: BEGIN
1028:     UPDATE notifications AS notification
1029:        SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
1030:            updated_at = CURRENT_TIMESTAMP
1031:      WHERE notification.company_id = changed_company
1032:        AND notification.recipient_user_id = changed_user
1033:        AND notification.state = 'active'
1034:        AND (
1035:            notification.recipient_principal_id IS NULL
1036:            OR NOT notification_principal_can_view(
1037:                notification.company_id,
1038:                notification.channel_id,
1039:                notification.recipient_principal_id
1040:            )
1041:        );
1042:     RETURN COALESCE(NEW, OLD);
1043: END;
1044: $$;
1045: 
1046: 
1047: --
1048: -- Name: notification_withdraw_deleted_principal(); Type: FUNCTION; Schema: public; Owner: -
1049: --
1050: 
1051: CREATE FUNCTION public.notification_withdraw_deleted_principal() RETURNS trigger
1052:     LANGUAGE plpgsql
1053:     AS $$
1054: BEGIN
1055:     UPDATE notifications
1056:        SET state = 'withdrawn', state_changed_at = CURRENT_TIMESTAMP,
1057:            updated_at = CURRENT_TIMESTAMP
1058:      WHERE company_id = OLD.company_id AND recipient_principal_id = OLD.id
1059:        AND state = 'active';
1060:     RETURN OLD;
1061: END;
1062: $$;
```

### migrations/20260817000000_init_schema.sql:10022-10039
```
10022: ALTER TABLE ONLY public.notifications
10023:     ADD CONSTRAINT notifications_event_identity_fk FOREIGN KEY (company_id, event_id, source_kind, source_id, action_kind, source_generation) REFERENCES public.notification_events(company_id, id, source_kind, source_id, action_kind, source_generation) ON DELETE CASCADE;
10024: 
10025: 
10026: --
10027: -- Name: notifications notifications_recipient_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
10028: --
10029: 
10030: ALTER TABLE ONLY public.notifications
10031:     ADD CONSTRAINT notifications_recipient_fk FOREIGN KEY (company_id, recipient_principal_id) REFERENCES public.principals(company_id, id) ON DELETE SET NULL (recipient_principal_id);
10032: 
10033: 
10034: --
10035: -- Name: notifications notifications_recipient_member_fk; Type: FK CONSTRAINT; Schema: public; Owner: -
10036: --
10037: 
10038: ALTER TABLE ONLY public.notifications
10039:     ADD CONSTRAINT notifications_recipient_member_fk FOREIGN KEY (company_id, recipient_user_id) REFERENCES public.company_members(company_id, user_id) ON DELETE CASCADE;
```

### migrations/20260817000000_init_schema.sql:16723-16742
```
16723: CREATE FUNCTION public.guard_notification_event_identity() RETURNS trigger LANGUAGE plpgsql AS $$
16724: BEGIN
16725:     IF (NEW.company_id,NEW.id,NEW.notification_id,NEW.source_kind,NEW.source_id,NEW.action_kind,
16726:         NEW.source_generation,NEW.occurred_at)
16727:         IS DISTINCT FROM (OLD.company_id,OLD.id,OLD.notification_id,OLD.source_kind,OLD.source_id,OLD.action_kind,
16728:         OLD.source_generation,OLD.occurred_at) THEN
16729:         RAISE EXCEPTION 'notification event source identity is immutable'
16730:             USING ERRCODE='23514',CONSTRAINT='workflow_guard:guard_notification_event_identity';
16731:     END IF;
16732:     -- The existing FK clears a physically deleted actor. It cannot rebind a live actor.
16733:     IF NEW.actor_principal_id IS DISTINCT FROM OLD.actor_principal_id AND NOT (
16734:         NEW.actor_principal_id IS NULL AND OLD.actor_principal_id IS NOT NULL AND NOT EXISTS (
16735:             SELECT 1 FROM public.principals WHERE company_id=OLD.company_id AND id=OLD.actor_principal_id)) THEN
16736:         RAISE EXCEPTION 'notification event actor can only be cleared by retirement'
16737:             USING ERRCODE='23514',CONSTRAINT='workflow_guard:guard_notification_event_identity';
16738:     END IF;
16739:     RETURN NEW;
16740: END $$;
16741: CREATE TRIGGER notification_event_identity_guard BEFORE UPDATE ON public.notification_events
16742:     FOR EACH ROW EXECUTE FUNCTION public.guard_notification_event_identity();
```

### migrations/20260817000000_init_schema.sql:18556-18574
```
18556: CREATE FUNCTION public.principal_direct_can_view(tenant uuid, target uuid, viewer uuid)
18557: RETURNS boolean LANGUAGE sql STABLE AS $$
18558:     SELECT COALESCE((
18559:         SELECT CASE recipient.kind
18560:             WHEN 'agent' THEN true
18561:             WHEN 'person' THEN actor.id = recipient.id OR actor.user_id = company.user_id
18562:             WHEN 'external' THEN recipient.historical_person_user_id IS NOT NULL
18563:                 AND actor.user_id = company.user_id
18564:             ELSE false END
18565:         FROM public.principals AS recipient
18566:         JOIN public.companies AS company ON company.id = recipient.company_id
18567:         JOIN public.principals AS actor ON actor.company_id = recipient.company_id AND actor.id = viewer
18568:         WHERE recipient.company_id = tenant AND recipient.id = target
18569:             AND (actor.kind = 'agent' OR (actor.kind = 'person' AND (
18570:                 actor.user_id = company.user_id OR EXISTS (
18571:                     SELECT 1 FROM public.company_members AS membership
18572:                     WHERE membership.company_id = tenant AND membership.user_id = actor.user_id))))
18573:     ), false)
18574: $$;
```
