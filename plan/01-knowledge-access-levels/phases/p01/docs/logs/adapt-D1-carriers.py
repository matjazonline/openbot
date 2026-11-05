from pathlib import Path
import json,re,collections
p=Path('plan/01-knowledge-access-levels/phases/p01/docs/logs');groups=collections.defaultdict(list);seen=set()
for line in (p/'D1-live-check.jsonl').read_text().splitlines():
 try:v=json.loads(line)
 except ValueError:continue
 if v.get('reason')!='compiler-message':continue
 d=v['message'];code=(d.get('code') or {}).get('code')
 if code not in ['E0063','E0027']:continue
 span=next((s for s in d['spans'] if s.get('is_primary')),None)
 if not span:continue
 key=(span['file_name'],span['byte_start'],code)
 if key in seen:continue
 seen.add(key)
 if span['file_name'].endswith('/ui_dashboard.rs'):continue
 groups[span['file_name']].append(dict(span=span,code=code,message=d['message']))
report=[]
for name,items in groups.items():
 f=Path(name);data=f.read_bytes()
 for item in sorted(items,key=lambda x:x['span']['byte_start'],reverse=True):
  sp=item['span'];begin=sp['byte_start'];brace=data.index(b'{',begin)
  if item['code']=='E0027':
   end=sp['byte_end'];chunk=data[begin:end];assert chunk.rstrip().endswith(b'}');i=begin+len(chunk.rstrip())-1;data=data[:i]+b'..'+data[i:]
  else:
   text=data[brace:brace+650].decode();m=re.search(r'\n\s*membership(?::\s*([^\n]+))?,',text)
   if 'CompanyMember' in item['message'] or 'CompanyInvite' in item['message']:value='crate::entities::access_level::AccessLevel::PUBLIC'
   else:
    assert m,(name,item)
    expr=(m[1] or 'membership').strip().removesuffix(',')
    if '.await?' in expr:expr='membership'
    value='crate::entities::access_level::fixture_level('+expr+')'
   indent=' '*(sp['column_start']-1+4);addition=('\n'+indent+'access_level: '+value+',').encode();data=data[:brace+1]+addition+data[brace+1:]
  report.append(dict(file=name,line=sp['line_start'],code=item['code']))
 f.write_bytes(data)
(p/'D1-carrier-adaptations.json').write_text(json.dumps(report,indent=2)+'\n')
f=Path('src/domain/entities/access_level.rs');f.write_text(f.read_text()+'''
/// Existing tests create fresh actors at the ordinary joining level, with the fixed owner rule.
#[cfg(test)]
pub(crate) fn fixture_level(membership: CompanyMembership) -> AccessLevel {
    if membership.is_owner() { AccessLevel::MAX } else { AccessLevel::PUBLIC }
}
''')
f=Path('src/application/use_cases/participant/test_support.rs');s=f.read_text();old='''        Ok(PrincipalAccessContext {
            access_level: crate::entities::access_level::fixture_level(membership),''';assert old in s;s=s.replace(old,'''        let membership = self.membership_for_subject(company_id, identity).await?;
        Ok(PrincipalAccessContext {
            access_level: crate::entities::access_level::fixture_level(membership),''',1).replace('            membership: self.membership_for_subject(company_id, identity).await?,','            membership,');f.write_text(s)
f=Path('src/adapters/http/routes/ui_dashboard.rs');s=f.read_text();old='''                        let membership = managed
                            .iter()
                            .find(|access| access.company.id == company.id)
                            .map_or(CompanyMembership::None, |access| access.membership);
                        CompanyAccess {
                            company,
                            membership,
                        }''';new='''                        let managed_access = managed.iter().find(|access| access.company.id == company.id);
                        CompanyAccess {
                            membership: managed_access.map_or(CompanyMembership::None, |access| access.membership),
                            access_level: managed_access.map_or(crate::entities::access_level::AccessLevel::PUBLIC,
                                |access| access.access_level),
                            company,
                        }''';assert old in s;s=s.replace(old,new);f.write_text(s)
print('Compiler-enumerated carrier adaptations',len(report),'+ operator projection preserving joint facts')
