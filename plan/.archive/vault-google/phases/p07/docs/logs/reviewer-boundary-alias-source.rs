use secrecy::{SecretString as PrivateSecret, ExposeSecret};
pub struct Leaked { value: PrivateSecret }
impl serde::Serialize for Leaked {
 fn serialize<S: serde::Serializer>(&self, serializer: S)->Result<S::Ok,S::Error> {
  serializer.serialize_str(self.value.expose_secret())
 }
}
pub enum Reply { Leak(Leaked) }
