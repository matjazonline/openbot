use super::{
    admission::Admission,
    dns::{DomainDns, public_address},
};
use crate::application::identity::{CompanyId, Failure};
use hickory_resolver::{
    config::{NameServerConfig, ResolverConfig},
    proto::{
        op::{Message, OpCode},
        rr::{
            RData, Record, RecordType,
            rdata::{A, AAAA, TXT},
        },
    },
};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{
        Arc, RwLock,
        atomic::{AtomicU8, Ordering},
    },
};
use tokio::net::UdpSocket;
use uuid::Uuid;

pub(crate) struct DnsFixture {
    pub dns: DomainDns,
    pub records: Arc<RwLock<HashMap<String, String>>>,
    pub mode: Arc<AtomicU8>,
    pub seen: Arc<tokio::sync::Notify>,
    pub release: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl DnsFixture {
    pub async fn new() -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let records = Arc::new(RwLock::new(HashMap::<String, String>::new()));
        let mode = Arc::new(AtomicU8::new(0));
        let seen = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let arrived = seen.clone();
        let resume = release.clone();
        let data = records.clone();
        let state = mode.clone();
        let task = tokio::spawn(async move {
            let mut bytes = [0; 4096];
            loop {
                let (length, peer) = socket.recv_from(&mut bytes).await.unwrap();
                let query = Message::from_vec(&bytes[..length]).unwrap();
                let q = &query.queries[0];
                let current = state.load(Ordering::Acquire);
                if current == 4 {
                    arrived.notify_one();
                    resume.notified().await;
                }
                if current == 2 || (current == 3 && q.query_type() == RecordType::AAAA) {
                    continue;
                }
                let mut response = Message::response(query.metadata.id, OpCode::Query);
                response.add_query(q.clone());
                response.metadata.recursion_available = true;
                let host = q.name().to_ascii().to_lowercase();
                match q.query_type() {
                    RecordType::TXT => {
                        if let Some(value) = data.read().unwrap().get(&host) {
                            response.add_answer(Record::from_rdata(
                                q.name().clone(),
                                0,
                                RData::TXT(TXT::new(vec![value.clone()])),
                            ));
                        }
                    }
                    RecordType::A => {
                        response.add_answer(Record::from_rdata(
                            q.name().clone(),
                            0,
                            RData::A(A("93.184.216.34".parse().unwrap())),
                        ));
                    }
                    RecordType::AAAA => {
                        response.add_answer(Record::from_rdata(
                            q.name().clone(),
                            0,
                            RData::AAAA(AAAA(
                                if current == 1 {
                                    "::1"
                                } else {
                                    "2606:4700::1111"
                                }
                                .parse()
                                .unwrap(),
                            )),
                        ));
                    }
                    _ => {}
                }
                socket
                    .send_to(&response.to_vec().unwrap(), peer)
                    .await
                    .unwrap();
            }
        });
        let mut server = NameServerConfig::udp(address.ip());
        server.connections[0].port = address.port();
        let dns = DomainDns::from_config(ResolverConfig::from_name_servers(vec![server])).unwrap();
        Self {
            dns,
            records,
            mode,
            seen,
            release,
            task,
        }
    }
    pub fn proof(&self, host: &str, challenge: &str) {
        self.records
            .write()
            .unwrap()
            .insert(format!("_attribution.{host}."), challenge.into());
    }
}
impl Drop for DnsFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn attribution_destination_dns_transport_all_addresses_and_timeout() {
    let fixture = DnsFixture::new().await;
    fixture.proof("shop.merchant.com", "proof");
    fixture
        .dns
        .verify("shop.merchant.com", "proof")
        .await
        .unwrap();
    assert!(matches!(
        fixture.dns.verify("shop.merchant.com", "wrong").await,
        Err(Failure::Invalid)
    ));
    fixture.mode.store(1, Ordering::Release);
    assert!(matches!(
        fixture.dns.verify("shop.merchant.com", "proof").await,
        Err(Failure::Invalid)
    ));
    for mode in [2, 3] {
        fixture.mode.store(mode, Ordering::Release);
        let start = tokio::time::Instant::now();
        assert!(matches!(
            fixture.dns.verify("shop.merchant.com", "proof").await,
            Err(Failure::Unavailable)
        ));
        assert!(start.elapsed() < std::time::Duration::from_secs(3));
    }
}
#[test]
fn attribution_destination_special_ranges_and_public_addresses() {
    for address in [
        "127.0.0.1",
        "10.1.2.3",
        "100.64.0.1",
        "169.254.169.254",
        "192.168.1.1",
        "192.0.2.1",
        "198.18.0.1",
        "224.1.1.1",
        "255.255.255.255",
        "::1",
        "::ffff:93.184.216.34",
        "64:ff9b::7f00:1",
        "2001:db8::1",
        "2002::1",
        "fc00::1",
        "fe80::1",
        "3fff::1",
    ] {
        assert!(
            !public_address(address.parse::<IpAddr>().unwrap()),
            "{address}"
        );
    }
    for address in ["93.184.216.34", "1.1.1.1", "2606:4700::1111"] {
        assert!(public_address(address.parse().unwrap()));
    }
}
#[test]
fn attribution_credentials_tenant_admission_is_bounded_and_releases() {
    let admission = Admission::default();
    let company = CompanyId(Uuid::new_v4());
    let permits: Vec<_> = (0..8).map(|_| admission.enter(company).unwrap()).collect();
    assert!(matches!(
        admission.enter(company),
        Err(Failure::RateLimited)
    ));
    let other = admission.enter(CompanyId(Uuid::new_v4())).unwrap();
    drop(permits);
    admission.enter(company).unwrap();
    drop(other);
}
