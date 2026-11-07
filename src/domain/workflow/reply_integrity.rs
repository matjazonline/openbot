//! Bounded canonical SHA-256 identity for an immutable final-response proposal.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReplyIntegrity([u8; 32]);

impl ReplyIntegrity {
    pub fn from_sha256(value: [u8; 32]) -> Self {
        Self(value)
    }
    pub fn as_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut result = String::with_capacity(64);
        for byte in self.0 {
            result.push(char::from(HEX[usize::from(byte >> 4)]));
            result.push(char::from(HEX[usize::from(byte & 15)]));
        }
        result
    }
}
impl TryFrom<String> for ReplyIntegrity {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != 64 {
            return Err("Invalid final reply integrity");
        }
        let mut result = [0; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            let high = nibble(pair[0]).ok_or("Invalid final reply integrity")?;
            let low = nibble(pair[1]).ok_or("Invalid final reply integrity")?;
            result[index] = (high << 4) | low;
        }
        Ok(Self(result))
    }
}
impl From<ReplyIntegrity> for String {
    fn from(value: ReplyIntegrity) -> Self {
        value.as_hex()
    }
}
fn nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}
