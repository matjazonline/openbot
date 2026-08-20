use std::sync::Arc;

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use tracing::{info, instrument};

use uuid::Uuid;

use crate::{
    app_error::{AppError, AppResult},
    entities::{user::User, value_objects::AvatarUrl},
};

#[async_trait]
pub trait UserPersistence: Send + Sync {
    async fn create_user(&self, username: &str, email: &str, password_hash: &str) -> AppResult<()>;
    async fn get_by_email(&self, email: &str) -> AppResult<Option<User>>;
    async fn get_by_username(&self, username: &str) -> AppResult<Option<User>>;
    async fn get_by_id(&self, id: Uuid) -> AppResult<Option<User>>;

    /// Stores the account's profile picture, or clears it with `None`. `Ok(None)` means no such
    /// user, so a stale session cannot silently write nothing.
    async fn update_avatar_url(
        &self,
        id: Uuid,
        avatar_url: Option<&AvatarUrl>,
    ) -> AppResult<Option<User>>;
}

pub trait UserCredentialsHasher: Send + Sync {
    fn hash_password(&self, password: &str) -> AppResult<String>;
    fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool>;
}

#[derive(Clone)]
pub struct UserUseCases {
    hasher: Arc<dyn UserCredentialsHasher>,
    persistence: Arc<dyn UserPersistence>,
}

impl UserUseCases {
    pub fn new(
        hasher: Arc<dyn UserCredentialsHasher>,
        persistence: Arc<dyn UserPersistence>,
    ) -> Self {
        Self {
            hasher,
            persistence,
        }
    }

    #[instrument(skip(self, password))]
    pub async fn add(&self, username: &str, email: &str, password: &SecretString) -> AppResult<()> {
        info!("Adding user...");

        let hash = self.hasher.hash_password(password.expose_secret())?;
        self.persistence.create_user(username, email, &hash).await?;

        info!("Adding user finished.");

        Ok(())
    }

    #[instrument(skip(self, password))]
    pub async fn login(&self, email_or_username: &str, password: &SecretString) -> AppResult<User> {
        info!("Attempting user login...");

        let user = if let Some(user) = self.persistence.get_by_email(email_or_username).await? {
            user
        } else if let Some(user) = self.persistence.get_by_username(email_or_username).await? {
            user
        } else {
            return Err(AppError::InvalidCredentials);
        };

        let is_valid = self
            .hasher
            .verify_password(password.expose_secret(), &user.password_hash)?;

        if !is_valid {
            return Err(AppError::InvalidCredentials);
        }

        info!("User login successful for {}", user.username);
        Ok(user)
    }

    #[instrument(skip(self))]
    pub async fn get_user_by_id(&self, id: Uuid) -> AppResult<Option<User>> {
        self.persistence.get_by_id(id).await
    }

    /// Points the account at a new profile picture, or back at its letter bubble with `None`.
    ///
    /// The URL arrives already parsed: [`AvatarUrl::parse`] is what rejects a scheme no `<img>`
    /// should be pointed at, and it runs where the form is read.
    #[instrument(skip(self))]
    pub async fn set_avatar(&self, id: Uuid, avatar_url: Option<&AvatarUrl>) -> AppResult<User> {
        info!("Updating avatar for user {id}...");

        self.persistence
            .update_avatar_url(id, avatar_url)
            .await?
            .ok_or_else(|| AppError::NotFound("User not found".into()))
    }
}

#[cfg(test)]
mod test {
    use async_trait::async_trait;
    use uuid::Uuid;

    use super::*;

    struct MockUserPersistence;

    #[async_trait]
    impl UserPersistence for MockUserPersistence {
        async fn create_user(
            &self,
            username: &str,
            email: &str,
            _password_hash: &str,
        ) -> AppResult<()> {
            assert_eq!(username, "testuser");
            assert_eq!(email, "testuser@gmail.com");

            Ok(())
        }

        async fn get_by_email(&self, email: &str) -> AppResult<Option<User>> {
            if email == "testuser@gmail.com" {
                Ok(Some(User {
                    id: Uuid::new_v4(),
                    username: "testuser".to_string(),
                    email: "testuser@gmail.com".to_string(),
                    password_hash: "secret_hash".to_string(),
                    avatar_url: None,
                    created_at: chrono::Utc::now(),
                }))
            } else {
                Ok(None)
            }
        }

        async fn get_by_username(&self, username: &str) -> AppResult<Option<User>> {
            if username == "testuser" {
                Ok(Some(User {
                    id: Uuid::new_v4(),
                    username: "testuser".to_string(),
                    email: "testuser@gmail.com".to_string(),
                    password_hash: "secret_hash".to_string(),
                    avatar_url: None,
                    created_at: chrono::Utc::now(),
                }))
            } else {
                Ok(None)
            }
        }

        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<User>> {
            Ok(Some(User {
                id: Uuid::new_v4(),
                username: "testuser".to_string(),
                email: "testuser@gmail.com".to_string(),
                password_hash: "secret_hash".to_string(),
                avatar_url: None,
                created_at: chrono::Utc::now(),
            }))
        }

        async fn update_avatar_url(
            &self,
            id: Uuid,
            avatar_url: Option<&AvatarUrl>,
        ) -> AppResult<Option<User>> {
            Ok(Some(User {
                id,
                username: "testuser".to_string(),
                email: "testuser@gmail.com".to_string(),
                password_hash: "secret_hash".to_string(),
                avatar_url: avatar_url.cloned(),
                created_at: chrono::Utc::now(),
            }))
        }
    }

    struct MockUserCredentialsHasher;

    impl UserCredentialsHasher for MockUserCredentialsHasher {
        fn hash_password(&self, password: &str) -> AppResult<String> {
            Ok(format!("{}_hash", password))
        }

        fn verify_password(&self, password: &str, hash: &str) -> AppResult<bool> {
            Ok(hash == &format!("{}_hash", password)
                || hash == "secret_hash" && password == "secret")
        }
    }

    #[tokio::test]
    async fn add_user_works() {
        let user_use_cases = UserUseCases::new(
            Arc::new(MockUserCredentialsHasher),
            Arc::new(MockUserPersistence),
        );

        let result = user_use_cases
            .add("testuser", "testuser@gmail.com", &"testuser_pw".into())
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn login_user_works() {
        let user_use_cases = UserUseCases::new(
            Arc::new(MockUserCredentialsHasher),
            Arc::new(MockUserPersistence),
        );

        let user = user_use_cases
            .login("testuser@gmail.com", &"secret".into())
            .await
            .unwrap();

        assert_eq!(user.username, "testuser");
    }
}
