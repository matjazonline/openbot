//! Saving a channel edit under its gate, against a real database.
//!
//! The tests share the test database and scope every assertion to their own company.

use uuid::Uuid;

use super::*;
use crate::{
    adapters::persistence::{
        PostgresPersistence,
        channel_gate::{ChannelGateAccess, ChannelGateKey, acquire_channel_gate_on},
        test_support::test_pool,
    },
    use_cases::{
        channel::{ChannelPersistence, ChannelWrite},
        company::{CompanyPersistence, CompanyWrite},
        user::UserPersistence,
    },
};

/// A company with an "edited" channel and an "elsewhere" channel whose address an edit can
/// collide with.
struct Fixture {
    persistence: PostgresPersistence,
    company_id: Uuid,
    channel_id: Uuid,
}

async fn fixture(pool: &sqlx::PgPool) -> Fixture {
    let persistence = PostgresPersistence::new(pool.clone());
    let suffix = Uuid::new_v4().simple().to_string();
    let username = format!("edit_manager_{suffix}");
    let email = format!("{username}@example.com");
    persistence
        .create_user(&username, &email, "hash")
        .await
        .unwrap();
    let manager = UserPersistence::get_by_email(&persistence, &email)
        .await
        .unwrap()
        .unwrap();
    let company = CompanyPersistence::create(
        &persistence,
        manager.id,
        CompanyWrite {
            name: "Edit Test".into(),
            slug: format!("edit-test-{suffix}"),
            ..CompanyWrite::default()
        },
    )
    .await
    .unwrap();
    let channel = ChannelPersistence::create(&persistence, company.id, write("edited", "Edited"))
        .await
        .unwrap();
    ChannelPersistence::create(&persistence, company.id, write("elsewhere", "Elsewhere"))
        .await
        .unwrap();
    Fixture {
        persistence,
        company_id: company.id,
        channel_id: channel.id,
    }
}

fn write(slug: &str, name: &str) -> ChannelWrite {
    ChannelWrite {
        name: name.into(),
        slug: slug.into(),
        enabled: false,
        ..ChannelWrite::default()
    }
}

impl Fixture {
    fn request(&self, name: &str) -> ChannelUpdate {
        ChannelUpdate {
            company_id: self.company_id,
            channel_id: self.channel_id,
            write: write("edited", name),
        }
    }

    async fn name(&self) -> String {
        ChannelPersistence::get_by_id(&self.persistence, self.channel_id)
            .await
            .unwrap()
            .unwrap()
            .name
    }

    async fn delete(self) {
        CompanyPersistence::delete(&self.persistence, self.company_id)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_failed_edit_rolls_back_every_setting_with_it() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;

    // The slug write fails after the name was written: the address is taken.
    let mut request = f.request("Renamed");
    request.write.slug = "elsewhere".into();
    let refused = update_channel_within(&pool, request, EditBudget::DEFAULT).await;

    assert!(refused.is_err());
    assert_eq!(f.name().await, "Edited");
    f.delete().await;
}

#[tokio::test]
async fn an_edit_of_another_companys_channel_finds_nothing() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;
    let mut request = f.request("Stolen");
    request.company_id = Uuid::new_v4();

    let refused = update_channel_within(&pool, request, EditBudget::DEFAULT).await;

    assert!(matches!(refused, Err(AppError::NotFound(_))), "{refused:?}");
    assert_eq!(f.name().await, "Edited");
    f.delete().await;
}

/// The database giving up on a lock, or the edit outliving its deadline, is a timeout that says
/// nothing changed -- not a database fault.
#[tokio::test]
async fn an_edit_that_cannot_take_its_locks_in_time_is_refused_as_a_timeout() {
    let Some(pool) = test_pool().await else {
        return;
    };
    let f = fixture(&pool).await;
    let mut admission = pool.begin().await.unwrap();
    acquire_channel_gate_on(
        &mut admission,
        ChannelGateKey::new(f.company_id, f.channel_id),
        ChannelGateAccess::Admit,
    )
    .await
    .unwrap();

    let impatient = EditBudget {
        lock_timeout: Duration::from_millis(100),
        ..EditBudget::DEFAULT
    };
    let refused = update_channel_within(&pool, f.request("Renamed"), impatient).await;
    assert!(
        matches!(&refused, Err(AppError::Timeout(message)) if message.contains("nothing was changed")),
        "{refused:?}"
    );

    let hurried = EditBudget {
        lock_timeout: Duration::from_secs(10),
        deadline: Duration::from_millis(300),
    };
    let refused = update_channel_within(&pool, f.request("Renamed"), hurried).await;
    assert!(
        matches!(&refused, Err(AppError::Timeout(message)) if message.contains("nothing was changed")),
        "{refused:?}"
    );

    admission.rollback().await.unwrap();
    assert_eq!(f.name().await, "Edited");
    f.delete().await;
}
