use axum::{Form,extract::FromRequest,http::Request,body::Body,response::IntoResponse};
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionForm { expected_revision:i64 }
fn main(){ tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
let req=Request::builder().method("DELETE").uri("/ui/profile/vault/11111111-1111-1111-1111-111111111111?expected_revision=1").header("content-type","application/x-www-form-urlencoded").body(Body::empty()).unwrap();
match Form::<RevisionForm>::from_request(req,&()).await { Ok(Form(v))=>panic!("unexpected success {}",v.expected_revision),Err(e)=>{let status=e.into_response().status();println!("Vault RevisionForm extraction: {}",status);assert_eq!(status.as_u16(),422);}}
});}
