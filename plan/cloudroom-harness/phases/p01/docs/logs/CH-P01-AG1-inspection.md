# CH-P01-AG1 inspection evidence — 2026-11-10

Decision CH-P01-M1/r1 SHA256 e24182f35493e0a275faa79a0f57479d2f7893907728bee3d379d6c2b4f2fc56
HEAD abac7038a08a355156b7680a01d2f79f3de756fd
Branch codex/cloudroom-harness-t1gfnhz1

Pure serialized-size calculation (not an HTTP execution): one allowed 256 KiB asset encodes to 349528 base64 bytes; compact JSON body is 349718 bytes; current shared route limit is 163840 bytes. The asset count and total bytes are within proposed bounds.

Source excerpts from the unchanged review-base commit:

## src/adapters/persistence/skill.rs:133-190

```text
133: fn copied_slug(original: &str, suffix: u8) -> String {
134:     if suffix == 1 {
135:         return original.to_string();
136:     }
137:     let ending = format!("-{suffix}");
138:     let stem_len = SkillSlug::MAX_CHARS - ending.len();
139:     format!("{}{}", &original[..original.len().min(stem_len)], ending)
140: }
141: 
142: /// Copy one library row inside the caller's transaction, retrying only slug occupancy.
143: pub(crate) async fn copy_library_skill_on(
144:     transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
145:     company_id: Uuid,
146:     skill_id: Uuid,
147:     created_by: &CreationProvenance,
148: ) -> AppResult<Skill> {
149:     let source = sqlx::query_as::<_, SkillDb>(&format!(
150:         "SELECT {SKILL_COLUMNS} FROM skills AS skill \
151:          WHERE skill.company_id IS NULL AND skill.id = $1"
152:     ))
153:     .bind(skill_id)
154:     .fetch_optional(&mut **transaction)
155:     .await
156:     .map_err(AppError::from)?
157:     .ok_or_else(|| AppError::NotFound("Library skill not found.".into()))?;
158:     let source: Skill = source.try_into()?;
159:     let provenance =
160:         serde_json::to_value(created_by).map_err(|error| AppError::Internal(error.to_string()))?;
161: 
162:     for suffix in 1..=100 {
163:         let candidate = copied_slug(source.slug.as_str(), suffix);
164:         let row = sqlx::query_as::<_, SkillDb>(&format!(
165:             "INSERT INTO skills AS skill \
166:                  (id, company_id, slug, name, description, instructions, created_by, required_tools) \
167:              SELECT $1, $2, $3, $4, $5, $6, $7, source.required_tools FROM skills AS source WHERE source.id = $8 \
168:              ON CONFLICT (company_id, slug) DO NOTHING \
169:              RETURNING {SKILL_COLUMNS}"
170:         ))
171:         .bind(Uuid::new_v4())
172:         .bind(company_id)
173:         .bind(candidate)
174:         .bind(&source.name)
175:         .bind(&source.description)
176:         .bind(&source.instructions)
177:         .bind(&provenance)
178:         .bind(skill_id)
179:         .fetch_optional(&mut **transaction)
180:         .await
181:         .map_err(write_error)?;
182:         if let Some(row) = row {
183:             return row.try_into();
184:         }
185:     }
186: 
187:     Err(AppError::Conflict(
188:         "Could not choose an available slug after 100 bounded attempts.".into(),
189:     ))
190: }
```

## src/adapters/persistence/channel.rs:560-578

```text
560:     let source_skill_ids = sqlx::query_scalar::<_, Uuid>(
561:         "SELECT skill_id FROM agent_skills \
562:              WHERE agent_id = $1 ORDER BY position",
563:     )
564:     .bind(library_agent_id)
565:     .fetch_all(&mut **tx)
566:     .await
567:     .map_err(AppError::from)?;
568:     let created_by = agent
569:         .created_by
570:         .clone()
571:         .unwrap_or_else(CreationProvenance::system);
572:     let mut copied_skill_ids = Vec::with_capacity(source_skill_ids.len());
573:     for source_skill_id in source_skill_ids {
574:         let copied = copy_library_skill_on(tx, company_id, source_skill_id, &created_by).await?;
575:         copied_skill_ids.push(copied.id);
576:     }
577:     agent.skill_ids = copied_skill_ids;
578: 
```

## src/adapters/http/routes/skill_library.rs:38-81

```text
38: const SKILL_FORM_BODY_LIMIT: usize = 6 * MAX_SKILL_INSTRUCTIONS_BYTES + 64 * 1024;
39: 
40: pub fn router() -> Router<AppState> {
41:     Router::new()
42:         .route(
43:             "/ui/companies/{company_id}/skills",
44:             get(company_section).post(create_company_skill),
45:         )
46:         .route(
47:             "/ui/companies/{company_id}/skills/library",
48:             get(company_library_browser),
49:         )
50:         .route(
51:             "/ui/companies/{company_id}/skills/from-library",
52:             post(copy_company_skill),
53:         )
54:         .route(
55:             "/ui/companies/{company_id}/skills/{skill_id}",
56:             get(company_skill)
57:                 .put(update_company_skill)
58:                 .delete(delete_company_skill),
59:         )
60:         .route(
61:             "/api/skill-library",
62:             get(list_library_json).post(create_library_json),
63:         )
64:         .route(
65:             "/api/skill-library/{skill_id}",
66:             get(get_library_json)
67:                 .put(update_library_json)
68:                 .delete(delete_library_json),
69:         )
70:         .route(
71:             "/ui/skill-library",
72:             get(library_workspace).post(create_library_skill),
73:         )
74:         .route(
75:             "/ui/skill-library/{skill_id}",
76:             get(library_skill)
77:                 .put(update_library_skill)
78:                 .delete(delete_library_skill),
79:         )
80:         .layer(DefaultBodyLimit::max(SKILL_FORM_BODY_LIMIT))
81: }
```

## src/adapters/http/routes/skill_library.rs:654-663

```text
654:     fn the_body_limit_admits_the_largest_valid_skill_as_json() {
655:         let payload = SkillPayload {
656:             slug: "s".repeat(120),
657:             name: "\u{1}".repeat(120),
658:             description: "\u{1}".repeat(500),
659:             instructions: "\u{1}".repeat(MAX_SKILL_INSTRUCTIONS_BYTES),
660:         };
661:         let body = serde_json::to_vec(&payload).unwrap();
662:         assert!(body.len() <= SKILL_FORM_BODY_LIMIT, "{} bytes", body.len());
663:     }
```

## src/adapters/persistence/agent_publication/capture.rs:194-210

```text
194: fn resolved_skill(
195:     company: CompanyId,
196:     assignment: Uuid,
197:     source: &SkillSource,
198: ) -> AppResult<ResolvedAgentSkill> {
199:     let skill = &source.skill;
200:     Ok(ResolvedAgentSkill {
201:         assignment_id: AgentSkillAssignmentId::new(assignment),
202:         skill: SkillSnapshot {
203:             company_id: company,
204:             skill_id: skill.id,
205:             name: TypeName::parse(skill.slug.as_str()).map_err(|_| invalid())?,
206:             instructions: skill.instructions.clone(),
207:             required_tools: source.required_tools.clone(),
208:         },
209:     })
210: }
```
