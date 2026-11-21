use crate::application::identity::{Failure, Result};
use hickory_resolver::{
    Resolver, TokioResolver,
    config::{CLOUDFLARE, ResolverConfig},
    net::runtime::TokioRuntimeProvider,
    proto::rr::{RData, RecordType},
};
use std::{net::IpAddr, time::Duration};

#[derive(Clone)]
pub struct DomainDns {
    resolver: TokioResolver,
}
impl DomainDns {
    pub fn production() -> anyhow::Result<Self> {
        Self::from_config(ResolverConfig::udp_and_tcp(&CLOUDFLARE))
    }
    pub(super) fn from_config(config: ResolverConfig) -> anyhow::Result<Self> {
        let mut builder = Resolver::builder_with_config(config, TokioRuntimeProvider::default());
        let options = builder.options_mut();
        options.timeout = Duration::from_millis(1500);
        options.attempts = 1;
        options.cache_size = 0;
        options.use_hosts_file = hickory_resolver::config::ResolveHosts::Never;
        Ok(Self {
            resolver: builder.build()?,
        })
    }
    /// Only the configured recursive servers receive packets. CNAMEs never become
    /// outbound HTTP requests; fully qualified names disable resolver search paths.
    pub async fn verify(&self, host: &str, challenge: &str) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(2),async {
            let name=format!("{host}.");
            let (txt,v4,v6)=tokio::join!(self.resolver.txt_lookup(format!("_attribution.{host}.")), self.resolver.lookup(name.clone(),RecordType::A), self.resolver.lookup(name,RecordType::AAAA));
            let txt=txt.map_err(|e|if e.is_no_records_found(){Failure::Invalid}else{Failure::Unavailable})?;
            if !txt.answers().iter().any(|record|matches!(&record.data, RData::TXT(value) if value.txt_data.concat()==challenge.as_bytes())) {return Err(Failure::Invalid);}
            let mut addresses=Vec::new();
            for answer in [v4,v6] {
                match answer {
                    Ok(answer)=>for record in answer.answers() {
                        match &record.data { RData::A(ip)=>addresses.push(IpAddr::V4(ip.0)), RData::AAAA(ip)=>addresses.push(IpAddr::V6(ip.0)), _=>{} }
                    },
                    Err(error) if error.is_no_records_found()=>{},
                    Err(_)=>return Err(Failure::Unavailable),
                }
            }
            if addresses.is_empty() || !addresses.into_iter().all(public_address) {return Err(Failure::Invalid);}
            Ok(())
        }).await.map_err(|_|Failure::Unavailable)?
    }
}
/// Conservative public routing policy; deny IANA special-purpose ranges, including
/// IPv4-mapped/translation/tunnel IPv6 forms. No socket is ever opened to these IPs.
pub fn public_address(address: IpAddr) -> bool {
    const V4: &[&str] = &[
        "0.0.0.0/8",
        "10.0.0.0/8",
        "100.64.0.0/10",
        "127.0.0.0/8",
        "169.254.0.0/16",
        "172.16.0.0/12",
        "192.0.0.0/24",
        "192.0.2.0/24",
        "192.88.99.0/24",
        "192.168.0.0/16",
        "198.18.0.0/15",
        "198.51.100.0/24",
        "203.0.113.0/24",
        "224.0.0.0/4",
        "240.0.0.0/4",
    ];
    const V6: &[&str] = &["2001::/23", "2001:db8::/32", "2002::/16", "3fff::/20"];
    match address {
        IpAddr::V4(_) => !V4
            .iter()
            .any(|s| s.parse::<ipnet::IpNet>().unwrap().contains(&address)),
        IpAddr::V6(_) => {
            "2000::/3"
                .parse::<ipnet::IpNet>()
                .unwrap()
                .contains(&address)
                && !V6
                    .iter()
                    .any(|s| s.parse::<ipnet::IpNet>().unwrap().contains(&address))
        }
    }
}
