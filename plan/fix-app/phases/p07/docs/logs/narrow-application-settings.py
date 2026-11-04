from pathlib import Path
import re
logs=Path('plan/fix-app/phases/p07/docs/logs')
p=Path('src/domain/entities/value_objects.rs');s=p.read_text().replace('string_newtype!(CompanySlug);','''string_newtype!(CompanySlug);

// Deployment mail domain and public application origin have distinct uses.
string_newtype!(MailDomain);
string_newtype!(PublicBaseUrl);''');p.write_text(s)
Path('src/application/settings.rs').write_text('''//! The deployment values application orchestration actually reads.
use crate::entities::value_objects::{MailDomain, PublicBaseUrl};
#[derive(Debug, Clone)]
pub struct ThreadSettings {
    pub mail_domain: MailDomain,
    pub max_spam_score: f64,
}
#[derive(Debug, Clone)]
pub struct NotificationMailSettings {
    pub mail_domain: MailDomain,
    pub public_base_url: PublicBaseUrl,
}
''')
p=Path('src/application/mod.rs');p.write_text(p.read_text().replace('pub mod session;','pub mod session;\npub mod settings;'))
p=Path('src/infra/config.rs');s=p.read_text();i=s.index('    pub fn is_spam_scan_enabled(')
s=s[:i]+'''    pub fn thread_settings(&self) -> std::sync::Arc<crate::application::settings::ThreadSettings> {
        std::sync::Arc::new(crate::application::settings::ThreadSettings { mail_domain: self.app_domain_name.clone().into(), max_spam_score: self.max_spam_score })
    }
    pub fn notification_settings(&self) -> crate::application::settings::NotificationMailSettings {
        crate::application::settings::NotificationMailSettings { mail_domain: self.app_domain_name.clone().into(), public_base_url: self.public_base_url().into() }
    }
    pub fn spam_scanning(&self) -> crate::use_cases::agent::SpamScanning {
        if self.is_spam_scan_enabled() { crate::use_cases::agent::SpamScanning::Available } else { crate::use_cases::agent::SpamScanning::Unavailable }
    }

'''+s[i:];p.write_text(s)

# Preserve all thread policy settings; discard fields the application never read.
files=set(re.findall(r'src/[^:\s()]+\.rs',(logs/'thread-config-sites.txt').read_text()))
files.update(re.findall(r'src/[^:\s()]+\.rs',(logs/'thread-config-field-sites.txt').read_text()))
for name in files:
 p=Path(name);s=p.read_text()
 if name.endswith('/tests.rs'):
  edits=[]
  for m in re.finditer(r'AppConfig\s*\{',s):
   start=m.start();j=m.end();depth=1
   while depth:
    if s[j]=='{':depth+=1
    if s[j]=='}':depth-=1
    j+=1
   body=s[m.end():j-1]
   domain=re.search(r'app_domain_name:\s*([^\n,]+)',body);score=re.search(r'max_spam_score:\s*([^\n,]+)',body)
   assert domain and score,(name,body)
   edits.append((start,j,'ThreadSettings { mail_domain: ('+domain[1]+').into(), max_spam_score: '+score[1]+' }'))
  for a,b,text in reversed(edits):s=s[:a]+text+s[b:]
  s=s.replace('use crate::infra::config::ResendApiConfig;\n','')
  # File storage belongs to the ingress adapter; this preflight fixture needs only mail settings.
  start=s.index('    let mut storage_config = internal_test_config();')
  end=s.index('    let storage = ',start)
  s=s[:start]+'    let storage_fixture = channel_fixture_with_config(TestChannel::default(), internal_test_config());\n'+s[end:]
  s=s.replace('crate::adapters::protocols::email::EmailIngressAdapter::for_config(\n        storage_fixture.use_cases.config(),\n    )','crate::adapters::protocols::email::EmailIngressAdapter::new(&storage_fixture.use_cases.settings.mail_domain)')
 if name.endswith('/mod.rs') and '/thread/mod.rs' in name:
  s=s.replace('use crate::infra::config::AppConfig;','use crate::application::settings::ThreadSettings;')
  start=s.index('    pub fn config(&self)');end=s.index('\n    }',start)+len('\n    }');s=s[:start]+s[end:]
  s=s.replace('    config: Arc<AppConfig>,','    settings: Arc<ThreadSettings>,').replace('        config: Arc<AppConfig>,','        settings: Arc<ThreadSettings>,').replace('            config,','            settings,')
 else:
  s=s.replace('use crate::infra::config::AppConfig;','use crate::application::settings::ThreadSettings;').replace('Arc<AppConfig>','Arc<ThreadSettings>')
 s=s.replace('self.config.app_domain_name','self.settings.mail_domain').replace('self.config.max_spam_score','self.settings.max_spam_score').replace('config.app_domain_name','config.mail_domain')
 s=s.replace('EmailIngressAdapter::for_config(self.config())','EmailIngressAdapter::new(&self.settings.mail_domain)')
 s=s.replace('internal_test_config','internal_test_settings').replace('channel_fixture_with_config','channel_fixture_with_settings')
 p.write_text(s)

