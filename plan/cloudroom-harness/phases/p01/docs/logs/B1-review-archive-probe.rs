pub use open_bots::{app_error, entities, use_cases};
#[path = "/private/tmp/openbot-cloudroom-harness-t1gfnhz1/worktree/src/adapters/http/routes/skill_library/archive.rs"]
mod archive;
fn main() {
 let mut skill=entities::skill::Skill {
  id: Default::default(), company_id:None,
  slug: entities::value_objects::SkillSlug::parse("probe").unwrap(),
  name:"Probe".into(), description:"Probe files".into(), instructions:"Read the supporting reference.".into(),
  files:vec![],created_by:entities::creation::CreationProvenance::system(),created_at:Default::default(),updated_at:Default::default()
 };
 for path in ["reference.md", "reference-SKILL.md"] {
  skill.files=vec![entities::skill_bundle::SkillFile::from_bytes(path.into(), &vec![b'x';32768],false).unwrap()];
  assert!(skill.validate().is_ok());
  let bytes=archive::encode(&skill).unwrap();
  println!("valid domain file={path}, bytes=32768, exported ZIP bytes={}, reimport={:?}",bytes.len(),archive::decode(&bytes).map(|v| v.files.unwrap().len()));
 }
}
