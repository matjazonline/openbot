//! Local DNS for the real mail-auth SPF/DMARC algorithms. No verdicts are stubbed.
use hickory_resolver::{
    config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts},
    proto::{
        op::{Message, MessageType, ResponseCode},
        rr::{Name, RData, Record, RecordType, rdata::TXT},
    },
};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::UdpSocket, task::JoinSet};
use tokio_util::sync::CancellationToken;

pub(crate) struct LocalEmailDns {
    address: SocketAddr,
    queries: Arc<Mutex<Vec<Name>>>,
    cancel: CancellationToken,
    tasks: JoinSet<()>,
}

impl LocalEmailDns {
    pub(crate) async fn start() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let queries = Arc::new(Mutex::new(Vec::new()));
        let cancel = CancellationToken::new();
        let mut tasks = JoinSet::new();
        let observed = queries.clone();
        let shutdown = cancel.clone();
        tasks.spawn(async move {
            let mut packet = [0_u8; 4096];
            loop {
                let received = tokio::select! {
                    _ = shutdown.cancelled() => return,
                    received = socket.recv_from(&mut packet) => received.unwrap(),
                };
                let request = Message::from_vec(&packet[..received.0]).unwrap();
                observed
                    .lock()
                    .unwrap()
                    .extend(request.queries().iter().map(|query| query.name().clone()));
                let response = answer(&request).to_vec().unwrap();
                socket.send_to(&response, received.1).await.unwrap();
            }
        });
        Self {
            address,
            queries,
            cancel,
            tasks,
        }
    }

    pub(crate) fn authenticator(&self) -> mail_auth::MessageAuthenticator {
        let mut config = ResolverConfig::new();
        config.add_name_server(NameServerConfig::new(self.address, Protocol::Udp));
        let mut options = ResolverOpts::default();
        options.attempts = 1;
        options.timeout = Duration::from_secs(1);
        mail_auth::MessageAuthenticator::new(config, options).unwrap()
    }

    pub(crate) fn queried(&self, name: &str) -> bool {
        self.queries
            .lock()
            .unwrap()
            .contains(&Name::from_ascii(name).unwrap())
    }

    pub(crate) async fn stop(mut self) {
        self.cancel.cancel();
        tokio::time::timeout(Duration::from_secs(2), async {
            while let Some(result) = self.tasks.join_next().await {
                result.unwrap();
            }
        })
        .await
        .expect("local DNS peer shuts down");
    }
}

fn answer(request: &Message) -> Message {
    let mut response = Message::new();
    response
        .set_id(request.id())
        .set_message_type(MessageType::Response)
        .set_op_code(request.op_code())
        .set_authoritative(true)
        .set_recursion_desired(request.recursion_desired())
        .set_recursion_available(true)
        .add_queries(request.queries().iter().cloned());
    for query in request.queries() {
        let value = match (query.name().to_ascii().as_str(), query.query_type()) {
            ("partner.test." | "example.test.", RecordType::TXT) => {
                Some("v=spf1 ip4:127.0.0.1 -all")
            }
            ("blocked.test.", RecordType::TXT) => Some("v=spf1 -all"),
            (
                "_dmarc.partner.test." | "_dmarc.blocked.test." | "_dmarc.example.test.",
                RecordType::TXT,
            ) => Some("v=DMARC1; p=reject; aspf=s; adkim=s"),
            _ => None,
        };
        match value {
            Some(value) => {
                response.add_answer(Record::from_rdata(
                    query.name().clone(),
                    60,
                    RData::TXT(TXT::new(vec![value.into()])),
                ));
            }
            None => {
                response.set_response_code(ResponseCode::NXDomain);
            }
        }
    }
    response
}
