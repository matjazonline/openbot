use mail_parser::MimeHeaders;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, warn};

use crate::{
    adapters::protocols::email::EmailIngressAdapter,
    application::use_cases::channel::parse_recipient_address,
    application::use_cases::thread::ThreadUseCases,
    domain::monitoring::{MonitoringService, SmtpConnectionMetrics, SmtpStatus},
    infra::config::AppConfig,
    services::email_parser::{RawAttachmentData, RawInboundPayload, extract_email},
};

static MAIL_FROM_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)from:\s*<([^>]+)>|from:\s*([^\s]+)").unwrap()
});
static RCPT_TO_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)to:\s*<([^>]+)>|to:\s*([^\s]+)").unwrap());

#[derive(Debug, PartialEq, Eq)]
enum SmtpState {
    Command,
    Data,
}

pub struct ConnGuard {
    ip: IpAddr,
    conns: Arc<RwLock<HashMap<IpAddr, usize>>>,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        if let Ok(mut lock) = self.conns.write() {
            if let Some(count) = lock.get_mut(&self.ip) {
                if *count > 1 {
                    *count -= 1;
                } else {
                    lock.remove(&self.ip);
                }
            }
        }
    }
}

pub async fn check_dnsbl(ip: IpAddr, dnsbl_servers: &[String]) -> Option<String> {
    if dnsbl_servers.is_empty() || ip.is_loopback() {
        return None;
    }

    let ip_reversed = match ip {
        IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            format!("{}.{}.{}.{}", octets[3], octets[2], octets[1], octets[0])
        }
        IpAddr::V6(_) => return None,
    };

    for server in dnsbl_servers {
        let lookup_host = format!("{}.{}", ip_reversed, server);
        if let Ok(mut addrs) = tokio::net::lookup_host((lookup_host.as_str(), 80)).await {
            if addrs.next().is_some() {
                return Some(server.clone());
            }
        }
    }

    None
}

pub struct SmtpServer {
    thread_use_cases: Arc<ThreadUseCases>,
    config: Arc<AppConfig>,
    monitoring: Option<Arc<dyn MonitoringService>>,
    active_conns: Arc<RwLock<HashMap<IpAddr, usize>>>,
}

