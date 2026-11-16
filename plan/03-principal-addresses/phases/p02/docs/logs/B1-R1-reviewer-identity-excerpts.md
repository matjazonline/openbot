### src/adapters/persistence/participant/person.rs:39-110
```
39: /// Caller holds the identity advisory key and its existing account/company ownership prefix.
40: pub(super) async fn resolve_on(
41:     db: &mut PgConnection,
42:     company: Uuid,
43:     user: Uuid,
44:     label: &str,
45:     identity: &QualifiedIdentity,
46: ) -> AppResult<PrincipalId> {
47:     let lifetime: Option<Uuid> = sqlx::query_scalar(
48:         "SELECT id FROM principals WHERE company_id = $1 AND historical_person_user_id = $2",
49:     )
50:     .bind(company)
51:     .bind(user)
52:     .fetch_optional(&mut *db)
53:     .await?;
54:     let holder = holder_on(db, company, identity).await?;
55:     let ids: Vec<Uuid> = lifetime
56:         .into_iter()
57:         .chain(holder.as_ref().map(|row| row.id))
58:         .collect();
59:     sqlx::query_scalar::<_, Uuid>(
60:         "SELECT id FROM principals WHERE company_id = $1 AND id = ANY($2) ORDER BY id FOR UPDATE",
61:     )
62:     .bind(company)
63:     .bind(ids)
64:     .fetch_all(&mut *db)
65:     .await?;
66: 
67:     // Re-read after the complete sorted lock set: a concurrent removal may have demoted it.
68:     let holder = holder_on(db, company, identity).await?;
69:     if let Some(row) = &holder
70:         && row.kind != "external"
71:         && !(row.kind == "person" && row.user_id == Some(user))
72:     {
73:         return Err(AppError::Conflict(format!(
74:             "Email identity is already attached to a {} principal.",
75:             row.kind,
76:         )));
77:     }
78:     let reusable = holder.filter(|row| {
79:         row.historical_person_user_id
80:             .is_none_or(|owner| owner == user)
81:     });
82:     let id = lifetime.or(reusable.map(|row| row.id));
83:     let id = match id {
84:         Some(id) => {
85:             sqlx::query_scalar(
86:                 "UPDATE principals SET kind = 'person', user_id = $3, display_label = $4, \
87:                    updated_at = CURRENT_TIMESTAMP WHERE company_id = $1 AND id = $2 RETURNING id",
88:             )
89:             .bind(company)
90:             .bind(id)
91:             .bind(user)
92:             .bind(label)
93:             .fetch_one(&mut *db)
94:             .await?
95:         }
96:         None => {
97:             sqlx::query_scalar(
98:                 "INSERT INTO principals (id, company_id, kind, user_id, display_label) \
99:                  VALUES ($1, $2, 'person', $3, $4) RETURNING id",
100:             )
101:             .bind(Uuid::new_v4())
102:             .bind(company)
103:             .bind(user)
104:             .bind(label)
105:             .fetch_one(&mut *db)
106:             .await?
107:         }
108:     };
109:     Ok(PrincipalId::new(id))
110: }
```

### src/adapters/persistence/participant.rs:237-278
```
237: pub(super) async fn upsert_account_identity_on(
238:     connection: &mut sqlx::PgConnection,
239:     company_id: Uuid,
240:     principal_id: PrincipalId,
241:     identity: &QualifiedIdentity,
242:     display_label: &str,
243: ) -> AppResult<()> {
244:     let claims = serde_json::to_value(IdentityClaimMetadata::account())
245:         .map_err(|error| AppError::Internal(error.to_string()))?;
246:     sqlx::query(
247:         r#"WITH account_identity AS (
248:                INSERT INTO participant_identities
249:                    (id, company_id, principal_id, transport, namespace, subject, display_label,
250:                     status, claim_metadata, provenance)
251:                VALUES ($1, $2, $3, $4, $5, $6, $7, 'verified', $8, 'account')
252:                ON CONFLICT (company_id, transport, namespace, subject) DO UPDATE SET
253:                    principal_id = EXCLUDED.principal_id,
254:                    display_label = EXCLUDED.display_label,
255:                    status = 'verified', claim_metadata = EXCLUDED.claim_metadata,
256:                    provenance = 'account', updated_at = CURRENT_TIMESTAMP
257:                RETURNING id
258:            )
259:            UPDATE messages AS message
260:            SET author_principal_id = $3
261:            FROM account_identity
262:            WHERE message.company_id = $2
263:              AND message.authored_identity_id = account_identity.id
264:              AND message.author_principal_id <> $3"#,
265:     )
266:     .bind(ParticipantIdentityId::random().as_uuid())
267:     .bind(company_id)
268:     .bind(principal_id.as_uuid())
269:     .bind(identity.transport().as_str())
270:     .bind(identity.namespace().as_str())
271:     .bind(identity.subject().as_str())
272:     .bind(display_label)
273:     .bind(claims)
274:     .execute(&mut *connection)
275:     .await
276:     .map_err(AppError::from)?;
277:     Ok(())
278: }
```

