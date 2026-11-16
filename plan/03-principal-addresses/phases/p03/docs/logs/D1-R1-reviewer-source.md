# D1-R1 independent source evidence
Frozen source `1f4af6f63e8f51cd33c0c777ffd62fbeb05d218830a9dbc5b1c6f5a8451398aa`; static inspection, no proposed implementation or runtime reproduction.

## src/adapters/persistence/integration/email_binding.rs:156–190

```text
156: /// Automatic endpoint synchronization, inside the transaction that registered or renamed the
157: /// principal's address. The transport records this automatic action with system provenance.
158: pub(crate) async fn sync_principal_email_binding_on(
159:     db: &mut PgConnection,
160:     company: Uuid,
161:     principal: PrincipalId,
162: ) -> AppResult<()> {
163:     #[derive(sqlx::FromRow)]
164:     struct AddressRow {
165:         slug: String,
166:         display_label: String,
167:     }
168:     let address: AddressRow = sqlx::query_as(
169:         "SELECT address.slug::text AS slug, principal.display_label \
170:          FROM addresses AS address JOIN principals AS principal \
171:            ON principal.company_id=address.company_id AND principal.id=address.principal_id \
172:          WHERE address.company_id=$1 AND address.principal_id=$2",
173:     )
174:     .bind(company)
175:     .bind(principal.as_uuid())
176:     .fetch_one(&mut *db)
177:     .await?;
178:     let slug = ChannelSlug::from(address.slug);
179:     write_canonical_email_binding(
180:         db,
181:         CanonicalEmailBinding {
182:             company_id: company,
183:             target: AddressTarget::Principal(principal),
184:             slug: &slug,
185:             name: &address.display_label,
186:             created_by: &CreationProvenance::system(),
187:         },
188:     )
189:     .await
190: }
```

## migrations/20260817000000_init_schema.sql:18670–18696

```text
18670: CREATE FUNCTION public.advance_principal_address_revision() RETURNS trigger LANGUAGE plpgsql AS $$
18671: DECLARE tenants uuid[]; targets uuid[]; owned record;
18672: BEGIN
18673:     IF TG_OP = 'UPDATE' THEN
18674:         IF (NEW.company_id, NEW.slug, NEW.principal_id, NEW.channel_id, NEW.is_primary)
18675:             IS NOT DISTINCT FROM (OLD.company_id, OLD.slug, OLD.principal_id, OLD.channel_id, OLD.is_primary) THEN
18676:             RETURN NEW;
18677:         END IF;
18678:         tenants := ARRAY[OLD.company_id, NEW.company_id];
18679:         targets := ARRAY[OLD.principal_id, NEW.principal_id];
18680:     ELSIF TG_OP = 'INSERT' THEN
18681:         tenants := ARRAY[NEW.company_id]; targets := ARRAY[NEW.principal_id];
18682:     ELSE tenants := ARRAY[OLD.company_id]; targets := ARRAY[OLD.principal_id]; END IF;
18683:     FOR owned IN
18684:         SELECT principal.company_id, principal.id FROM public.principals AS principal
18685:         WHERE EXISTS (SELECT 1 FROM unnest(tenants, targets) AS affected(company_id, id)
18686:             WHERE affected.company_id = principal.company_id AND affected.id = principal.id)
18687:         ORDER BY principal.company_id, principal.id FOR UPDATE OF principal
18688:     LOOP
18689:         UPDATE public.principals SET settings_revision = settings_revision + 1,
18690:             updated_at = CURRENT_TIMESTAMP WHERE company_id = owned.company_id AND id = owned.id;
18691:     END LOOP;
18692:     RETURN COALESCE(NEW, OLD);
18693: END $$;
18694: CREATE TRIGGER addresses_principal_revision AFTER INSERT OR UPDATE OR DELETE ON public.addresses
18695:     FOR EACH ROW EXECUTE FUNCTION public.advance_principal_address_revision();
18696: 
```

## src/adapters/protocols/email/types.rs:33–45

```text
33: /// What this leaves is exactly what a lookup has in hand:
34: /// `(company_id, channel_slug)`. Inbound routing parses the host off and resolves the company
35: /// before it can authorize anything anyway
36: /// ([`EmailAddressParser`](super::EmailAddressParser)), and the display form stays
37: /// a rendering concern where `Channel::inbound_address` already builds it from all three parts.
38: #[derive(Debug, Clone, PartialEq, Eq, Hash)]
39: pub struct EmailEndpointKey(ExternalEndpointKey);
40: 
41: impl EmailEndpointKey {
42:     /// The channel's canonical local part.
43:     pub fn canonical(channel: &ChannelSlug) -> Result<Self, TransportValueError> {
44:         Ok(Self(ExternalEndpointKey::parse(channel.to_string())?))
45:     }
```

## src/adapters/persistence/notification/agent_notice_message.rs:15–41