impl SmtpServer {
    pub fn new(thread_use_cases: Arc<ThreadUseCases>, config: Arc<AppConfig>) -> Self {
        Self {
            thread_use_cases,
            config,
            monitoring: None,
            active_conns: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    pub async fn start_server_loop(
        self: Arc<Self>,
        mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
    ) {
        if !self.config.incoming_smtp_enabled {
            info!("Incoming SMTP server is disabled via configuration.");
            return;
        }

        let addr = format!(
            "{}:{}",
            self.config.incoming_smtp_host, self.config.incoming_smtp_port
        );

        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(err) => {
                error!("Failed to bind incoming SMTP server on {addr}: {err}");
                return;
            }
        };

        info!("Incoming SMTP server listening on {addr}");

        loop {
            tokio::select! {
                res = listener.accept() => {
                    match res {
                        Ok((stream, peer_addr)) => {
                            let server = self.clone();
                            tokio::spawn(async move {
                                if let Err(err) = server.handle_connection(stream, peer_addr).await {
                                    warn!("Error handling SMTP connection from {peer_addr}: {err}");
                                }
                            });
                        }
                        Err(err) => {
                            warn!("Error accepting SMTP connection: {err}");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutting down incoming SMTP server listener...");
                    break;
                }
            }
        }
    }

    async fn handle_connection(
        &self,
        mut stream: TcpStream,
        peer_addr: std::net::SocketAddr,
    ) -> anyhow::Result<()> {
        let start_time = Instant::now();
        let client_ip = peer_addr.ip();

        // 1. Connection Rate Limiting
        let rate_limited = {
            let mut lock = self.active_conns.write().unwrap_or_else(|e| e.into_inner());
            let current = lock.entry(client_ip).or_insert(0);
            if *current >= self.config.smtp_rate_limit_conns_per_ip {
                true
            } else {
                *current += 1;
                false
            }
        };

        if rate_limited {
            warn!("Connection from {} rejected: Rate limit reached", client_ip);
            if let Some(ref m) = self.monitoring {
                m.record_smtp_connection(&SmtpConnectionMetrics {
                    client_ip,
                    status: SmtpStatus::BlockedRateLimit,
                    duration_ms: start_time.elapsed().as_millis() as u64,
                    mail_from: None,
                    rcpt_to: None,
                });
            }
            stream
                .write_all(b"421 4.7.0 Too many active connections from your IP address\r\n")
                .await?;
            stream.flush().await?;
            return Ok(());
        }

        let _guard = ConnGuard {
            ip: client_ip,
            conns: self.active_conns.clone(),
        };

        // 2. DNSBL (RBL) Filtering
        if self.config.dnsbl_enabled {
            if let Some(rbl) = check_dnsbl(client_ip, &self.config.dnsbl_servers).await {
                warn!("Connection from {} blocked by DNSBL {}", client_ip, rbl);
                if let Some(ref m) = self.monitoring {
                    m.record_smtp_connection(&SmtpConnectionMetrics {
                        client_ip,
                        status: SmtpStatus::BlockedDnsbl,
                        duration_ms: start_time.elapsed().as_millis() as u64,
                        mail_from: None,
                        rcpt_to: None,
                    });
                }
                let msg = format!(
                    "554 5.7.1 Service unavailable; Client host [{}] blocked using {}\r\n",
                    client_ip, rbl
                );
                stream.write_all(msg.as_bytes()).await?;
                stream.flush().await?;
                return Ok(());
            }
        }

        let (reader, mut writer) = stream.split();
        let mut reader = BufReader::new(reader);

        // Initial 220 banner
        let banner = format!(
            "220 {} ESMTP Service Ready\r\n",
            self.config.app_domain_name
        );
        writer.write_all(banner.as_bytes()).await?;
        writer.flush().await?;

        let mut state = SmtpState::Command;
        let mut ehlo_domain: Option<String> = None;
        let mut mailfrom: Option<String> = None;
        let mut rcpts: Vec<String> = Vec::new();
        let mut data_buffer = String::new();
        let mut line_buf = String::new();

        loop {
            line_buf.clear();
            let bytes_read = reader.read_line(&mut line_buf).await?;
            if bytes_read == 0 {
                break; // Connection closed
            }

            let trimmed = line_buf.trim_end_matches(['\r', '\n']);

            match state {
                SmtpState::Command => {
                    let space_pos = trimmed.find(' ').unwrap_or(trimmed.len());
                    let (cmd_raw, arg_raw) = trimmed.split_at(space_pos);
                    let cmd = cmd_raw.trim().to_uppercase();
                    let arg = arg_raw.trim();

                    match cmd.as_str() {
                        "HELO" | "EHLO" => {
                            if !arg.is_empty() {
                                let is_self_domain =
                                    arg.eq_ignore_ascii_case(&self.config.app_domain_name);
                                if is_self_domain
                                    && self.config.reject_self_domain_helo
                                    && !client_ip.is_loopback()
                                {
                                    warn!(
                                        "Client {} spoofed local domain {} in EHLO/HELO",
                                        client_ip, arg
                                    );
                                    if let Some(ref m) = self.monitoring {
                                        m.record_smtp_connection(&SmtpConnectionMetrics {
                                            client_ip,
                                            status: SmtpStatus::RejectedHelo,
                                            duration_ms: start_time.elapsed().as_millis() as u64,
                                            mail_from: None,
                                            rcpt_to: None,
                                        });
                                    }
                                    writer.write_all(b"550 5.5.2 Helo command rejected: Host name spoofed\r\n").await?;
                                    writer.flush().await?;
                                    return Ok(());
                                }
                                ehlo_domain = Some(arg.to_string());
                            }
                            let resp = format!(
                                "250-{} Hello {}\r\n250-SIZE 20971520\r\n250 OK\r\n",
                                self.config.app_domain_name,
                                peer_addr.ip()
                            );
                            writer.write_all(resp.as_bytes()).await?;
                        }
                        "MAIL" => {
                            if let Some(cap) = MAIL_FROM_RE.captures(arg) {
                                let raw_addr = cap
                                    .get(1)
                                    .or_else(|| cap.get(2))
                                    .map(|m| m.as_str())
                                    .unwrap_or_default();
                                let extracted = extract_email(raw_addr);
                                mailfrom = Some(extracted);
                                writer.write_all(b"250 2.1.0 Ok\r\n").await?;
                            } else {
                                writer
                                    .write_all(b"501 Syntax: MAIL FROM:<address>\r\n")
                                    .await?;
                            }
                        }
                        "RCPT" => {
                            if mailfrom.is_none() {
                                writer
                                    .write_all(b"503 Error: Send MAIL FROM first\r\n")
                                    .await?;
                            } else if let Some(cap) = RCPT_TO_RE.captures(arg) {
                                let raw_addr = cap
                                    .get(1)
                                    .or_else(|| cap.get(2))
                                    .map(|m| m.as_str())
                                    .unwrap_or_default();
                                let extracted = extract_email(raw_addr);
                                rcpts.push(extracted);
                                writer.write_all(b"250 2.1.5 Ok\r\n").await?;
                            } else {
                                writer
                                    .write_all(b"501 Syntax: RCPT TO:<address>\r\n")
                                    .await?;
                            }
                        }
                        "DATA" => {
                            if rcpts.is_empty() {
                                writer.write_all(b"503 Error: MAIL FROM and RCPT TO must be set before DATA\r\n").await?;
                            } else {
                                state = SmtpState::Data;
                                writer
                                    .write_all(
                                        b"354 Start mail input; end with <CR><LF>.<CR><LF>\r\n",
                                    )
                                    .await?;
                            }
                        }
                        "RSET" => {
                            mailfrom = None;
                            rcpts.clear();
                            data_buffer.clear();
                            writer.write_all(b"250 2.0.0 Ok\r\n").await?;
                        }
                        "NOOP" => {
                            writer.write_all(b"250 2.0.0 Ok\r\n").await?;
                        }
                        "QUIT" => {
                            writer.write_all(b"221 2.0.0 Bye\r\n").await?;
                            writer.flush().await?;
                            break;
                        }
                        _ => {
                            writer
                                .write_all(b"500 5.5.1 Command unrecognized\r\n")
                                .await?;
                        }
                    }
                    writer.flush().await?;
                }
                SmtpState::Data => {
                    if trimmed == "." {
                        let mut spf_status: Option<String> = None;
                        let mut dkim_status: Option<String> = None;
                        let mut dmarc_status: Option<String> = None;
                        let mut spf_output_obj: Option<mail_auth::SpfOutput> = None;

                        // Perform active DNS SPF, DKIM & DMARC verification via mail-auth
                        if let Ok(resolver) = mail_auth::MessageAuthenticator::new_quad9() {
                            if let Some(ref sender) = mailfrom {
                                let domain = sender.split('@').nth(1).unwrap_or(sender);
                                let ehlo = ehlo_domain.as_deref().unwrap_or(domain);
                                let params =
                                    mail_auth::spf::verify::SpfParameters::verify_mail_from(
                                        peer_addr.ip(),
                                        ehlo,
                                        domain,
                                        sender,
                                    );
                                let spf_res = resolver.verify_spf(params).await;
                                spf_status = Some(match spf_res.result() {
                                    mail_auth::SpfResult::Pass => "pass".to_string(),
                                    mail_auth::SpfResult::Fail => "fail".to_string(),
                                    mail_auth::SpfResult::SoftFail => "softfail".to_string(),
                                    mail_auth::SpfResult::Neutral => "neutral".to_string(),
                                    _ => "none".to_string(),
                                });
                                spf_output_obj = Some(spf_res);
                            }

                            if let Some(auth_msg) =
                                mail_auth::AuthenticatedMessage::parse(data_buffer.as_bytes())
                            {
                                let dkim_outputs_obj = resolver.verify_dkim(&auth_msg).await;
                                if !dkim_outputs_obj.is_empty() {
                                    if dkim_outputs_obj
                                        .iter()
                                        .all(|s| matches!(s.result(), mail_auth::DkimResult::Pass))
                                    {
                                        dkim_status = Some("pass".to_string());
                                    } else if dkim_outputs_obj.iter().any(|s| {
                                        matches!(s.result(), mail_auth::DkimResult::Fail(_))
                                    }) {
                                        dkim_status = Some("fail".to_string());
                                    } else {
                                        dkim_status = Some("none".to_string());
                                    }
                                }

                                if let Some(ref spf_output) = spf_output_obj {
                                    if let Some(ref sender) = mailfrom {
                                        let domain = sender.split('@').nth(1).unwrap_or(sender);
                                        let dmarc_params =
                                            mail_auth::dmarc::verify::DmarcParameters::new(
                                                &auth_msg,
                                                &dkim_outputs_obj,
                                                domain,
                                                spf_output,
                                            );
                                        let dmarc_output =
                                            resolver.verify_dmarc(dmarc_params).await;
                                        let dmarc_dkim_pass = matches!(
                                            dmarc_output.dkim_result(),
                                            mail_auth::DmarcResult::Pass
                                        );
                                        let dmarc_spf_pass = matches!(
                                            dmarc_output.spf_result(),
                                            mail_auth::DmarcResult::Pass
                                        );
                                        dmarc_status = Some(if dmarc_dkim_pass || dmarc_spf_pass {
                                            "pass".to_string()
                                        } else {
                                            "fail".to_string()
                                        });
                                    }
                                }
                            }
                        }

                        let mut raw_payload = parse_raw_mime_to_payload(
                            data_buffer.as_bytes(),
                            mailfrom.as_deref(),
                            rcpts.first().map(|s| s.as_str()),
                            &rcpts,
                            spf_status,
                            dkim_status,
                            dmarc_status,
                        );

                        // Stage 1 & Stage 2 Spam Scanner (Run for external senders on public channels; skip for trusted participants)
                        let mut should_scan_spam = true;
                        let recipient_str = raw_payload.to.clone();
                        if let Some((company_slug, channel_slug)) =
                            parse_recipient_address(&recipient_str, &self.config.app_domain_name)
                        {
                            if let Ok(Some(company)) = self
                                .thread_use_cases
                                .company_persistence()
                                .get_by_slug(&company_slug)
                                .await
                            {
                                if let Ok(Some(channel)) = self
                                    .thread_use_cases
                                    .channel_persistence()
                                    .get_by_company_slug_and_channel_slug(
                                        &company_slug,
                                        &channel_slug,
                                    )
                                    .await
                                {
                                    let sender_clean = raw_payload.from.trim();
                                    let is_team_member = self
                                        .thread_use_cases
                                        .company_persistence()
                                        .is_company_team_member(company.id, sender_clean)
                                        .await
                                        .unwrap_or(false);

                                    let is_trusted = match &channel.participant_emails {
                                        Some(allowed) if !allowed.is_empty() => {
                                            let explicitly_listed = allowed.iter().any(|e| {
                                                !e.trim().eq_ignore_ascii_case("@public")
                                                    && e.eq_ignore_ascii_case(sender_clean)
                                            });
                                            explicitly_listed || is_team_member
                                        }
                                        _ => is_team_member,
                                    };

                                    if is_trusted {
                                        should_scan_spam = false;
                                    }
                                }
                            }
                        }

                        if should_scan_spam {
                            let spam_scanner =
                                crate::services::spam_scanner::SpamScannerService::new(
                                    self.config.clone(),
                                );
                            let scan_res = spam_scanner
                                .scan(
                                    data_buffer.as_bytes(),
                                    raw_payload.subject.as_deref(),
                                    Some(&raw_payload.from),
                                    raw_payload.text.as_deref(),
                                    raw_payload.html.as_deref(),
                                )
                                .await;

                            if scan_res.score > 0.0 {
                                raw_payload.spam_score = Some(scan_res.score);
                                if let Some(ref m) = self.monitoring {
                                    m.record_histogram("smtp_spam_score", scan_res.score, &[]);
                                }
                            }
                        }

                        let norm_payload = EmailIngressAdapter::parse(raw_payload, &self.config);
                        match self
                            .thread_use_cases
                            .ingest_normalized_message(norm_payload)
                            .await
                        {
                            Ok(ingest) => {
                                if let Some(ref m) = self.monitoring {
                                    let status = if ingest.accepted {
                                        SmtpStatus::Accepted
                                    } else {
                                        match ingest.reason.as_deref() {
                                            Some("SPF authentication failed") => {
                                                SmtpStatus::RejectedSpf
                                            }
                                            Some("DKIM authentication failed") => {
                                                SmtpStatus::RejectedDkim
                                            }
                                            Some("DMARC authentication failed") => {
                                                SmtpStatus::RejectedDmarc
                                            }
                                            Some("Spam score threshold exceeded") => {
                                                SmtpStatus::RejectedSpamScore
                                            }
                                            Some(r) if r.contains("rate limit") => {
                                                SmtpStatus::BlockedRateLimit
                                            }
                                            Some(r) if r.contains("DNSBL") => {
                                                SmtpStatus::BlockedDnsbl
                                            }
                                            _ => SmtpStatus::Error,
                                        }
                                    };
                                    m.record_smtp_connection(&SmtpConnectionMetrics {
                                        client_ip,
                                        status,
                                        duration_ms: start_time.elapsed().as_millis() as u64,
                                        mail_from: mailfrom.clone(),
                                        rcpt_to: rcpts.first().cloned(),
                                    });
                                }

                                if ingest.accepted {
                                    let thread_use_cases_bg = self.thread_use_cases.clone();
                                    tokio::spawn(async move {
                                        if let Err(err) = thread_use_cases_bg
                                            .execute_agent_and_dispatch(&ingest, true)
                                            .await
                                        {
                                            warn!("SMTP background agent execution failed: {err}");
                                        }
                                    });
                                    writer
                                        .write_all(b"250 2.0.0 Message queued for delivery\r\n")
                                        .await?;
                                } else {
                                    let thread_use_cases_bg = self.thread_use_cases.clone();
                                    let ingest_bg = ingest.clone();
                                    tokio::spawn(async move {
                                        thread_use_cases_bg
                                            .handle_bounce_dispatch(&ingest_bg)
                                            .await;
                                    });
                                    let msg = format!(
                                        "250 2.0.0 Message processed ({})\r\n",
                                        ingest.reason.as_deref().unwrap_or("ok")
                                    );
                                    writer.write_all(msg.as_bytes()).await?;
                                }
                            }
                            Err(err) => {
                                warn!("Error ingesting SMTP email: {err}");
                                if let Some(ref m) = self.monitoring {
                                    m.record_smtp_connection(&SmtpConnectionMetrics {
                                        client_ip,
                                        status: SmtpStatus::Error,
                                        duration_ms: start_time.elapsed().as_millis() as u64,
                                        mail_from: mailfrom.clone(),
                                        rcpt_to: rcpts.first().cloned(),
                                    });
                                }
                                writer
                                    .write_all(b"451 4.3.0 Local error in processing\r\n")
                                    .await?;
                            }
                        }
                        writer.flush().await?;

                        mailfrom = None;
                        rcpts.clear();
                        data_buffer.clear();
                        state = SmtpState::Command;
                    } else {
                        // Dot un-stuffing per RFC 5321
                        let line_content = if line_buf.starts_with("..") {
                            &line_buf[1..]
                        } else {
                            &line_buf[..]
                        };
                        data_buffer.push_str(line_content);
                    }
                }
            }
        }

        Ok(())
    }
}

fn extract_address_str(addr: &mail_parser::Address) -> Option<String> {
    match addr {
        mail_parser::Address::List(list) => list
            .first()
            .and_then(|a| a.address.as_deref())
            .map(extract_email),
        mail_parser::Address::Group(groups) => groups
            .first()
            .and_then(|g| g.addresses.first())
            .and_then(|a| a.address.as_deref())
            .map(extract_email),
    }
}

pub fn parse_raw_mime_to_payload(
    raw_mime: &[u8],
    smtp_mail_from: Option<&str>,
    smtp_rcpt_to: Option<&str>,
    _all_rcpts: &[String],
    mut spf_status: Option<String>,
    mut dkim_status: Option<String>,
    mut dmarc_status: Option<String>,
) -> RawInboundPayload {
    if let Some(msg) = mail_parser::MessageParser::new().parse(raw_mime) {
        // Extract sender
        let from = smtp_mail_from
            .filter(|s| !s.is_empty())
            .map(extract_email)
            .or_else(|| msg.from().and_then(extract_address_str))
            .unwrap_or_default();

        // Extract primary recipient ('to')
        let to = smtp_rcpt_to
            .filter(|s| !s.is_empty())
            .map(extract_email)
            .or_else(|| msg.to().and_then(extract_address_str))
            .unwrap_or_default();

        // Extract CC
        let cc = msg.cc().and_then(extract_address_str);

        // Extract subject
        let subject = msg.subject().map(|s| s.to_string());

        // Extract body text and html
        let text = msg.body_text(0).map(|t| t.to_string());
        let html = msg.body_html(0).map(|h| h.to_string());

        // Format headers string & fallback header-based SPF/DKIM extraction
        let headers = {
            let mut hdrs = String::new();
            for header in msg.headers() {
                let name = header.name();
                let name_lower = name.to_lowercase();

                if let Some(val_str) = header.value().as_text() {
                    let val_lower = val_str.to_lowercase();
                    if name_lower == "authentication-results" {
                        if val_lower.contains("spf=pass") {
                            spf_status = Some("pass".to_string());
                        } else if val_lower.contains("spf=fail") && spf_status.is_none() {
                            spf_status = Some("fail".to_string());
                        }
                    } else if name_lower == "received-spf" {
                        if val_lower.starts_with("pass") {
                            spf_status = Some("pass".to_string());
                        } else if val_lower.starts_with("fail") && spf_status.is_none() {
                            spf_status = Some("fail".to_string());
                        }
                    }

                    if name_lower == "authentication-results" {
                        if val_lower.contains("dkim=pass") {
                            dkim_status = Some("pass".to_string());
                        } else if val_lower.contains("dkim=fail") && dkim_status.is_none() {
                            dkim_status = Some("fail".to_string());
                        }

                        if val_lower.contains("dmarc=pass") {
                            dmarc_status = Some("pass".to_string());
                        } else if val_lower.contains("dmarc=fail") && dmarc_status.is_none() {
                            dmarc_status = Some("fail".to_string());
                        }
                    }

                    hdrs.push_str(name);
                    hdrs.push_str(": ");
                    hdrs.push_str(val_str);
                    hdrs.push('\n');
                } else if let Some(addr) = header.value().as_address() {
                    if let Some(addr_str) = extract_address_str(addr) {
                        hdrs.push_str(name);
                        hdrs.push_str(": ");
                        hdrs.push_str(&addr_str);
                        hdrs.push('\n');
                    }
                }
            }
            if hdrs.is_empty() { None } else { Some(hdrs) }
        };

        // Extract attachments
        let mut attachments_data = Vec::new();
        for att in msg.attachments() {
            let filename = att.attachment_name().unwrap_or("attachment").to_string();
            let content_type = att
                .content_type()
                .map(|c| c.c_type.to_string())
                .unwrap_or_else(|| "application/octet-stream".to_string());
            let content = att.contents().to_vec();

            attachments_data.push(RawAttachmentData {
                filename,
                content_type,
                content,
            });
        }

        RawInboundPayload {
            to,
            from,
            cc,
            subject,
            text,
            html,
            headers,
            spf: spf_status,
            dkim: dkim_status,
            dmarc: dmarc_status,
            spam_score: None,
            attachments_data,
        }
    } else {
        let raw_text = String::from_utf8_lossy(raw_mime).to_string();
        RawInboundPayload {
            to: smtp_rcpt_to.map(extract_email).unwrap_or_default(),
            from: smtp_mail_from.map(extract_email).unwrap_or_default(),
            text: Some(raw_text),
            spf: spf_status,
            dkim: dkim_status,
            dmarc: dmarc_status,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use mail_auth::common::verify::VerifySignature;
    use std::sync::Mutex;
    use uuid::Uuid;

    use crate::{
        app_error::AppResult,
        entities::{channel::Channel, company::Company, message::Message, thread::Thread},
        use_cases::{
            channel::ChannelPersistence, company::CompanyPersistence, thread::ThreadPersistence,
        },
    };

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(
            &self,
            _user_id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
            unimplemented!()
        }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Company>> {
            unimplemented!()
        }
        async fn get_by_slug(&self, slug: &str) -> AppResult<Option<Company>> {
            Ok(self
                .companies
                .lock()
                .unwrap()
                .iter()
                .find(|c| c.slug == slug)
                .cloned())
        }
        async fn list_by_user_id(&self, _user_id: Uuid) -> AppResult<Vec<Company>> {
            unimplemented!()
        }
        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _enable_llm_spam_guardrail: Option<bool>,
        ) -> AppResult<Company> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
        async fn is_company_team_member(&self, _company_id: Uuid, _email: &str) -> AppResult<bool> {
            Ok(true)
        }
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }
    }

    struct MockChannelPersistence {
        channels: Mutex<Vec<Channel>>,
    }

    #[async_trait]
    impl ChannelPersistence for MockChannelPersistence {
        async fn create(
            &self,
            _company_id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _participant_emails: Option<Vec<String>>,
            _agent_ids: Option<Vec<Uuid>>,
            _channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Channel>> {
            unimplemented!()
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &str,
            channel_slug: &str,
        ) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.slug == channel_slug)
                .cloned())
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(self.channels.lock().unwrap().clone())
        }
        async fn update(
            &self,
            _id: Uuid,
            _name: &str,
            _slug: &str,
            _api_key: Option<&str>,
            _provider: Option<&str>,
            _model: Option<&str>,
            _participant_emails: Option<Vec<String>>,
            _agent_ids: Option<Vec<Uuid>>,
            _channel_config: Option<serde_json::Value>,
        ) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
    }

