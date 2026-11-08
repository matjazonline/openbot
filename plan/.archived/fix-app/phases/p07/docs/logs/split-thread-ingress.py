from pathlib import Path
p=Path('src/application/use_cases/thread/mod.rs');s=p.read_text()
def function_end(text,start):
 i=text.index('{',start);depth=1;j=i+1;quoted=False;escaped=False;comment=False
 while depth:
  c=text[j]
  if comment:
   if c=='\n':comment=False
  elif quoted:
   if escaped:escaped=False
   elif c=='\\':escaped=True
   elif c=='"':quoted=False
  elif text[j:j+2]=='//':comment=True;j+=1
  elif c=='"':quoted=True
  elif c=='{':depth+=1
  elif c=='}':depth-=1
  j+=1
 return j
# Relays are an ingress boundary. Remove the existing async forwarding layer while relocating it.
a=s.index('#[async_trait]\nimpl InternalMailRelay');b=s.index('\n#[cfg(test)]',a);trait=s[a:b];s=s[:a]+s[b:]
a=s.index('    async fn resolve_internal_destination(');b=s.index('    /// Tell a sender their message could not be routed.',a);relay=s[a:b];s=s[:a]+s[b:]
a=relay.index('    /// Ingest a message one channel');b=relay.index('    /// The canonical inbound message one internal hop',a);relay=relay[:a]+relay[b:]
trait=trait.replace('''        let ingest = self
            .ingest_relayed_message(mail, &selector, company_id)
            .await?;''','''        let message = self.internal_inbound_message(mail, &selector, company_id)?;
        let ingest = self.ingest(message).await?;''')
relay=relay.replace('AppResult<Option<(Channel, Channel, Company)>>','AppResult<Option<RelaySource>>').replace('Ok(Some((source, target, company)))','Ok(Some(RelaySource { channel: source, company }))').replace('let Some((source, _target, company)) = self','let Some(RelaySource { channel: source, company }) = self')
a=relay.index('        let routing = InboundRouting::parse(');b=relay.index('\n\n        Ok(InboundMessage::arriving',a)
relay=relay[:a]+'''        let routing = super::single_channel_routing(recipient_handle, selector.clone(), disposition)?;'''+relay[b:]
Path('src/application/use_cases/thread/ingest/relay.rs').write_text('''//! Typed internal transport ingress, without a MIME round trip.
use super::super::*;
struct RelaySource { channel: Channel, company: Company }
'''+trait+'\nimpl ThreadUseCases {\n'+relay+'\n}\n')
# Notices keep their formatting and durable dispatch together.
a=s.index('    /// Tell a sender their message could not be routed.');b=function_end(s,s.index('    pub async fn handle_bounce_dispatch(',a));dispatch=s[a:b];s=s[:a]+s[b:]
a=s.index('#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]\npub struct BounceSuggestion');b=s.index('/// What one inbound message became.',a);notice=s[a:b];s=s[:a]+s[b:]
Path('src/application/use_cases/thread/notices.rs').write_text('''//! Bounce/help notices and their durable dispatch.
use super::*;
use serde::{Deserialize, Serialize};
impl ThreadUseCases {
'''+dispatch+'\n}\n'+notice)
s=s.replace('mod support;','''mod support;
mod notices;
pub use notices::{BounceInfo, BounceSuggestion, ChannelDirectoryEntry, bounce_cause, format_bounce_email_body, format_help_email_body};''')
# Canonical ingress moves next to the other ingress producers.
a=s.index('    /// Ingest a canonical message into a channel');b=function_end(s,s.index('    pub async fn ingest_canonical(',a));s=s[:a]+s[b:];p.write_text(s)
p=Path('src/application/use_cases/thread/ingest/mod.rs');s=p.read_text().replace('pub(crate) mod commit;','mod canonical;\nmod relay;\npub(crate) mod commit;')
s+='''
/// Both trusted ingress producers address one channel through the same bounded routing shape.
fn single_channel_routing(handle: crate::entities::transport::QualifiedIdentity, selector: crate::entities::transport::ChannelSelector, disposition: crate::transport::MessageDisposition) -> AppResult<crate::transport::InboundRouting> {
    use crate::transport::{AddressedRecipient, AddressedTarget, InboundRouting, RecipientRole};
    InboundRouting::parse(vec![AddressedRecipient { role: RecipientRole::To, handle, target: AddressedTarget::Channels(vec![selector]), disposition }])
}
''';p.write_text(s)