p=Path('src/application/use_cases/channel.rs');s=p.read_text().replace('    infra::config::AppConfig,\n','').replace('    config: Arc<AppConfig>,','    spam_scanning: SpamScanning,').replace('        config: Arc<AppConfig>,','        spam_scanning: SpamScanning,').replace('            config,','            spam_scanning,')
a=s.index('    /// This deployment\'s spam scanning,');b=s.index('    fn check_spam_interlock(',a);s=s[:a]+s[b:];s=s.replace('self.spam_scanning()','self.spam_scanning')
a=s.index('    fn test_config(');b=s.index('\n    #[tokio::test]',a);s=s[:a]+'''    fn test_scanning(spam_enabled: bool) -> SpamScanning {
        if spam_enabled { SpamScanning::Available } else { SpamScanning::Unavailable }
    }
'''+s[b:];s=s.replace('test_config(', 'test_scanning(');p.write_text(s)
p=Path('src/application/services/notification_worker.rs');s=p.read_text().replace('    infra::config::AppConfig,','    application::settings::NotificationMailSettings,').replace('config: Arc<AppConfig>','settings: NotificationMailSettings').replace('            config,','            settings,').replace('self.config.public_base_url()','self.settings.public_base_url').replace('self.config.app_domain_name','self.settings.mail_domain');p.write_text(s)

# Rewrite only constructor argument expressions at traced call sites, outside narrow application fixtures.
def transform_calls(s,symbol,transform):
 pos=0
 while True:
  i=s.find(symbol+'(',pos)
  if i<0:break
  start=i+len(symbol)+1;j=start;stack=['('];args=[];a=start;quoted=False;escaped=False
  while stack:
   c=s[j]
   if quoted:
    if escaped:escaped=False
    elif c=='\\':escaped=True
    elif c=='"':quoted=False
   elif c=='"':quoted=True
   elif c in '([{':stack.append(c)
   elif c in ')]}':
    stack.pop()
    if not stack:
     if s[a:j].strip():args.append((a,j))
     break
   elif c==',' and len(stack)==1:
    if s[a:j].strip():args.append((a,j))
    a=j+1
   j+=1
  a,b=args[-1];old=s[a:b];leading=old[:len(old)-len(old.lstrip())];trailing=old[len(old.rstrip()):];new=leading+transform(old.strip())+trailing
  s=s[:a]+new+s[b:];pos=j+len(new)-len(old)+1
 return s
for log,symbol,method in [('thread-constructor-sites.txt','ThreadUseCases::new','thread_settings'),('thread-test-constructor-sites.txt','ThreadUseCases::for_test','thread_settings'),('channel-constructor-sites.txt','ChannelUseCases::new','spam_scanning')]:
 for name in set(re.findall(r'src/[^:\s()]+\.rs',(logs/log).read_text())):
  if name.startswith('src/application/'):continue
  p=Path(name);s=p.read_text();s=transform_calls(s,symbol,lambda expr:'('+expr+').'+method+'()');p.write_text(s)
p=Path('src/infra/setup.rs');s=p.read_text();s=s.replace('        config.clone(),\n        monitoring.clone(),\n    ));','        config.notification_settings(),\n        monitoring.clone(),\n    ));',1);p.write_text(s)

# HTTP ingress receives its own configuration from state; no application accessor survives.
p=Path('src/adapters/http/routes/channel.rs');s=p.read_text().replace('    State(events): State<MailboxEvents>,\n    viewer: Viewer,','    State(events): State<MailboxEvents>,\n    State(config): State<Arc<AppConfig>>,\n    viewer: Viewer,',1).replace('thread_use_cases.config().app_domain_name.clone()','config.app_domain_name.clone()')
s=s.replace('    thread_use_cases: &ThreadUseCases,\n    payload: RawInboundPayload,','    thread_use_cases: &ThreadUseCases,\n    app_domain_name: &str,\n    payload: RawInboundPayload,',1).replace('EmailIngressAdapter::for_config(thread_use_cases.config())','EmailIngressAdapter::new(app_domain_name)').replace('compose_and_ingest(&thread_use_cases, raw_payload, delivery_for(mode))','compose_and_ingest(&thread_use_cases, &config.app_domain_name, raw_payload, delivery_for(mode))');p.write_text(s)
p=Path('src/adapters/http/routes/webhooks/sendgrid.rs');s=p.read_text().replace('    State(thread_use_cases): State<Arc<ThreadUseCases>>,','    State(thread_use_cases): State<Arc<ThreadUseCases>>,\n    State(config): State<Arc<AppConfig>>,',1).replace('    let config = thread_use_cases.config();\n','').replace('configured_sendgrid(config)','configured_sendgrid(&config)').replace('decode_sendgrid_request(request, config)','decode_sendgrid_request(request, &config)').replace('.persist(config, file_storage.as_deref())','.persist(&config, file_storage.as_deref())');p.write_text(s)
print('Narrow settings installed and traced constructors/HTTP consumers updated')
