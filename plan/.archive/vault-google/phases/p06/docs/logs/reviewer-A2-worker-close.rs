use rmcp::transport::{Transport,streamable_http_client::{StreamableHttpClientTransport,StreamableHttpClientTransportConfig}};
fn main(){tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
 let mut transport=StreamableHttpClientTransport::with_client(reqwest::Client::new(),StreamableHttpClientTransportConfig::with_uri("http://127.0.0.1:9/never-contacted"));
 tokio::task::yield_now().await;
 transport.cancel_token().cancel();
 let result=tokio::time::timeout(std::time::Duration::from_millis(100),transport.close()).await;
 assert!(result.is_err(),"changed dependency: cancelled handshake close now completes");
 println!("rmcp 1.8.0 WorkerTransport.close remains pending after cancel while waiting for the first handshake message; no HTTP request was submitted");
 // Closing our sender/receiver lets this probe's otherwise waiting worker finish.
 drop(transport);tokio::task::yield_now().await;
});}
