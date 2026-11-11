use super::*;
use crate::entities::skill_bundle::{SkillFile, skill_markdown_metadata, validate_skill_files};
use serde_json::json;

fn assets(script: &[u8], executable: bool) -> Vec<SkillFile> {
    vec![
        SkillFile::from_bytes("scripts/check.py".into(), script, executable).unwrap(),
        SkillFile::from_bytes("scripts/data.bin".into(), b"\0\xff\xfe\r\n", false).unwrap(),
    ]
}

fn directory(slug: &str) -> SkillWrite {
    let mut write = skill_write(slug);
    write.instructions = format!(
        "---\nname: {slug}\ndescription: Research facts\nallowed-tools: [Read, Bash]\n---\n\nRun scripts/check.py with its neighboring data.\n\n"
    );
    write.files = Some(assets(b"#!/usr/bin/env python3\nprint('one')\n", true));
    write
}

#[tokio::test]
async fn skill_bundle_persistence_create_update_preserve_and_scope_round_trip() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::new(pool);
    let owner = company(&p, "bundle-owner").await;
    let foreign = company(&p, "bundle-foreign").await;
    let initial = directory("research");
    let expected = initial.files.clone().unwrap();
    let created = p.create_company(owner, initial.clone()).await.unwrap();
    let read = p.get_company(owner, created.id).await.unwrap().unwrap();
    assert_eq!(read.instructions, initial.instructions);
    assert_eq!(read.files, expected);
    let mut update = initial.clone();
    update.instructions.push_str("Edit two.\n");
    update.files = Some(assets(b"print('two')\n", false));
    let updated = p
        .update_company(owner, created.id, update.clone())
        .await
        .unwrap();
    assert_eq!(updated.files, update.files.clone().unwrap());
    assert!(!updated.files[0].executable);
    let mut metadata_only = update.clone();
    metadata_only.files = None;
    metadata_only.name = "Edited metadata".into();
    p.update_company(owner, created.id, metadata_only)
        .await
        .unwrap();
    let current = p.get_company(owner, created.id).await.unwrap().unwrap();
    assert_eq!(current.files, updated.files);
    assert_eq!(current.instructions, update.instructions);
    assert!(p.get_company(foreign, created.id).await.unwrap().is_none());
    assert!(
        p.update_company(foreign, created.id, initial)
            .await
            .is_err()
    );
    update.files = Some(vec![]);
    p.update_company(owner, created.id, update).await.unwrap();
    assert!(
        p.get_company(owner, created.id)
            .await
            .unwrap()
            .unwrap()
            .files
            .is_empty()
    );
}

#[tokio::test]
async fn skill_bundle_library_copy_renames_only_new_identity_and_keeps_assets_independent() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::new(pool);
    let company = company(&p, "bundle-copy").await;
    let source = SkillManagementPersistence::create_library(
        &p,
        directory(&format!("research-{}", Uuid::new_v4().simple())),
    )
    .await
    .unwrap();
    let first = p
        .copy_library_to_company(company, source.id, CreationProvenance::system())
        .await
        .unwrap();
    let second = p
        .copy_library_to_company(company, source.id, CreationProvenance::system())
        .await
        .unwrap();
    assert_eq!(first.instructions, source.instructions);
    assert_eq!(second.slug.as_str(), format!("{}-2", source.slug));
    assert_eq!(
        skill_markdown_metadata(&second.instructions)
            .unwrap()
            .unwrap()
            .name,
        second.slug.as_str()
    );
    assert!(second.instructions.contains("allowed-tools:"));
    assert!(
        second
            .instructions
            .ends_with("\nRun scripts/check.py with its neighboring data.\n\n")
    );
    assert_eq!(first.files, source.files);
    assert_eq!(second.files, source.files);
    let mut changed = directory(second.slug.as_str());
    changed.files = Some(assets(b"print('copy changed')", false));
    p.update_company(company, second.id, changed).await.unwrap();
    let unchanged = p.get_library(source.id).await.unwrap().unwrap();
    assert_eq!(unchanged.instructions, source.instructions);
    assert_eq!(unchanged.files, source.files);
    assert_eq!(
        p.get_company(company, first.id)
            .await
            .unwrap()
            .unwrap()
            .files,
        source.files
    );
}

#[tokio::test]
async fn skill_bundle_database_constraints_match_the_domain_and_refuse_direct_invalid_writes() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let p = PostgresPersistence::new(pool.clone());
    let company = company(&p, "bundle-constraints").await;
    let skill = p
        .create_company(company, skill_write("bundle"))
        .await
        .unwrap();
    let valid = json!({"path":"scripts/run.py","content_base64":"AP/+DQo=","executable":true});
    let mut cases = vec![
        json!([]),
        json!([valid.clone()]),
        json!(null),
        json!({}),
        json!([{"path":"x"}]),
    ];
    for path in [
        "../x",
        "/x",
        "a//b",
        "C:/x",
        "a\\b",
        "SKILL.md",
        "skill.md/x",
        "a./x",
        "x ",
        "é.py",
    ] {
        let mut value = valid.clone();
        value["path"] = json!(path);
        cases.push(json!([value]));
    }
    for base64 in ["AA", "AB==", "AA==\n", "not base64!"] {
        let mut value = valid.clone();
        value["content_base64"] = json!(base64);
        cases.push(json!([value]));
    }
    let mut collision = valid.clone();
    collision["path"] = json!("SCRIPTS/RUN.py");
    cases.push(json!([valid.clone(), collision]));
    let mut parent = valid.clone();
    parent["path"] = json!("scripts");
    cases.push(json!([parent, valid]));
    for (index, value) in cases.into_iter().enumerate() {
        let rust = serde_json::from_value::<Vec<SkillFile>>(value.clone())
            .is_ok_and(|files| validate_skill_files(&files).is_ok());
        let sql: bool = sqlx::query_scalar("SELECT public.valid_skill_files($1)")
            .bind(&value)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(rust, index < 2, "domain case {index}");
        assert_eq!(rust, sql, "validation case {index}");
    }
    let mut tx = pool.begin().await.unwrap();
    let error = sqlx::query("UPDATE skills SET files=$1 WHERE company_id=$2 AND id=$3")
        .bind(json!([{"path":"../escape","content_base64":"AA==","executable":true}]))
        .bind(company)
        .bind(skill.id)
        .execute(&mut *tx)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();
}
