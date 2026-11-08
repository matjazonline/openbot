use open_bots::application::{integrations::OperationArguments, workflow::{http::HttpRequest, actions::{action_step_failure,ActionFamily,ActionStepOutcome}}};
fn main() {
 let request=serde_json::json!({"method":"POST","path":"/send","headers":{},"body":{"text":"x".repeat(17000)}});
 assert!(HttpRequest::parse(&request).is_ok());
 let error=match OperationArguments::new(request.clone()){Err(e)=>e,Ok(_)=>panic!("bound unexpectedly accepted")};
 let error:open_bots::app_error::AppError=error.into();
 let failure=action_step_failure(ActionFamily::Http,ActionStepOutcome::Dispatch(&error));
 println!("native request accepted; gateway projection rejected {} bytes; current propagated dispatch classification: {}",serde_json::to_vec(&request).unwrap().len(),failure.failure.code().as_str());
 assert_eq!(failure.failure.code().as_str(),"http.unresolved");
}
