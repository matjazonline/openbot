from pathlib import Path
import re,json
root=Path('.')
ports=Path('src/application/ports');ports.mkdir()

def cut(path,start,end):
 s=path.read_text();i=s.index(start);j=s.index(end,i);part=s[i:j];path.write_text(s[:i]+s[j:]);return part

channel=Path('src/application/use_cases/channel.rs')
write=cut(channel,'/// Everything one channel write','impl ChannelWrite {')
contract=cut(channel,'/// One authorized channel edit','/// The ports the channel read rule')
(ports/'channel.rs').write_text('''//! Shared channel persistence contract, independent of use-case orchestration.
use async_trait::async_trait;
use uuid::Uuid;
use crate::{app_error::AppResult, entities::{channel::Channel, creation::CreationProvenance, value_objects::{ChannelSlug, CompanySlug}}};
'''+write+contract)

participant=Path('src/application/use_cases/participant/mod.rs')
observation=cut(participant,'/// One sighting of a transport handle.','#[derive(Debug, Clone)]\npub struct IdentityResolution')
access=cut(participant,'/// The actor facts channel policy needs,','pub trait ParticipantPersistence:')
(ports/'participant.rs').write_text('''//! Shared principal-access facts and observed identity contract.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::{app_error::AppResult, entities::{participant::{IdentityClaimMetadata, IdentityProvenance, PrincipalAccessContext}, transport::QualifiedIdentity}};
'''+observation+access)

integration=Path('src/application/use_cases/integration.rs')
limit=cut(integration,'/// The bound on one manager-facing binding listing.','/// Everything installing')
binding=cut(integration,'/// Everything creating one protocol interface','/// The rules that hold across an installation')
(ports/'integration.rs').write_text('''//! Channel-binding persistence and its audited write contract.
use async_trait::async_trait;
use uuid::Uuid;
use crate::{app_error::{AppError, AppResult}, entities::{creation::CreationProvenance, transport::{BindingAccessPolicy, BindingAccessSnapshot, BindingAuditAction, BindingAuditEvent, BindingChangeReason, BindingDeliveryPolicy, BindingStatus, ChannelBinding, ChannelBindingId, EndpointNamespace, ExternalEndpointKey, InstallationId, TransportKind}}};
'''+limit+binding)

thread=Path('src/application/use_cases/thread/mod.rs')
thread_port=cut(thread,'#[async_trait]\npub trait ThreadPersistence:','#[derive(Clone)]\npub struct ThreadUseCases')
identity=cut(thread,'pub fn qualified_email_identity(','/// One RFC Message-ID')
(ports/'thread').mkdir()
(ports/'thread/mod.rs').write_text('''//! Shared message writes and thread persistence; no runtime or use-case dependency.
use std::collections::HashMap;
use async_trait::async_trait;
use uuid::Uuid;
use crate::{app_error::{AppError, AppResult}, entities::{cursor::{MessageCursor, ThreadCursor}, internal_note::{AddInternalNote, InternalNoteView, TombstoneInternalNote}, message::{CanonicalMessageId, Message, MessageRole, ThreadEntryKind}, message_view::{AgentHistoryMessage, EmailReplyContext, MessageAuditView, ThreadMessageView}, thread::Thread, transport::{IdentityNamespace, IdentitySubject, QualifiedIdentity}, value_objects::{EmailAddress, MessageId, ThreadIndex}}};
mod message_write;
mod protocol_extension;
pub use message_write::{AgentAuthor, MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, MessageWrite};
pub use protocol_extension::ProtocolExtension;
'''+identity+thread_port)
p=Path('src/application/use_cases/thread/message_write.rs');(ports/'thread/message_write.rs').write_text(p.read_text());p.unlink()
s=thread.read_text();a=s.index('mod message_write;');b=s.index('mod support;',a)
s=s[:a]+'''use crate::application::ports::thread::{MessageAuthorWrite, MessageCorrelation, MessageParticipantWrite, MessageWrite, ThreadPersistence, qualified_email_identity};
'''+s[b:];thread.write_text(s)