```text
15: pub(super) async fn commit_on(
16:     tx: &mut Transaction<'_, Postgres>,
17:     event: &NotificationEvent,
18:     source: &NoticeSource,
19:     route: &Route,
20:     key: &ExternalMessageKey,
21: ) -> AppResult<()> {
22:     let body: String=sqlx::query_scalar("SELECT format('A workflow needs action.\nReason: %s\nStep: %s\nOpen: /ui/workflow-decisions/escalations/%s',reason_code,COALESCE(step_id,'unavailable'),$3::uuid) \
23:         FROM workflow_escalation_cases WHERE company_id=$1 AND id=$2")
24:         .bind(event.company_id).bind(source.case_id).bind(source.decision_id).fetch_one(&mut **tx).await?;
25:     let request = request(event, route, body, key)?;
26:     // Canonical ingress retains its existing provider/thread key and dedup order.
27:     let stored = Box::pin(crate::adapters::persistence::thread::commit_platform_on(
28:         tx, &request,
29:     ))
30:     .await?;
31:     let [intent] = stored.intent_ids.as_slice() else {
32:         return Err(AppError::Internal(
33:             "An agent notice must commit exactly one intent".into(),
34:         ));
35:     };
36:     sqlx::query("INSERT INTO workflow_escalation_agent_receipts(company_id,notice_id,event_id,principal_id, \
37:         agent_id,desk_id,binding_id,binding_revision,message_id,intent_id) \
38:         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT(company_id,notice_id) DO NOTHING")
39:         .bind(event.company_id).bind(event.source_id).bind(event.id.as_uuid()).bind(source.principal_id)
40:         .bind(route.agent_id).bind(route.desk_id).bind(route.binding_id).bind(route.binding_revision)
41:         .bind(stored.message_id.as_uuid()).bind(intent).execute(&mut **tx).await?;
```

## src/adapters/persistence/notification/agent_notice_message.rs:45–78

```text
45: fn request(
46:     event: &NotificationEvent,
47:     route: &Route,
48:     body: String,
49:     key: &ExternalMessageKey,
50: ) -> AppResult<InboundCommitRequest> {
51:     let endpoint = qualified_email_identity(route.address.to_string())?;
52:     let binding = ChannelBindingId::new(route.transport_binding_id);
53:     Ok(InboundCommitRequest {
54:         company_id: event.company_id,
55:         envelope: InboundEnvelope {
56:             source: InboundSource {
57:                 binding_id: binding,
58:                 event_key: None,
59:                 message_key: key.clone(),
60:                 thread_key: ExternalThreadKey::parse(key.as_str())
61:                     .map_err(|e| AppError::Internal(e.to_string()))?,
62:             },
63:             // DTO requires a transport identity, but the PRIVATE platform writer
64:             // ignores it and resolves no sender/recipient account or observed handle.
65:             author: endpoint.clone(),
66:             addressed: BoundedVec::parse(
67:                 "notice recipients",
68:                 vec![AddressedIdentity::new(RecipientRole::To, endpoint)],
69:             )?,
70:             content: CanonicalContent::parse("Workflow escalation needs action", body)?,
71:             attachments: BoundedVec::empty(),
72:             reply_candidates: Default::default(),
73:             directives: IngressDirectives::default(),
74:             policy: IngressPolicyFacts::TrustedApplication,
75:             correlation_id: event.id.as_uuid().into(),
76:             extension: ProtocolExtension::email(EmailMessageMetadata::new(MessageId::from(
77:                 key.as_str(),
78:             ))),
```

## src/adapters/persistence/thread/inbound_target.rs:67–110

```text
67: pub(super) async fn hold_principal_settings(
68:     tx: &mut Transaction<'_, Postgres>,
69:     request: &InboundCommitRequest,
70:     author: super::InboundAuthor,
71: ) -> AppResult<()> {
72:     let identities = observed_identities(request, author);
73:     for identity in identities {
74:         lock_identity_key(tx, request.company_id, identity).await?;
75:     }
76:     let ids: Vec<_> = request
77:         .intents
78:         .iter()
79:         .filter_map(|intent| intent.target.principal_id())
80:         .map(PrincipalId::as_uuid)
81:         .collect();
82:     // One ordered acquisition, before binding/provider/thread locks or identity observations.
83:     sqlx::query(
84:         "SELECT id FROM principals WHERE company_id=$1 AND id=ANY($2) ORDER BY id FOR SHARE",
85:     )
86:     .bind(request.company_id)
87:     .bind(&ids)
88:     .fetch_all(&mut **tx)
89:     .await?;
90:     for intent in &request.intents {
91:         let Some(principal) = intent.target.principal_id() else {
92:             continue;
93:         };
94:         let current: bool = sqlx::query_scalar(
95:             "SELECT EXISTS(SELECT 1 FROM principals AS target WHERE target.company_id=$1 AND target.id=$2 \
96:              AND target.settings_revision=$3 AND EXISTS(SELECT 1 FROM addresses AS address \
97:                  WHERE address.company_id=$1 AND address.principal_id=target.id) \
98:              AND principal_direct_can_participate($1,target.id,(SELECT identity.principal_id \
99:                  FROM participant_identities AS identity WHERE identity.company_id=$1 AND identity.transport=$4 \
100:                  AND identity.namespace=$5 AND identity.subject=$6 AND identity.status='verified')))",
101:         ).bind(request.company_id).bind(principal.as_uuid()).bind(intent.settings_revision)
102:             .bind(request.envelope.author.transport().as_str()).bind(request.envelope.author.namespace().as_str())
103:             .bind(request.envelope.author.subject().as_str()).fetch_one(&mut **tx).await?;
104:         if !current {
105:             return Err(AppError::Superseded(
106:                 "Direct routing authority changed; route the message again".into(),
107:             ));
108:         }
109:     }
110:     Ok(())
```

