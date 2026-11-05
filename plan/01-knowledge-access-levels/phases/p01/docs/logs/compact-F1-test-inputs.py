from pathlib import Path
import re
p=Path('src/adapters/http/routes/company_invite_level_test_support.rs');s=p.read_text();marker='impl RouteFixture {\n';assert marker in s;s=s.replace(marker,marker+'''    pub fn local<'a>(&self, actor: &'a User, target_id: Uuid, level: AccessLevel) -> Request<'a> {
        Request { actor, company_id: self.base.company.id, target_id, level }
    }
''',1);p.write_text(s)
p=Path('src/adapters/http/routes/company_invite_level_route_tests.rs');s=p.read_text();s,count=re.subn(r'Request\s*\{\s*actor:\s*([^,]+),\s*company_id:\s*base\.company\.id,\s*target_id:\s*([^,]+),\s*level:\s*([^,}]+),?\s*\}',lambda m:'f.local('+m[1].strip()+', '+m[2].strip()+', '+m[3].strip()+')',s);assert count==15,count;p.write_text(s)