ingress=Path('src/application/transport/ingress.rs')
extension=cut(ingress,'/// The provider-specific facts that survive','/// The keys that might identify')
(ports/'thread/protocol_extension.rs').write_text('''//! Persisted protocol metadata shared by message producers and readers.
use serde::{Deserialize, Serialize};
use crate::entities::{email_message::EmailMessageMetadata, transport::{ChannelBindingId, ExternalEventKey}};
'''+extension)
(ports/'mod.rs').write_text('''//! Ports shared across application subsystems. Contracts depend on domain and pure values.
pub mod channel;
pub mod integration;
pub mod participant;
pub mod thread;
''')
p=Path('src/application/mod.rs');p.write_text(p.read_text().replace('pub mod notification;','pub mod notification;\npub mod ports;'))

moves={}
for mod,names in {'channel':['ChannelPersistence','ChannelWrite','ChannelUpdate'],'participant':['PrincipalAccessPersistence','IdentityObservation'],'thread':['ThreadPersistence','MessageWrite','AgentAuthor','MessageCorrelation','MessageParticipantWrite','MessageAuthorWrite','qualified_email_identity'],'integration':['ChannelBindingPersistence','BindingWrite','BindingStatusChange','InboundEndpoint','MAX_BINDING_AUDIT_EVENTS']}.items():
 for name in names:
  for base in ('crate::use_cases::','crate::application::use_cases::'):
   moves[tuple((base+mod+'::'+name).split('::'))]=tuple(('crate::application::ports::'+mod+'::'+name).split('::'))
for base in ('crate::transport','crate::application::transport','crate::transport::ingress','crate::application::transport::ingress'):
 moves[tuple((base+'::ProtocolExtension').split('::'))]=tuple('crate::application::ports::thread::ProtocolExtension'.split('::'))

# Parse each Rust use tree, relocating only matching leaves. Keep aliases and unchanged trees intact.
def leaves(code):
 code=re.sub(r'//[^\n]*|/\*.*?\*/','',code,flags=re.S)
 tokens=re.findall(r'r#\w+|\w+|::|[{},*]',code);pos=0;out=[]
 def one(prefix):
  nonlocal pos
  if tokens[pos]=='{':
   pos+=1
   while tokens[pos]!='}':
    one(prefix)
    if tokens[pos]==',':pos+=1
   pos+=1;return
  word=tokens[pos];pos+=1;path=prefix+[word]
  if pos<len(tokens) and tokens[pos]=='::':pos+=1;one(path);return
  alias=''
  if pos<len(tokens) and tokens[pos]=='as':
   pos+=1;alias=' as '+tokens[pos];pos+=1
  out.append((tuple(path),alias))
 one([])
 assert pos==len(tokens),(code,tokens[pos:])
 return out

def rewrite(s):
 pattern=re.compile(r'(?m)^([ \t]*)(pub(?:\([^\n]*?\))?\s+)?use\s+([^;]+);')
 def statement(m):
  try:items=leaves(m[3])
  except (AssertionError,IndexError):return m[0]
  if not any(path in moves for path,alias in items):return m[0]
  return '\n'.join(m[1]+(m[2] or '')+'use '+'::'.join(moves.get(path,path))+alias+';' for path,alias in items)
 s=pattern.sub(statement,s)
 for before,after in sorted(moves.items(),key=lambda x:-len(x[0])):
  s=re.sub(r'(?<![\w:])'+re.escape('::'.join(before))+r'\b','::'.join(after),s)
 return s
files=set()
for paths in json.loads(Path('plan/fix-app/phases/p07/docs/logs/move-sites-files.json').read_text()).values():files.update(paths)
files.update(str(p) for p in ports.rglob('*.rs'))
for f in sorted(files):
 p=Path(f)
 if p.exists():
  s=p.read_text();updated=rewrite(s)
  if updated!=s:p.write_text(updated)

# Owners privately import their moved contracts for orchestration; old API paths no longer export them.
for file,imp in [(channel,'channel::{ChannelPersistence, ChannelWrite, ChannelUpdate}'),(participant,'participant::{IdentityObservation, PrincipalAccessPersistence}'),(integration,'integration::{BindingWrite, BindingStatusChange, ChannelBindingPersistence, InboundEndpoint, MAX_BINDING_AUDIT_EVENTS}'),(ingress,'thread::ProtocolExtension')]:
 s=file.read_text();idx=s.index('\nuse ');s=s[:idx]+f'\nuse crate::application::ports::{imp};\n'+s[idx:];file.write_text(s)
print('Shared contracts moved; imports rewritten in',len(files),'traced files')