### src/adapters/persistence/participant.rs:334-363
```
334: /// Give an account its company principal, with its mailbox as a verified email identity.
335: ///
336: /// If that mailbox was already seen as an outsider, the existing external principal is *promoted*
337: /// rather than duplicated, so the threads it is already a party to stay attached to the person who
338: /// just joined. The promotion is keyed on the account's own registered address and nothing else --
339: /// never a display name, and never an email a provider merely claimed on somebody's behalf.
340: pub(crate) async fn create_person_principal_on(
341:     connection: &mut sqlx::PgConnection,
342:     company_id: Uuid,
343:     user_id: Uuid,
344:     display_label: &str,
345:     email: &str,
346: ) -> AppResult<PrincipalId> {
347:     let qualified = account_email_identity(email)?;
348:     lock_identity_key(connection, company_id, &qualified).await?;
349: 
350:     let principal_id =
351:         person::resolve_on(connection, company_id, user_id, display_label, &qualified).await?;
352: 
353:     upsert_account_identity_on(
354:         connection,
355:         company_id,
356:         principal_id,
357:         &qualified,
358:         display_label,
359:     )
360:     .await?;
361:     ensure_person_address_on(connection, company_id, principal_id, display_label).await?;
362:     Ok(principal_id)
363: }
```

### src/adapters/persistence/participant.rs:555-593
```
555:     async fn access_context_for_identity(
556:         &self,
557:         company_id: Uuid,
558:         identity: &QualifiedIdentity,
559:     ) -> AppResult<PrincipalAccessContext> {
560:         let row = sqlx::query_as::<_, IdentityAccessRow>(
561:             r#"SELECT principal.id AS principal_id, identity.status,
562:                       CASE WHEN company.user_id = principal.user_id THEN 'owner' ELSE member.role END AS membership,
563:                       member.access_level::smallint AS access_level
564:                FROM participant_identities AS identity
565:                JOIN principals AS principal ON (principal.company_id, principal.id) = (identity.company_id, identity.principal_id)
566:                JOIN companies AS company ON company.id = principal.company_id
567:                LEFT JOIN company_members AS member ON (member.company_id, member.user_id) = (principal.company_id, principal.user_id)
568:                WHERE identity.company_id=$1 AND identity.transport=$2 AND identity.namespace=$3 AND identity.subject=$4"#)
569:             .bind(company_id).bind(identity.transport().as_str()).bind(identity.namespace().as_str())
570:             .bind(identity.subject().as_str()).fetch_optional(&self.pool).await?;
571:         let Some(row) = row else {
572:             return Ok(PrincipalAccessContext {
573:                 principal_id: None,
574:                 membership: CompanyMembership::None,
575:                 access_level: crate::entities::access_level::AccessLevel::PUBLIC,
576:                 grant_authority: PrincipalGrantAuthority::Available,
577:             });
578:         };
579:         let status = IdentityStatus::from_str(&row.status)
580:             .map_err(|error| AppError::Internal(error.to_string()))?;
581:         if status == IdentityStatus::Observed {
582:             return Ok(PrincipalAccessContext::external(PrincipalId::new(
583:                 row.principal_id,
584:             )));
585:         }
586:         let membership = membership_from_db(row.membership.as_deref())?;
587:         Ok(PrincipalAccessContext {
588:             principal_id: Some(PrincipalId::new(row.principal_id)),
589:             membership,
590:             access_level: reader_level(row.principal_id, membership, row.access_level)?,
591:             grant_authority: PrincipalGrantAuthority::Available,
592:         })
593:     }
```

### src/adapters/persistence/account_identity.rs:52-102
```
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
```

### src/adapters/persistence/account_identity.rs:183-278
```
183: async fn adopt_new_address(
184:     connection: &mut sqlx::PgConnection,
185:     company_id: Uuid,
186:     person: PrincipalId,
187:     new: &QualifiedIdentity,
188:     display_label: &str,
189: ) -> AppResult<()> {
190:     let holder = identity_holder(connection, company_id, new).await?;
191:     let adopted_outsider = match &holder {
192:         None => None,
193:         Some(holder) if holder.principal_id == person.as_uuid() => None,
194:         Some(holder) if holder.kind == "external" => Some(holder.principal_id),
195:         Some(holder) => {
196:             return Err(AppError::Conflict(format!(
197:                 "Email identity is already attached to a {} principal.",
198:                 holder.kind
199:             )));
200:         }
201:     };
202:     upsert_account_identity_on(connection, company_id, person, new, display_label).await?;
203:     if let Some(outsider) = adopted_outsider {
204:         adopt_outsider_places(connection, company_id, person, outsider).await?;
205:     }
206:     Ok(())
207: }
208: 
209: /// Give the person the channel grants and thread places an outsider held for the address it just
210: /// took, when that address was all the outsider was.
211: ///
212: /// An outsider that still has another identity keeps its rows, because they serve that identity
213: /// too. Otherwise the outsider is left with no identity, grant or thread place -- unreachable, but
214: /// not deleted, since other history (a workflow destination, a notification actor) may name it.
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
