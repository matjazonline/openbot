//! Distinct nonnil UUID identities share wire validation, never their Rust type.
macro_rules! entity_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
        #[serde(transparent)]
        pub struct $name(pub uuid::Uuid);
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                d: D,
            ) -> std::result::Result<Self, D::Error> {
                let id = <uuid::Uuid as serde::Deserialize>::deserialize(d)?;
                if id.is_nil() {
                    return Err(serde::de::Error::custom("empty identity"));
                }
                Ok(Self(id))
            }
        }
    };
}
pub(crate) use entity_id;
