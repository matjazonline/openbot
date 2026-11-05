use super::*;
#[tokio::test]
async fn knowledge_upload_unicode16_all_allowed_scalars_and_multilingual_names_match_postgres_identity()
 {
    let Some(db) = own_database().await else {
        return;
    };
    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!((180000..190000).contains(&version));
    let mut batch = Vec::with_capacity(4096);
    let mut checked = 0;
    for scalar in (0..=0x10ffff).filter_map(char::from_u32) {
        let candidate = format!("x{scalar}x.md");
        let Ok(parsed) = KnowledgeFileName::parse(&format!("0_{candidate}")) else {
            continue;
        };
        if parsed.name().as_str() != candidate {
            continue;
        }
        batch.push(candidate);
        if batch.len() == 4096 {
            check_fold(&db.pool, &batch).await;
            checked += batch.len();
            batch.clear();
        }
    }
    check_fold(&db.pool, &batch).await;
    checked += batch.len();
    assert!(checked > 1_000_000);
    check_fold(
        &db.pool,
        &[
            "Straße.md",
            "İstanbul.md",
            "ὈΔΥΣΣΕΎΣ.md",
            "Σςσ.md",
            "ﬃ.md",
            "Москва.md",
            "中文.md",
            "ԵՐԵՒԱՆ.md",
            "\u{1c89}.md",
        ]
        .map(String::from),
    )
    .await;
}
async fn check_fold(pool: &PgPool, names: &[String]) {
    let folded:Vec<String>=sqlx::query_scalar("SELECT casefold(scalar.value COLLATE pg_unicode_fast) FROM unnest($1::text[]) WITH ORDINALITY AS scalar(value,position) ORDER BY scalar.position")
        .bind(names).fetch_all(pool).await.unwrap();
    assert_eq!(folded.len(), names.len());
    for (name, actual) in names.iter().zip(folded) {
        let expected =
            crate::entities::value_objects::KnowledgeDocumentName::from(name.as_str()).key();
        assert_eq!(actual, expected.as_str(), "{name:?}");
    }
}
#[tokio::test]
async fn knowledge_upload_database_embedding_shape_finiteness_norm_and_tenant_level_matrix() {
    let Some(db) = own_database().await else {
        return;
    };
    let f = Fixture::new(db.pool.clone()).await;
    let other = Fixture::new(db.pool.clone()).await;
    let layer = f.replace(f.owner, "4_notes.md", "text").await;
    let valid = format!("{{1,{}}}", vec!["0"; 383].join(","));
    struct Case {
        vector: String,
        valid: bool,
    }
    let mut cases = vec![
        Case {
            vector: valid.clone(),
            valid: true,
        },
        Case {
            vector: format!("[0:383]={valid}"),
            valid: false,
        },
        Case {
            vector: "{}".into(),
            valid: false,
        },
        Case {
            vector: format!("{{{valid}}}"),
            valid: false,
        },
    ];
    for first in [
        "0",
        "NaN",
        "Infinity",
        "-Infinity",
        "NULL",
        "1.001",
        "0.999",
        "1.0001",
        "-1",
    ] {
        cases.push(Case {
            vector: format!("{{{first},{}}}", vec!["0"; 383].join(",")),
            valid: matches!(first, "1.0001" | "-1"),
        });
    }
    for length in [383, 385] {
        cases.push(Case {
            vector: format!("{{1,{}}}", vec!["0"; length - 1].join(",")),
            valid: false,
        });
    }
    for case in cases {
        let result: bool = sqlx::query_scalar("SELECT knowledge_embedding_is_valid($1::real[])")
            .bind(&case.vector)
            .fetch_one(&f.pool)
            .await
            .unwrap();
        assert_eq!(result, case.valid, "{}", case.vector);
        let mut tx = f.pool.begin().await.unwrap();
        let insertion=sqlx::query("INSERT INTO knowledge_chunks (id,company_id,document_id,access_level,ordinal,body,embedding_model,embedding) VALUES ($1,$2,$3,4,1,'body','test-model',$4::real[])")
            .bind(Uuid::new_v4()).bind(f.owner.company.as_uuid()).bind(layer.id).bind(&case.vector).execute(&mut *tx).await;
        assert_eq!(insertion.is_ok(), case.valid);
        tx.rollback().await.unwrap();
    }
    struct Reference {
        company: CompanyId,
        level: i16,
    }
    for reference in [
        Reference {
            company: other.owner.company,
            level: 4,
        },
        Reference {
            company: f.owner.company,
            level: 3,
        },
    ] {
        let error=sqlx::query("INSERT INTO knowledge_chunks (id,company_id,document_id,access_level,ordinal,body,embedding_model,embedding) VALUES ($1,$2,$3,$4,1,'body','test-model',$5::real[])")
            .bind(Uuid::new_v4()).bind(reference.company.as_uuid()).bind(layer.id).bind(reference.level).bind(&valid).execute(&f.pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23503")
        );
    }
    let error = sqlx::query("UPDATE knowledge_documents SET content='mismatch' WHERE id=$1")
        .bind(layer.id)
        .execute(&f.pool)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
}