## src/adapters/persistence/thread/inbound.rs:949–967

```text
949:         participants,
950:         correlation: correlation_of(envelope),
951:         created_at: Utc::now(),
952:     };
953:     if matches!(source, InboundAuthor::Platform) {
954:         if request.origin != crate::transport::InboundOrigin::Application
955:             || request.reply_delivery != ReplyDelivery::InAppOnly
956:             || !request.deliveries.is_empty()
957:         {
958:             return Err(AppError::BadRequest(
959:                 "A platform ingress must stay private".into(),
960:             ));
961:         }
962:         write.author = MessageAuthorWrite::Platform;
963:         write.role = MessageRole::System;
964:         write.audience = crate::entities::message::MessageAudience::InternalOnly;
965:         // Platform notices resolve no observed identities or member accounts.
966:         write.participants.clear();
967:     }
```

## src/adapters/persistence/workflow/admission.rs:22–34

```text
22:         authority::authorize_company(&mut tx, command.company_id(), command.actor()).await?;
23:         association::authorize_association(
24:             &mut tx,
25:             command.company_id(),
26:             command.actor(),
27:             command.association(),
28:         )
29:         .await?;
30:         admission_source::authorize(&mut tx, command).await?;
31:         if !claim_holds(&mut tx, command).await? {
32:             tx.rollback().await?;
33:             return Ok(AdmissionResult::Stale);
34:         }
```

## src/adapters/persistence/workflow/admission_source.rs:80–108

```text
80:     let author = sqlx::query_scalar::<_, Uuid>(
81:         "SELECT message.author_principal_id FROM messages AS message JOIN thread_messages AS membership \
82:            ON membership.company_id = message.company_id AND membership.message_id = message.id \
83:          WHERE message.company_id = $1 AND message.id = $2 \
84:            AND membership.channel_id IS NOT DISTINCT FROM $3::uuid AND membership.thread_id = $4 \
85:            AND membership.principal_id IS NOT DISTINCT FROM $5::uuid \
86:          FOR SHARE OF message, membership",
87:     )
88:     .bind(command.company_id().as_uuid())
89:     .bind(message)
90:     .bind(target.desk_id())
91:     .bind(thread_id.as_uuid())
92:     .bind(target.principal_id().map(|id| id.as_uuid()))
93:     .fetch_optional(&mut **tx)
94:     .await?
95:     .ok_or_else(missing)?;
96:     if let AddressTarget::Principal(principal) = target {
97:         // Association authorization holds the target SHARE gate and company serialization.
98:         // The original source stays on its exact target after address/policy edits.
99:         let allowed: bool = sqlx::query_scalar("SELECT principal_direct_can_participate($1,$2,$3)")
100:             .bind(command.company_id().as_uuid())
101:             .bind(principal.as_uuid())
102:             .bind(author)
103:             .fetch_one(&mut **tx)
104:             .await?;
105:         if !allowed {
106:             return Err(missing());
107:         }
108:     }
```

## migrations/20260817000000_init_schema.sql:16203–16223

```text
16203: CREATE FUNCTION public.principal_direct_can_participate(tenant uuid, target uuid, writer uuid)
16204: RETURNS boolean LANGUAGE sql STABLE AS $$
16205:     SELECT COALESCE((
16206:         SELECT CASE recipient.kind
16207:             WHEN 'agent' THEN recipient.open_to_public OR team.allowed
16208:             WHEN 'person' THEN team.allowed
16209:                 AND (actor.id = recipient.id OR actor.user_id = company.user_id)
16210:             ELSE false END
16211:         FROM public.principals AS recipient
16212:         JOIN public.companies AS company ON company.id = recipient.company_id
16213:         LEFT JOIN public.principals AS actor ON actor.company_id = recipient.company_id AND actor.id = writer
16214:         CROSS JOIN LATERAL (
16215:             SELECT COALESCE(actor.kind = 'agent' OR (actor.kind = 'person' AND (
16216:                 actor.user_id = company.user_id OR EXISTS (
16217:                     SELECT 1 FROM public.company_members AS membership
16218:                     WHERE membership.company_id = tenant AND membership.user_id = actor.user_id))), false) AS allowed
16219:         ) AS team
16220:         WHERE recipient.company_id = tenant AND recipient.id = target
16221:     ), false)
16222: $$;
16223: 
```
