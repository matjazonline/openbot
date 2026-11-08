use open_bots::{adapters::mcp::{HttpMcpClient, policy::EndpointPolicy}, application::services::mcp_client::McpClient};
use std::{sync::{Arc, Mutex, mpsc}, io::{Read, Write}, net::TcpListener, time::{Duration, Instant}};
fn request(s: &mut std::net::TcpStream) -> (String,serde_json::Value) {
 s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
 let mut bytes=Vec::new(); let mut one=[0u8;1];
 while !bytes.ends_with(b"\r\n\r\n") { if s.read(&mut one).unwrap_or(0)==0 {break} bytes.push(one[0]); }
 let h=String::from_utf8_lossy(&bytes).to_string();
 let n=h.lines().find_map(|l|l.to_lowercase().strip_prefix("content-length:").and_then(|x|x.trim().parse::<usize>().ok())).unwrap_or(0);
 let mut b=vec![0;n]; let _=s.read_exact(&mut b);
 (h,serde_json::from_slice(&b).unwrap_or(serde_json::Value::Null))
}
fn reply(s:&mut std::net::TcpStream, body:&str) {let _=write!(s,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nMcp-Session-Id: reviewer-session\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body);}
fn main() {
 let listener=TcpListener::bind("127.0.0.1:0").unwrap(); let address=listener.local_addr().unwrap();
 let (arrived_tx,arrived_rx)=tokio::sync::oneshot::channel(); let (release_tx,release_rx)=mpsc::channel();
 let events=Arc::new(Mutex::new(Vec::new())); let out=events.clone();
 let server=std::thread::spawn(move||{
  let (mut s,_)=listener.accept().unwrap(); let (_,body)=request(&mut s); assert_eq!(body["method"],"initialize");
  arrived_tx.send(()).unwrap(); release_rx.recv().unwrap();
  reply(&mut s,&serde_json::json!({"jsonrpc":"2.0","id":body["id"],"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"reviewer","version":"1"}}}).to_string()); drop(s);
  listener.set_nonblocking(true).unwrap(); let end=Instant::now()+Duration::from_secs(2);
  while Instant::now()<end { match listener.accept() {Ok((mut s,_))=>{let(h,b)=request(&mut s); out.lock().unwrap().push(format!("{} {}",h.lines().next().unwrap_or(""),b.get("method").and_then(|v|v.as_str()).unwrap_or("")));let _=s.write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");}, Err(_)=>std::thread::sleep(Duration::from_millis(5))} }
 });
 tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap().block_on(async {
  let endpoint=format!("http://{address}/mcp"); let client=Arc::new(HttpMcpClient::new(EndpointPolicy::new([endpoint.clone()]).unwrap()));
  let c=client.clone(); let task=tokio::spawn(async move {c.connect(&endpoint.try_into().unwrap(),Some(secrecy::SecretString::from("reviewer-only-sentinel"))).await});
  arrived_rx.await.unwrap(); task.abort(); assert!(matches!(task.await,Err(e) if e.is_cancelled()));
  println!("connect future dropped before server released initialize response"); release_tx.send(()).unwrap();
  tokio::time::sleep(Duration::from_millis(800)).await;
  let observed=events.lock().unwrap().clone(); println!("requests AFTER cancellation: {observed:?}");
  assert!(observed.iter().any(|s|s.contains("notifications/initialized")),"expected reproduction: actual initialization continues after cancellation");
  client.shutdown().await.unwrap();
 }); server.join().unwrap();
}