    struct MockThreadPersistence {
        threads: Mutex<Vec<Thread>>,
        messages: Mutex<Vec<Message>>,
    }

    #[async_trait]
    impl ThreadPersistence for MockThreadPersistence {
        async fn create_thread(
            &self,
            channel_id: Uuid,
            subject: &str,
            participant_emails: &[String],
        ) -> AppResult<Thread> {
            let thread = Thread {
                id: Uuid::new_v4(),
                channel_id,
                subject: subject.to_string(),
                participant_emails: participant_emails.to_vec(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
            };
            self.threads.lock().unwrap().push(thread.clone());
            Ok(thread)
        }

        async fn get_thread_by_id(&self, id: Uuid) -> AppResult<Option<Thread>> {
            Ok(self
                .threads
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .cloned())
        }

        async fn update_thread_participants(
            &self,
            id: Uuid,
            participant_emails: &[String],
        ) -> AppResult<Thread> {
            let mut list = self.threads.lock().unwrap();
            let thread = list.iter_mut().find(|t| t.id == id).unwrap();
            thread.participant_emails = participant_emails.to_vec();
            Ok(thread.clone())
        }

        async fn find_thread_by_message_ids(
            &self,
            message_ids: &[String],
        ) -> AppResult<Option<Thread>> {
            let thread_id = {
                let msgs = self.messages.lock().unwrap();
                msgs.iter()
                    .find(|m| message_ids.contains(&m.message_id))
                    .map(|m| m.thread_id)
            };
            if let Some(tid) = thread_id {
                return self.get_thread_by_id(tid).await;
            }
            Ok(None)
        }

        async fn find_thread_by_thread_index(
            &self,
            thread_index_prefix: &str,
        ) -> AppResult<Option<Thread>> {
            let thread_id = {
                let msgs = self.messages.lock().unwrap();
                msgs.iter()
                    .find(|m| {
                        m.thread_index
                            .as_deref()
                            .unwrap_or_default()
                            .starts_with(thread_index_prefix)
                    })
                    .map(|m| m.thread_id)
            };
            if let Some(tid) = thread_id {
                return self.get_thread_by_id(tid).await;
            }
            Ok(None)
        }

        async fn count_recent_messages(
            &self,
            thread_id: Uuid,
            _duration_secs: i64,
        ) -> AppResult<usize> {
            let msgs = self.messages.lock().unwrap();
            Ok(msgs.iter().filter(|m| m.thread_id == thread_id).count())
        }

        async fn create_message(&self, message: &Message) -> AppResult<Message> {
            self.messages.lock().unwrap().push(message.clone());
            Ok(message.clone())
        }

        async fn get_message_by_message_id(&self, message_id: &str) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|m| m.message_id == message_id)
                .cloned())
        }

        async fn list_messages_by_thread_id(&self, thread_id: Uuid) -> AppResult<Vec<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.thread_id == thread_id)
                .cloned()
                .collect())
        }
    }

    struct MockTaskPersistence;

    #[async_trait]
    impl crate::adapters::persistence::task::TaskPersistence for MockTaskPersistence {
        async fn enqueue_task(
            &self,
            company_id: Uuid,
            channel_id: Uuid,
            thread_id: Option<Uuid>,
            task_type: &str,
            payload: serde_json::Value,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            Ok(crate::entities::task::BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id,
                task_type: task_type.to_string(),
                status: crate::entities::task::TaskStatus::Pending,
                payload,
                retry_count: 0,
                max_retries: 3,
                last_error: None,
                run_at: Utc::now().naive_utc(),
                created_at: Utc::now().naive_utc(),
                updated_at: Utc::now().naive_utc(),
            })
        }
        async fn get_task_by_id(
            &self,
            _id: Uuid,
        ) -> AppResult<Option<crate::entities::task::BackgroundTask>> {
            Ok(None)
        }
        async fn update_task_payload(
            &self,
            _id: Uuid,
            _payload: serde_json::Value,
        ) -> AppResult<()> {
            Ok(())
        }
        async fn poll_next_pending_tasks(
            &self,
            _limit: i64,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            Ok(vec![])
        }
        async fn mark_task_processing(&self, _id: Uuid) -> AppResult<bool> {
            Ok(true)
        }
        async fn mark_task_completed(&self, _id: Uuid) -> AppResult<()> {
            Ok(())
        }
        async fn mark_task_failed(
            &self,
            _id: Uuid,
            _error_msg: &str,
            _next_run_at: chrono::NaiveDateTime,
            _is_dead_letter: bool,
        ) -> AppResult<()> {
            Ok(())
        }
        async fn stop_task(&self, _id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn resume_task(&self, _id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn update_task_status(
            &self,
            _id: Uuid,
            _status: crate::entities::task::TaskStatus,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn list_company_tasks(
            &self,
            _company_id: Uuid,
            _channel_id: Option<Uuid>,
            _status: Option<crate::entities::task::TaskStatus>,
            _sort_asc: bool,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_parse_raw_mime_to_payload() {
        let raw_mime = b"From: Sender <sender@external.com>\r\nTo: Agent <inbound@acme.mailagents.com>\r\nSubject: Test Email\r\nMessage-ID: <TEST12345@external.com>\r\nAuthentication-Results: spf=pass dkim=pass\r\nContent-Type: text/plain\r\n\r\nHello from SMTP server!";

        let payload = parse_raw_mime_to_payload(
            raw_mime,
            Some("sender@external.com"),
            Some("inbound@acme.mailagents.com"),
            &["inbound@acme.mailagents.com".to_string()],
            None,
            None,
            None,
        );

        assert_eq!(payload.from, "sender@external.com");
        assert_eq!(payload.to, "inbound@acme.mailagents.com");
        assert_eq!(payload.subject.as_deref(), Some("Test Email"));
        assert!(
            payload
                .text
                .as_deref()
                .unwrap_or_default()
                .contains("Hello from SMTP server!")
        );
        assert!(
            payload
                .headers
                .as_deref()
                .unwrap_or_default()
                .contains("Message-ID")
        );
        assert_eq!(payload.spf.as_deref(), Some("pass"));
        assert_eq!(payload.dkim.as_deref(), Some("pass"));
    }

    #[tokio::test]
    async fn test_smtp_server_end_to_end_ingestion() {
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                slug: "acme".to_string(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: Uuid::new_v4(),
                company_id,
                name: "Inbound Flow".to_string(),
                slug: "inbound".to_string(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let task_persistence = Arc::new(MockTaskPersistence);

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "127.0.0.1".to_string(),
            incoming_smtp_port: 0, // OS assigns port
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence,
            config.clone(),
        ));

        let server = Arc::new(SmtpServer::new(thread_use_cases, config));

        // Bind listener on 127.0.0.1:0
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        let server_clone = server.clone();
        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            server_clone
                .handle_connection(stream, peer_addr)
                .await
                .unwrap();
        });

        // Client connection and SMTP dialog
        let mut client = TcpStream::connect(local_addr).await.unwrap();
        let (reader, mut writer) = client.split();
        let mut buf_reader = BufReader::new(reader);
        let mut response = String::new();

        // 220 Greeting
        buf_reader.read_line(&mut response).await.unwrap();
        assert!(response.contains("220"));

        // EHLO
        response.clear();
        writer
            .write_all(b"EHLO client.example.com\r\n")
            .await
            .unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();
        assert!(response.contains("250"));

        // Read rest of EHLO multiline response
        while buf_reader.read_line(&mut response).await.unwrap() > 0 {
            if response.ends_with("250 OK\r\n") {
                break;
            }
        }

        // MAIL FROM
        response.clear();
        writer
            .write_all(b"MAIL FROM:<sender@external.com>\r\n")
            .await
            .unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();
        assert!(response.contains("250"));

        // RCPT TO
        response.clear();
        writer
            .write_all(b"RCPT TO:<inbound@acme.mailagents.com>\r\n")
            .await
            .unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();
        assert!(response.contains("250"));

        // DATA
        response.clear();
        writer.write_all(b"DATA\r\n").await.unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();
        assert!(response.contains("354"));

        // Email Body
        let email_data = b"From: sender@external.com\r\nTo: inbound@acme.mailagents.com\r\nSubject: SMTP Test Order\r\nMessage-ID: <SMTP123@external.com>\r\nAuthentication-Results: spf=pass dkim=pass dmarc=pass\r\n\r\nHello agent, please process this order via SMTP.\r\n.\r\n";
        response.clear();
        writer.write_all(email_data).await.unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();
        assert!(response.contains("250"));

        // QUIT
        response.clear();
        writer.write_all(b"QUIT\r\n").await.unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();
        assert!(response.contains("221"));

        // Allow async background agent task to run
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Verify Message in DB
        let threads = thread_persistence.threads.lock().unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].subject, "SMTP Test Order");

        let messages = thread_persistence.messages.lock().unwrap();
        assert_eq!(messages.len(), 2); // 1 Inbound Human + 1 Outbound Agent
        assert_eq!(
            messages[0].clean_text_body,
            "Hello agent, please process this order via SMTP."
        );
        assert_eq!(
            messages[0].role,
            crate::entities::message::MessageRole::Human
        );
        assert_eq!(
            messages[0].direction,
            crate::entities::message::MessageDirection::Inbound
        );
    }

    #[tokio::test]
    async fn test_dkim_verification_with_gmail_raw_email() {
        let msg = "Received: by mail-pj1-f42.google.com with SMTP id 98e67ed59e1d1-2f83a8afcbbso7421825a91.1for <reg@populus.network>; Mon, 17 Feb 2025 15:18:31 -0800 (PST)\n\
DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed;d=gmail.com; s=20230601; t=1739834309; x=1740439109; darn=populus.network; h=to:subject:message-id:date:from:mime-version:from:to:cc:subject:date:message-id:reply-to; bh=x19wIji+Z4l9gErNFMJoiHzL6DQXpodslSPSAwR1YBY=; b=GAEwDJgLvTeL0qx6uJiUcEm/jUeoaP8bkDyToJnmOAG5uMssxa9ZSUqF1TbF6vZHcp7/VwrZvazoVgs3jb+0u70/99y6oGtv6JoGz+tDxIAVZNzufE2ZaXeWO1SDAkdEWRRQr4GAssq0Ht3DnDs3ck6h0YwIjUbspDYTgutQ3/e1d5tRG3VwjOa1pJQqwU0lfFTYZe1RJad1Ag9iw1KvceyMSB4lduGQ9/TxxvrEqbomqZ04xwY+cSYEmx7opp6tCI/LiQh6syLh4q5aay7Sop4KZhNRrhoU4+Q3jqHVzGRpz4pk6RPRjie8I9ZDCuXPCFHLpuL/5PtnQ/vNDrLmDQ==\n\
X-Google-DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed;d=1e100.net; s=20230601; t=1739834309; x=1740439109;h=to:subject:message-id:date:from:mime-version:x-gm-message-state:from:to:cc:subject:date:message-id:reply-to;bh=x19wIji+Z4l9gErNFMJoiHzL6DQXpodslSPSAwR1YBY=;b=eIZEuOD890TXcD8E7BoEI6+uPK1f+tfovN+dqlbEwreeQnN8WNP5IQBylFbytwpkz9oXOP5Jj/4TEo1uXNoQlJlelYbn3irxL9MoiTZLOj5w6wQdZCNu2AY5gN3Mh3shiDML5OeHYt9fAcBn+0AkJ8Id9TqtcMiGWkFlrTRdzqFAzD0fDXFTm6YpTGdwb1qaq6sQ4S6Ums+T53ZLC+2K1gYtXx3dsTd8AU3D2sTUKv5iIMInX1QY3Lg9iH0Sc/Xu1L+h0liI87clIq+TORuWUhVhhyBLWdaF+dqsvQRFY/+tyqzO2LuZA006UT9dAIYd/Lic3ydFK/fQah1G3m7JdQ==\n\
MIME-Version: 1.0\n\
From: Populus <hello.populus@gmail.com>\n\
Date: Tue, 18 Feb 2025 00:18:18 +0100\n\
Message-ID: <CAGj=2VKEn_MHfovWkBCqn4sp3AXPR=ZTLMso=mPjWtnMDStiRw@mail.gmail.com>\n\
Subject: reg\n\
To: reg@populus.network\n\
Content-Type: text/plain; charset=\"UTF-8\"\n\n\
regis";

        let msg_crlf = msg.replace("\r\n", "\n").replace('\n', "\r\n");
        let auth_msg = mail_auth::AuthenticatedMessage::parse(msg_crlf.as_bytes())
            .expect("Message must be parseable");
        if let Ok(resolver) = mail_auth::MessageAuthenticator::new_quad9() {
            let dkim_result = resolver.verify_dkim(&auth_msg).await;
            println!("DKIM verification result: {:?}", dkim_result);
            assert!(
                !dkim_result.is_empty(),
                "DKIM signature should be parsed and evaluated"
            );
            assert_eq!(
                dkim_result[0].signature().as_ref().unwrap().domain(),
                "gmail.com"
            );
            assert_eq!(
                dkim_result[0].signature().as_ref().unwrap().selector(),
                "20230601"
            );
        }
    }

    #[tokio::test]
    async fn test_spf_verification_with_google_ip() {
        if let Ok(resolver) = mail_auth::MessageAuthenticator::new_quad9() {
            let params = mail_auth::spf::verify::SpfParameters::verify_mail_from(
                "209.85.216.42".parse().unwrap(),
                "mail-pj1-f42.google.com",
                "gmail.com",
                "hello.populus@gmail.com",
            );
            let spf_res = resolver.verify_spf(params).await;
            println!("SPF verification result: {:?}", spf_res.result());
            assert_eq!(spf_res.result(), mail_auth::SpfResult::Pass);
        }
    }

    #[tokio::test]
    async fn test_smtp_server_end_to_end_with_dkim_raw_email() {
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Populus Network".to_string(),
                slug: "populus".to_string(),
                api_key: None,
                provider: None,
                model: None,
                enable_llm_spam_guardrail: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                id: Uuid::new_v4(),
                company_id,
                name: "Reg Channel".to_string(),
                slug: "network".to_string(),
                api_key: None,
                provider: None,
                model: None,
                participant_emails: None,
                agent_ids: None,
                channel_config: None,
                created_at: Utc::now().naive_utc(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let task_persistence = Arc::new(MockTaskPersistence);

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "127.0.0.1".to_string(),
            incoming_smtp_port: 0,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 30,
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence,
            config.clone(),
        ));

        let server = Arc::new(SmtpServer::new(thread_use_cases, config));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        let server_clone = server.clone();
        tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            server_clone
                .handle_connection(stream, peer_addr)
                .await
                .unwrap();
        });

        let mut client = TcpStream::connect(local_addr).await.unwrap();
        let (reader, mut writer) = client.split();
        let mut buf_reader = BufReader::new(reader);
        let mut response = String::new();

        buf_reader.read_line(&mut response).await.unwrap();
        writer
            .write_all(b"EHLO mail-pj1-f42.google.com\r\n")
            .await
            .unwrap();
        writer.flush().await.unwrap();
        while buf_reader.read_line(&mut response).await.unwrap() > 0 {
            if response.ends_with("250 OK\r\n") {
                break;
            }
        }

        writer
            .write_all(b"MAIL FROM:<hello.populus@gmail.com>\r\n")
            .await
            .unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();

        writer
            .write_all(b"RCPT TO:<network@populus.mailagents.com>\r\n")
            .await
            .unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();

        writer.write_all(b"DATA\r\n").await.unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();

        let raw_msg = "Received: by mail-pj1-f42.google.com with SMTP id 98e67ed59e1d1-2f83a8afcbbso7421825a91.1for <network@populus.mailagents.com>; Mon, 17 Feb 2025 15:18:31 -0800 (PST)\r\n\
DKIM-Signature: v=1; a=rsa-sha256; c=relaxed/relaxed;d=gmail.com; s=20230601; t=1739834309; x=1740439109; darn=populus.network; h=to:subject:message-id:date:from:mime-version:from:to:cc:subject:date:message-id:reply-to; bh=x19wIji+Z4l9gErNFMJoiHzL6DQXpodslSPSAwR1YBY=; b=GAEwDJgLvTeL0qx6uJiUcEm/jUeoaP8bkDyToJnmOAG5uMssxa9ZSUqF1TbF6vZHcp7/VwrZvazoVgs3jb+0u70/99y6oGtv6JoGz+tDxIAVZNzufE2ZaXeWO1SDAkdEWRRQr4GAssq0Ht3DnDs3ck6h0YwIjUbspDYTgutQ3/e1d5tRG3VwjOa1pJQqwU0lfFTYZe1RJad1Ag9iw1KvceyMSB4lduGQ9/TxxvrEqbomqZ04xwY+cSYEmx7opp6tCI/LiQh6syLh4q5aay7Sop4KZhNRrhoU4+Q3jqHVzGRpz4pk6RPRjie8I9ZDCuXPCFHLpuL/5PtnQ/vNDrLmDQ==\r\n\
Authentication-Results: spf=pass dkim=pass dmarc=pass\r\n\
From: Populus <hello.populus@gmail.com>\r\n\
To: network@populus.mailagents.com\r\n\
Subject: Registration Request\r\n\
Message-ID: <CAGj=2VKEn_MHfovWkBCqn4sp3AXPR=ZTLMso=mPjWtnMDStiRw@mail.gmail.com>\r\n\r\nPlease register my account.\r\n.\r\n";

        writer.write_all(raw_msg.as_bytes()).await.unwrap();
        writer.flush().await.unwrap();
        response.clear();
        buf_reader.read_line(&mut response).await.unwrap();
        assert!(response.contains("250"));

        writer.write_all(b"QUIT\r\n").await.unwrap();
        writer.flush().await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let messages = thread_persistence.messages.lock().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].clean_text_body, "Please register my account.");
    }

    #[tokio::test]
    async fn test_smtp_rate_limiting_and_monitoring() {
        use crate::adapters::monitoring::InMemoryMonitor;

        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(Vec::new()),
        });
        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(Vec::new()),
        });
        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });
        let task_persistence = Arc::new(MockTaskPersistence);

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            access_token_ttl: time::Duration::days(1),
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            smtp_host: "localhost".to_string(),
            smtp_port: 1025,
            smtp_username: "".to_string(),
            smtp_password: "".to_string(),
            smtp_from_address: "noreply@mailagents.com".to_string(),
            incoming_smtp_enabled: true,
            incoming_smtp_host: "127.0.0.1".to_string(),
            incoming_smtp_port: 0,
            max_spam_score: 5.0,
            dnsbl_enabled: false,
            dnsbl_servers: vec![],
            smtp_rate_limit_conns_per_ip: 1, // Limit to 1 connection
            reject_self_domain_helo: true,
            enable_heuristic_scanner: true,
            enable_spam_scanner: false,
            spam_scanner_type: "rspamd".to_string(),
            spam_scanner_url: "http://localhost:11333/checkv2".to_string(),
            enable_llm_spam_guardrail: false,
        });

        let monitor = Arc::new(InMemoryMonitor::new());

        let thread_use_cases = Arc::new(ThreadUseCases::new(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence,
            task_persistence,
            config.clone(),
        ));

        let server =
            Arc::new(SmtpServer::new(thread_use_cases, config).with_monitoring(monitor.clone()));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = listener.local_addr().unwrap();

        let server_clone = server.clone();
        tokio::spawn(async move {
            while let Ok((stream, peer_addr)) = listener.accept().await {
                let s = server_clone.clone();
                tokio::spawn(async move {
                    let _ = s.handle_connection(stream, peer_addr).await;
                });
            }
        });

        // 1st connection (holds the connection open)
        let mut client1 = TcpStream::connect(local_addr).await.unwrap();
        let (reader1, _) = client1.split();
        let mut buf_reader1 = BufReader::new(reader1);
        let mut response1 = String::new();
        buf_reader1.read_line(&mut response1).await.unwrap();
        assert!(response1.contains("220"));

        // 2nd concurrent connection (should be rate-limited)
        let mut client2 = TcpStream::connect(local_addr).await.unwrap();
        let (reader2, _) = client2.split();
        let mut buf_reader2 = BufReader::new(reader2);
        let mut response2 = String::new();
        buf_reader2.read_line(&mut response2).await.unwrap();
        assert!(response2.contains("421"));

        let stats = monitor.get_stats_json();
        assert_eq!(stats["smtp_connections"]["blocked_rate_limit"], 1);
    }
}
