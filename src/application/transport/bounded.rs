//! One definition of "this collection has a limit, and the limit is enforced where it is built".
//!
//! Every list that arrives from a provider -- addressed identities, attachments, reply candidates,
//! rendered parts -- has a maximum. Writing that maximum as a `const` next to a `Vec` field
//! documents it; writing it into the *type* is what rejects the input that exceeds it, which is
//! the difference `src/AGENTS.md` draws between advertising a bound and enforcing one.

use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::app_error::AppError;

/// A limit that input exceeded. Carries what was seen and what is allowed, because an operator
/// reading "too large" without either number cannot tell a misconfiguration from an attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BoundsError {
    #[error("{field} holds {actual} items, more than the {max} allowed")]
    TooMany {
        field: &'static str,
        actual: usize,
        max: usize,
    },
    #[error("{field} is {actual} bytes, more than the {max} allowed")]
    TooLarge {
        field: &'static str,
        actual: usize,
        max: usize,
    },
}

/// Input that breaks a documented limit is the caller's fault, not the server's.
impl From<BoundsError> for AppError {
    fn from(error: BoundsError) -> Self {
        AppError::BadRequest(error.to_string())
    }
}

/// Rejects `value` when it is longer than `max` bytes.
pub fn bounded_text(field: &'static str, value: &str, max: usize) -> Result<(), BoundsError> {
    (value.len() <= max)
        .then_some(())
        .ok_or(BoundsError::TooLarge {
            field,
            actual: value.len(),
            max,
        })
}

/// A `Vec` that cannot hold more than `MAX` items.
///
/// Derefs to `[T]`, so iteration, indexing and `len()` read exactly as they would on the `Vec` it
/// replaces; the only thing callers lose is the ability to construct one that is too long.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct BoundedVec<T, const MAX: usize>(Vec<T>);

impl<T, const MAX: usize> BoundedVec<T, MAX> {
    pub const MAX: usize = MAX;

    pub fn parse(field: &'static str, items: Vec<T>) -> Result<Self, BoundsError> {
        if items.len() > MAX {
            return Err(BoundsError::TooMany {
                field,
                actual: items.len(),
                max: MAX,
            });
        }
        Ok(Self(items))
    }

    pub fn empty() -> Self {
        Self(Vec::new())
    }

    pub fn into_inner(self) -> Vec<T> {
        self.0
    }

    /// Rebuild the list one element at a time. The length cannot grow, so the bound still holds
    /// and there is nothing to re-check -- which is what makes this safe to expose where a
    /// `into_inner().into_iter().map(..).collect()` round trip would silently drop the type.
    pub fn map<U>(self, transform: impl FnMut(T) -> U) -> BoundedVec<U, MAX> {
        BoundedVec(self.0.into_iter().map(transform).collect())
    }
}

impl<T, const MAX: usize> Default for BoundedVec<T, MAX> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T, const MAX: usize> Deref for BoundedVec<T, MAX> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<'a, T, const MAX: usize> IntoIterator for &'a BoundedVec<T, MAX> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T, const MAX: usize> IntoIterator for BoundedVec<T, MAX> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Deserialization is where a stored or provider-supplied list is at its most untrusted, so it
/// applies the same limit rather than trusting that whatever wrote the JSON respected it.
impl<'de, T, const MAX: usize> Deserialize<'de> for BoundedVec<T, MAX>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let items = Vec::<T>::deserialize(deserializer)?;
        Self::parse("collection", items).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_at_its_limit_is_accepted_and_one_past_it_is_not() {
        assert!(BoundedVec::<u8, 2>::parse("items", vec![1, 2]).is_ok());
        assert_eq!(
            BoundedVec::<u8, 2>::parse("items", vec![1, 2, 3]),
            Err(BoundsError::TooMany {
                field: "items",
                actual: 3,
                max: 2,
            })
        );
    }

    /// The bound has to survive a round trip: a stored list that grew past the limit by any other
    /// path must fail to decode rather than come back as an unbounded `Vec`.
    #[test]
    fn an_over_limit_list_is_refused_on_the_way_back_in_without_panicking() {
        let encoded = serde_json::to_string(&vec![1, 2, 3]).unwrap();
        assert!(serde_json::from_str::<BoundedVec<u8, 2>>(&encoded).is_err());
        assert_eq!(
            serde_json::from_str::<BoundedVec<u8, 3>>(&encoded)
                .unwrap()
                .into_inner(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn bounded_text_measures_bytes_rather_than_characters() {
        assert!(bounded_text("subject", "ab", 2).is_ok());
        // Two characters, four bytes: a limit that exists to bound allocation counts bytes.
        assert!(bounded_text("subject", "🦀", 2).is_err());
    }
}
