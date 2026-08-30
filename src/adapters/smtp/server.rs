use mail_parser::MimeHeaders;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{error, info, warn};

use crate::{
    adapters::protocols::email::EmailIngressAdapter,
    application::use_cases::channel::parse_recipient_address,
    application::use_cases::thread::ThreadUseCases,
    domain::monitoring::{MonitoringService, SmtpConnectionMetrics, SmtpStatus},
    entities::auth::AuthVerdict,
    entities::company_member::CompanyMembership,
    infra::config::AppConfig,
    services::email_parser::{
        MAX_INBOUND_MESSAGE_BYTES, RawAttachmentData, RawInboundPayload, extract_email,
    },
};

static MAIL_FROM_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?i)from:\s*<([^>]+)>|from:\s*([^\s]+)").unwrap()
});
static RCPT_TO_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"(?i)to:\s*<([^>]+)>|to:\s*([^\s]+)").unwrap());

const MAX_COMMAND_LINE_BYTES: usize = 512;
const MAX_DATA_LINE_BYTES: usize = 1_000;
const MAX_RECIPIENTS: usize = 100;
const MAX_COMMANDS: usize = 1_000;
const MAX_CONNECTIONS: usize = 256;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);
const DATA_TIMEOUT: Duration = Duration::from_secs(300);
const SESSION_TIMEOUT: Duration = Duration::from_secs(600);
const DNS_TIMEOUT: Duration = Duration::from_secs(5);

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
    /// Live connection count per client IP, enforcing `smtp_rate_limit_conns_per_ip`.
    ///
    /// **This counter is process-local, so the limit is per instance, not per deployment.** Running
    /// N servers behind one address means a single IP can hold N × `smtp_rate_limit_conns_per_ip`
    /// connections in total. That is a deliberate trade — it needs no shared state on the hot
    /// connection-accept path — but it means the configured number is a per-machine budget, and
    /// scaling out quietly loosens the effective limit. Enforcing a true global cap would require
    /// moving this to shared storage (Postgres or Redis) and paying a round trip per connection.
    active_conns: Arc<RwLock<HashMap<IpAddr, usize>>>,
    connection_slots: Arc<Semaphore>,
}

impl SmtpServer {
    pub fn new(thread_use_cases: Arc<ThreadUseCases>, config: Arc<AppConfig>) -> Self {
        Self {
            thread_use_cases,
            config,
            monitoring: None,
            active_conns: Arc::new(RwLock::new(HashMap::new())),
            connection_slots: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
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

        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                res = listener.accept() => {
                    match res {
                        Ok((stream, peer_addr)) => {
                            let Ok(slot) = self.connection_slots.clone().try_acquire_owned() else {
                                let mut stream = stream;
                                let _ = stream.write_all(b"421 4.3.2 Server busy, try again later\r\n").await;
                                continue;
                            };
                            let server = self.clone();
                            connections.spawn(async move {
                                let _slot = slot;
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
                Some(result) = connections.join_next(), if !connections.is_empty() => {
                    if let Err(error) = result {
                        warn!(%error, "SMTP connection task failed");
                    }
                }
            }
        }
        connections.abort_all();
        while connections.join_next().await.is_some() {}
    }

    async fn handle_connection(
        &self,
        stream: TcpStream,
        peer_addr: std::net::SocketAddr,
    ) -> anyhow::Result<()> {
        timeout(SESSION_TIMEOUT, self.handle_session(stream, peer_addr))
            .await
            .map_err(|_| anyhow::anyhow!("SMTP session timed out"))?
    }

    async fn handle_session(
        &self,
        mut stream: TcpStream,
        peer_addr: std::net::SocketAddr,
    ) -> anyhow::Result<()> {
        let start_time = Instant::now();
        let client_ip = peer_addr.ip();

        let Some(_guard) = self
            .admit_connection(&mut stream, client_ip, start_time)
            .await?
        else {
            return Ok(());
        };

        let (reader, mut writer) = stream.split();
        let mut reader = BufReader::new(reader);

        let banner = format!(
            "220 {} ESMTP Service Ready\r\n",
            self.config.app_domain_name
        );
        writer.write_all(banner.as_bytes()).await?;
        writer.flush().await?;

        let mut state = SmtpState::Command;
        let mut session = SmtpSession::default();
        let mut command_count = 0usize;

        loop {
            let (line_limit, wait) = match state {
                SmtpState::Command => (MAX_COMMAND_LINE_BYTES, COMMAND_TIMEOUT),
                SmtpState::Data => (MAX_DATA_LINE_BYTES, DATA_TIMEOUT),
            };
            let line = match timeout(wait, read_limited_line(&mut reader, line_limit)).await {
                Err(_) => {
                    writer
                        .write_all(b"421 4.4.2 Timeout waiting for input\r\n")
                        .await?;
                    break;
                }
                Ok(Err(LineReadError::TooLong)) => {
                    writer.write_all(b"500 5.2.3 Line too long\r\n").await?;
                    break;
                }
                Ok(Err(LineReadError::Io(error))) => return Err(error.into()),
                Ok(Ok(line)) => line,
            };
            if line.is_empty() {
                break; // Connection closed
            }
            let trimmed = line.strip_suffix(b"\n").unwrap_or(&line);
            let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);

            match state {
                SmtpState::Command => {
                    command_count += 1;
                    if command_count > MAX_COMMANDS {
                        writer.write_all(b"421 4.5.3 Too many commands\r\n").await?;
                        break;
                    }
                    let Ok(trimmed) = std::str::from_utf8(trimmed) else {
                        writer
                            .write_all(b"500 5.5.2 Command must be ASCII\r\n")
                            .await?;
                        continue;
                    };
                    let outcome = self
                        .handle_command(trimmed, &mut session, &mut writer, peer_addr, start_time)
                        .await?;
                    writer.flush().await?;
                    match outcome {
                        CommandOutcome::Continue => {}
                        CommandOutcome::EnterData => state = SmtpState::Data,
                        CommandOutcome::Close => break,
                    }
                }
                SmtpState::Data => {
                    if trimmed == b"." {
                        match timeout(
                            DATA_TIMEOUT,
                            self.finish_data_transaction(
                                &mut session,
                                &mut writer,
                                peer_addr,
                                start_time,
                            ),
                        )
                        .await
                        {
                            Ok(result) => result?,
                            Err(_) => {
                                writer
                                    .write_all(b"451 4.4.2 Message processing timed out\r\n")
                                    .await?
                            }
                        }
                        session.reset_transaction();
                        state = SmtpState::Command;
                    } else {
                        if !session.push_data_line(&line) {
                            writer
                                .write_all(
                                    b"552 5.3.4 Message exceeds fixed maximum message size\r\n",
                                )
                                .await?;
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Per-IP connection limiting and DNSBL screening, before a single SMTP verb is read.
    ///
    /// `Ok(None)` means the client was turned away and already told why; the returned guard keeps
    /// the IP's connection count incremented for as long as the caller holds it.
    async fn admit_connection(
        &self,
        stream: &mut TcpStream,
        client_ip: IpAddr,
        start_time: Instant,
    ) -> anyhow::Result<Option<ConnGuard>> {
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
            self.record_connection(
                client_ip,
                SmtpStatus::BlockedRateLimit,
                start_time,
                None,
                None,
            );
            stream
                .write_all(b"421 4.7.0 Too many active connections from your IP address\r\n")
                .await?;
            stream.flush().await?;
            return Ok(None);
        }

        let guard = ConnGuard {
            ip: client_ip,
            conns: self.active_conns.clone(),
        };

        if self.config.dnsbl_enabled
            && let Ok(Some(rbl)) = timeout(
                DNS_TIMEOUT,
                check_dnsbl(client_ip, &self.config.dnsbl_servers),
            )
            .await
        {
            warn!("Connection from {} blocked by DNSBL {}", client_ip, rbl);
            self.record_connection(client_ip, SmtpStatus::BlockedDnsbl, start_time, None, None);
            let msg = format!(
                "554 5.7.1 Service unavailable; Client host [{}] blocked using {}\r\n",
                client_ip, rbl
            );
            stream.write_all(msg.as_bytes()).await?;
            stream.flush().await?;
            return Ok(None);
        }

        Ok(Some(guard))
    }

    /// One SMTP verb. The caller flushes and applies the returned state change.
    async fn handle_command<W: tokio::io::AsyncWrite + Unpin>(
        &self,
        line: &str,
        session: &mut SmtpSession,
        writer: &mut W,
        peer_addr: std::net::SocketAddr,
        start_time: Instant,
    ) -> anyhow::Result<CommandOutcome> {
        let space_pos = line.find(' ').unwrap_or(line.len());
        let (cmd_raw, arg_raw) = line.split_at(space_pos);
        let cmd = cmd_raw.trim().to_uppercase();
        let arg = arg_raw.trim();

        match cmd.as_str() {
            "HELO" | "EHLO" => {
                let client_ip = peer_addr.ip();
                // A remote client claiming to be us is forging our own domain.
                if !arg.is_empty() {
                    if arg.eq_ignore_ascii_case(&self.config.app_domain_name)
                        && self.config.reject_self_domain_helo
                        && !client_ip.is_loopback()
                    {
                        warn!(
                            "Client {} spoofed local domain {} in EHLO/HELO",
                            client_ip, arg
                        );
                        self.record_connection(
                            client_ip,
                            SmtpStatus::RejectedHelo,
                            start_time,
                            None,
                            None,
                        );
                        writer
                            .write_all(b"550 5.5.2 Helo command rejected: Host name spoofed\r\n")
                            .await?;
                        writer.flush().await?;
                        return Ok(CommandOutcome::Close);
                    }
                    session.ehlo_domain = Some(arg.to_string());
                }
                let resp = format!(
                    "250-{} Hello {}\r\n250-SIZE {}\r\n250 OK\r\n",
                    self.config.app_domain_name, client_ip, MAX_INBOUND_MESSAGE_BYTES
                );
                writer.write_all(resp.as_bytes()).await?;
            }
            "MAIL" => match extract_command_address(&MAIL_FROM_RE, arg) {
                Some(address) => {
                    if declared_message_size(arg)
                        .is_some_and(|size| size > MAX_INBOUND_MESSAGE_BYTES)
                    {
                        writer
                            .write_all(b"552 5.3.4 Message exceeds fixed maximum message size\r\n")
                            .await?;
                        return Ok(CommandOutcome::Continue);
                    }
                    session.mailfrom = Some(address);
                    writer.write_all(b"250 2.1.0 Ok\r\n").await?;
                }
                None => {
                    writer
                        .write_all(b"501 Syntax: MAIL FROM:<address>\r\n")
                        .await?;
                }
            },
            "RCPT" => {
                if session.mailfrom.is_none() {
                    writer
                        .write_all(b"503 Error: Send MAIL FROM first\r\n")
                        .await?;
                } else {
                    match extract_command_address(&RCPT_TO_RE, arg) {
                        Some(address) => {
                            if session.rcpts.len() >= MAX_RECIPIENTS {
                                writer
                                    .write_all(b"452 4.5.3 Too many recipients\r\n")
                                    .await?;
                                return Ok(CommandOutcome::Continue);
                            }
                            session.rcpts.push(address);
                            writer.write_all(b"250 2.1.5 Ok\r\n").await?;
                        }
                        None => {
                            writer
                                .write_all(b"501 Syntax: RCPT TO:<address>\r\n")
                                .await?;
                        }
                    }
                }
            }
            "DATA" => {
                if session.rcpts.is_empty() {
                    writer
                        .write_all(b"503 Error: MAIL FROM and RCPT TO must be set before DATA\r\n")
                        .await?;
                } else {
                    writer
                        .write_all(b"354 Start mail input; end with <CR><LF>.<CR><LF>\r\n")
                        .await?;
                    return Ok(CommandOutcome::EnterData);
                }
            }
            "RSET" => {
                session.reset_transaction();
                writer.write_all(b"250 2.0.0 Ok\r\n").await?;
            }
            "NOOP" => {
                writer.write_all(b"250 2.0.0 Ok\r\n").await?;
            }
            "QUIT" => {
                writer.write_all(b"221 2.0.0 Bye\r\n").await?;
                writer.flush().await?;
                return Ok(CommandOutcome::Close);
            }
            _ => {
                writer
                    .write_all(b"500 5.5.1 Command unrecognized\r\n")
                    .await?;
            }
        }
        Ok(CommandOutcome::Continue)
    }

    /// The terminating `.` arrived: authenticate, score, ingest, and answer the client.
    async fn finish_data_transaction<W: tokio::io::AsyncWrite + Unpin>(
        &self,
        session: &SmtpSession,
        writer: &mut W,
        peer_addr: std::net::SocketAddr,
        start_time: Instant,
    ) -> anyhow::Result<()> {
        let client_ip = peer_addr.ip();
        let auth = verify_email_authentication(
            &session.data_buffer,
            session.mailfrom.as_deref(),
            client_ip,
        )
        .await;

        let mut raw_payload = parse_raw_mime_to_payload(
            &session.data_buffer,
            session.mailfrom.as_deref(),
            session.rcpts.first().map(|s| s.as_str()),
            &session.rcpts,
            auth.spf,
            auth.dkim,
            auth.dmarc,
        );

        if self.sender_needs_spam_scan(&raw_payload).await {
            self.apply_spam_score(session, &mut raw_payload).await;
        }

        let norm_payload = EmailIngressAdapter::parse_and_store(
            raw_payload,
            &self.config,
            self.thread_use_cases.file_storage(),
        )
        .await;
        match self
            .thread_use_cases
            .ingest_normalized_message(norm_payload)
            .await
        {
            Ok(ingest) => {
                self.record_connection(
                    client_ip,
                    ingest_status(&ingest),
                    start_time,
                    session.mailfrom.clone(),
                    session.rcpts.first().cloned(),
                );

                if ingest.accepted {
                    writer
                        .write_all(b"250 2.0.0 Message queued for delivery\r\n")
                        .await?;
                } else {
                    let msg = format!(
                        "550 5.7.1 Message rejected ({})\r\n",
                        ingest.reason.as_deref().unwrap_or("ok")
                    );
                    writer.write_all(msg.as_bytes()).await?;
                }
            }
            Err(err) => {
                warn!("Error ingesting SMTP email: {err}");
                self.record_connection(
                    client_ip,
                    SmtpStatus::Error,
                    start_time,
                    session.mailfrom.clone(),
                    session.rcpts.first().cloned(),
                );
                writer
                    .write_all(b"451 4.3.0 Local error in processing\r\n")
                    .await?;
            }
        }
        writer.flush().await?;
        Ok(())
    }

    /// Spam scanning is for strangers: a sender the destination channel already trusts skips it.
    async fn sender_needs_spam_scan(&self, raw_payload: &RawInboundPayload) -> bool {
        let Some((company_slug, channel_slug)) =
            parse_recipient_address(&raw_payload.to, &self.config.app_domain_name)
        else {
            return true;
        };
        let Ok(Some(company)) = self
            .thread_use_cases
            .company_persistence()
            .get_by_slug(&company_slug)
            .await
        else {
            return true;
        };
        let Ok(Some(channel)) = self
            .thread_use_cases
            .channel_persistence()
            .get_by_company_slug_and_channel_slug(&company_slug, &channel_slug)
            .await
        else {
            return true;
        };

        let sender = raw_payload.from.trim();
        let membership = match self
            .thread_use_cases
            .company_persistence()
            .membership_for_email(company.id, sender)
            .await
        {
            Ok(membership) => membership,
            Err(error) => {
                // Every other bail-out here scans too: a directory lookup that failed must not be
                // the reason a stranger is promoted to a trusted sender.
                warn!(%error, "Membership lookup failed; scanning sender as a stranger");
                CompanyMembership::None
            }
        };
        !channel.participant_access(sender, membership).trusted
    }

    async fn apply_spam_score(&self, session: &SmtpSession, raw_payload: &mut RawInboundPayload) {
        let scanner = crate::services::spam_scanner::SpamScannerService::new(self.config.clone());
        let scan_res = scanner
            .scan(
                &session.data_buffer,
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

    fn record_connection(
        &self,
        client_ip: IpAddr,
        status: SmtpStatus,
        start_time: Instant,
        mail_from: Option<String>,
        rcpt_to: Option<String>,
    ) {
        if let Some(ref m) = self.monitoring {
            m.record_smtp_connection(&SmtpConnectionMetrics {
                client_ip,
                status,
                duration_ms: start_time.elapsed().as_millis() as u64,
                mail_from,
                rcpt_to,
            });
        }
    }
}

/// Per-connection SMTP transaction state.
#[derive(Default)]
struct SmtpSession {
    ehlo_domain: Option<String>,
    mailfrom: Option<String>,
    rcpts: Vec<String>,
    data_buffer: Vec<u8>,
}

impl SmtpSession {
    /// `RSET`, or the end of a message: the greeting survives, the envelope does not.
    fn reset_transaction(&mut self) {
        self.mailfrom = None;
        self.rcpts.clear();
        self.data_buffer.clear();
    }

    fn push_data_line(&mut self, raw_line: &[u8]) -> bool {
        // Dot un-stuffing per RFC 5321.
        let line = if raw_line.starts_with(b"..") {
            &raw_line[1..]
        } else {
            raw_line
        };
        if self.data_buffer.len().saturating_add(line.len()) > MAX_INBOUND_MESSAGE_BYTES {
            return false;
        }
        self.data_buffer.extend_from_slice(line);
        true
    }
}

#[derive(Debug)]
enum LineReadError {
    Io(std::io::Error),
    TooLong,
}

async fn read_limited_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Vec<u8>, LineReadError> {
    let mut line = Vec::with_capacity(max_bytes.min(1024));
    let read = reader
        .take((max_bytes + 1) as u64)
        .read_until(b'\n', &mut line)
        .await
        .map_err(LineReadError::Io)?;
    if read > max_bytes || (read == max_bytes && !line.ends_with(b"\n")) {
        return Err(LineReadError::TooLong);
    }
    Ok(line)
}

fn declared_message_size(argument: &str) -> Option<usize> {
    argument.split_ascii_whitespace().find_map(|part| {
        let (name, value) = part.split_once('=')?;
        name.eq_ignore_ascii_case("SIZE")
            .then(|| value.parse().ok())
            .flatten()
    })
}

/// What the command loop should do after one verb.
enum CommandOutcome {
    Continue,
    EnterData,
    Close,
}

/// DNS-backed authentication shared by direct SMTP and authenticated upstream ingress.
pub(crate) async fn verify_email_authentication(
    raw_mime: &[u8],
    mail_from: Option<&str>,
    client_ip: IpAddr,
) -> AuthResults {
    let mut results = AuthResults::default();
    let Ok(resolver) = mail_auth::MessageAuthenticator::new_quad9() else {
        return results;
    };

    let mut spf_output = None;
    if let Some(sender) = mail_from {
        let domain = sender.split('@').nth(1).unwrap_or(sender);
        let spf_res = resolver
            .verify_spf(mail_auth::spf::verify::SpfParameters::verify_mail_from(
                client_ip, domain, domain, sender,
            ))
            .await;
        results.spf = match spf_res.result() {
            mail_auth::SpfResult::Pass => AuthVerdict::Pass,
            mail_auth::SpfResult::Fail => AuthVerdict::Fail,
            mail_auth::SpfResult::SoftFail => AuthVerdict::SoftFail,
            mail_auth::SpfResult::Neutral => AuthVerdict::Neutral,
            mail_auth::SpfResult::TempError => AuthVerdict::TempError,
            mail_auth::SpfResult::PermError => AuthVerdict::PermError,
            _ => AuthVerdict::Unavailable,
        };
        spf_output = Some(spf_res);
    }

    let Some(auth_msg) = mail_auth::AuthenticatedMessage::parse(raw_mime) else {
        return results;
    };
    let dkim_outputs = resolver.verify_dkim(&auth_msg).await;
    if !dkim_outputs.is_empty() {
        results.dkim = if dkim_outputs
            .iter()
            .any(|output| matches!(output.result(), mail_auth::DkimResult::Pass))
        {
            AuthVerdict::Pass
        } else if dkim_outputs
            .iter()
            .any(|output| matches!(output.result(), mail_auth::DkimResult::Fail(_)))
        {
            AuthVerdict::Fail
        } else {
            AuthVerdict::Unavailable
        };
    }

    if let (Some(spf_output), Some(sender)) = (spf_output.as_ref(), mail_from) {
        let domain = sender.split('@').nth(1).unwrap_or(sender);
        let dmarc = resolver
            .verify_dmarc(mail_auth::dmarc::verify::DmarcParameters::new(
                &auth_msg,
                &dkim_outputs,
                domain,
                spf_output,
            ))
            .await;
        results.dmarc = if matches!(dmarc.dkim_result(), mail_auth::DmarcResult::Pass)
            || matches!(dmarc.spf_result(), mail_auth::DmarcResult::Pass)
        {
            AuthVerdict::Pass
        } else {
            AuthVerdict::Fail
        };
    }
    results
}

/// SPF/DKIM/DMARC verdicts from the local DNS verifier.
#[derive(Default)]
pub(crate) struct AuthResults {
    pub(crate) spf: AuthVerdict,
    pub(crate) dkim: AuthVerdict,
    pub(crate) dmarc: AuthVerdict,
}

fn extract_command_address(re: &regex::Regex, arg: &str) -> Option<String> {
    let cap = re.captures(arg)?;
    let raw = cap
        .get(1)
        .or_else(|| cap.get(2))
        .map(|m| m.as_str())
        .unwrap_or_default();
    Some(extract_email(raw))
}

/// Map an ingest rejection reason onto the connection metric it should be counted as.
fn ingest_status(ingest: &crate::use_cases::thread::InboundIngestResult) -> SmtpStatus {
    if ingest.accepted {
        return SmtpStatus::Accepted;
    }
    match ingest.reason.as_deref() {
        // The server answered a reserved `_` address itself. Nothing was routed, but nothing went
        // wrong either, so this must not be counted as an SMTP error.
        //
        // Matching on the reason *string* is the coupling `src/AGENTS.md` warns about, and several
        // arms below already miscategorize ("Company or Channel not found" lands on `Error`).
        // Turning `InboundIngestResult::reason` into an enum is the real fix, and is not this
        // change's job.
        Some(crate::use_cases::thread::SYSTEM_ADDRESS_ANSWERED) => SmtpStatus::Accepted,
        Some("DMARC authentication did not pass") => SmtpStatus::RejectedDmarc,
        Some("Spam score threshold exceeded") => SmtpStatus::RejectedSpamScore,
        Some(r) if r.contains("rate limit") => SmtpStatus::BlockedRateLimit,
        Some(r) if r.contains("DNSBL") => SmtpStatus::BlockedDnsbl,
        _ => SmtpStatus::Error,
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
    spf_status: AuthVerdict,
    dkim_status: AuthVerdict,
    dmarc_status: AuthVerdict,
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
                if let Some(val_str) = header.value().as_text() {
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
                stored_key: None,
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
    use crate::entities::task::NewTask;

    #[tokio::test]
    async fn smtp_line_limits_accept_boundary_and_reject_one_over() {
        let mut at_limit = vec![b'a'; MAX_COMMAND_LINE_BYTES];
        at_limit[MAX_COMMAND_LINE_BYTES - 1] = b'\n';
        let mut reader = BufReader::new(at_limit.as_slice());
        assert_eq!(
            read_limited_line(&mut reader, MAX_COMMAND_LINE_BYTES)
                .await
                .unwrap()
                .len(),
            MAX_COMMAND_LINE_BYTES
        );

        let over_limit = vec![b'a'; MAX_COMMAND_LINE_BYTES + 1];
        let mut reader = BufReader::new(over_limit.as_slice());
        assert!(matches!(
            read_limited_line(&mut reader, MAX_COMMAND_LINE_BYTES).await,
            Err(LineReadError::TooLong)
        ));
    }

    #[test]
    fn declared_size_and_streamed_data_share_the_twenty_mib_limit() {
        assert_eq!(
            declared_message_size("FROM:<sender@example.com> SIZE=20971520"),
            Some(MAX_INBOUND_MESSAGE_BYTES)
        );
        assert_eq!(
            declared_message_size("FROM:<sender@example.com> size=20971521"),
            Some(MAX_INBOUND_MESSAGE_BYTES + 1)
        );

        let mut session = SmtpSession::default();
        session.data_buffer = vec![b'x'; MAX_INBOUND_MESSAGE_BYTES - 1];
        assert!(session.push_data_line(b"x"));
        assert!(!session.push_data_line(b"y"));
    }
    use async_trait::async_trait;
    use chrono::Utc;
    use mail_auth::common::verify::VerifySignature;
    use std::sync::Mutex;
    use uuid::Uuid;

    use crate::{
        app_error::AppResult,
        entities::{
            channel::Channel,
            company::Company,
            cursor::{MessageCursor, ThreadCursor},
            message::Message,
            thread::Thread,
        },
        use_cases::{
            channel::{ChannelPersistence, ChannelWrite},
            company::{CompanyPersistence, CompanyWrite},
            thread::ThreadPersistence,
        },
    };

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    #[async_trait]
    impl CompanyPersistence for MockCompanyPersistence {
        async fn create(&self, _user_id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
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
        async fn update(&self, _id: Uuid, _write: CompanyWrite) -> AppResult<Company> {
            unimplemented!()
        }
        async fn delete(&self, _id: Uuid) -> AppResult<()> {
            unimplemented!()
        }
        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }

        /// This double never reaches a provider call: the tests around it assert on ingestion and
        /// threading, and an agent run that gets this far fails at parameter resolution by design.
        async fn list_model_connections(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyModelConnection>> {
            Ok(Vec::new())
        }

        async fn model_api_key(
            &self,
            _company_id: Uuid,
            _provider: &crate::entities::value_objects::ModelProvider,
        ) -> AppResult<Option<String>> {
            Ok(None)
        }

        async fn replace_model_connections_for_user(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
            _connections: Vec<crate::use_cases::company::CompanyModelConnectionWrite>,
        ) -> AppResult<()> {
            unimplemented!("this double is not exercised on the model-connection write path")
        }
    }

    struct MockChannelPersistence {
        channels: Mutex<Vec<Channel>>,
    }

    #[async_trait]
    impl ChannelPersistence for MockChannelPersistence {
        async fn create(&self, _company_id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
            unimplemented!()
        }
        async fn get_by_id(&self, _id: Uuid) -> AppResult<Option<Channel>> {
            unimplemented!()
        }
        async fn get_by_company_slug_and_channel_slug(
            &self,
            _company_slug: &crate::entities::value_objects::CompanySlug,
            channel_slug: &crate::entities::value_objects::ChannelSlug,
        ) -> AppResult<Option<Channel>> {
            Ok(self
                .channels
                .lock()
                .unwrap()
                .iter()
                .find(|w| w.matches_slug(channel_slug))
                .cloned())
        }
        async fn list_by_company_id(&self, _company_id: Uuid) -> AppResult<Vec<Channel>> {
            Ok(self.channels.lock().unwrap().clone())
        }
        async fn update(&self, _id: Uuid, _write: ChannelWrite) -> AppResult<Channel> {
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
            participant_emails: &[crate::entities::value_objects::EmailAddress],
        ) -> AppResult<Thread> {
            let thread = Thread {
                id: Uuid::new_v4(),
                channel_id,
                subject: subject.to_string(),
                participant_emails: participant_emails.to_vec(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
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

        async fn list_threads_by_channel_id(
            &self,
            _channel_id: Uuid,
            _before: Option<ThreadCursor>,
            _limit: usize,
        ) -> AppResult<Vec<Thread>> {
            unimplemented!()
        }

        async fn list_threads_updated_after(
            &self,
            channel_id: Uuid,
            after: Option<ThreadCursor>,
            limit: usize,
        ) -> AppResult<Vec<Thread>> {
            let mut threads: Vec<Thread> = self
                .threads
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.channel_id == channel_id)
                .filter(|t| after.is_none_or(|cursor| t.cursor() > cursor))
                .cloned()
                .collect();
            threads.sort_by_key(|t| t.cursor());
            threads.truncate(limit);
            Ok(threads)
        }

        async fn update_thread_participants(
            &self,
            id: Uuid,
            participant_emails: &[crate::entities::value_objects::EmailAddress],
        ) -> AppResult<Thread> {
            let mut list = self.threads.lock().unwrap();
            let thread = list.iter_mut().find(|t| t.id == id).unwrap();
            thread.participant_emails = participant_emails.to_vec();
            Ok(thread.clone())
        }

        async fn find_thread_by_message_ids(
            &self,
            _channel_id: Uuid,
            message_ids: &[crate::entities::value_objects::MessageId],
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
            _channel_id: Uuid,
            thread_index_prefix: &crate::entities::value_objects::ThreadIndex,
        ) -> AppResult<Option<Thread>> {
            let thread_id = {
                let msgs = self.messages.lock().unwrap();
                msgs.iter()
                    .find(|m| {
                        m.thread_index
                            .as_deref()
                            .unwrap_or_default()
                            .starts_with(thread_index_prefix.as_str())
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

        async fn get_message_by_message_id(
            &self,
            _company_id: Uuid,
            message_id: &crate::entities::value_objects::MessageId,
        ) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|m| &m.message_id == message_id)
                .cloned())
        }

        async fn find_outbound_reply(
            &self,
            thread_id: Uuid,
            in_reply_to: &crate::entities::value_objects::MessageId,
        ) -> AppResult<Option<Message>> {
            Ok(self
                .messages
                .lock()
                .unwrap()
                .iter()
                .find(|message| {
                    message.thread_id == thread_id
                        && message.direction == crate::entities::message::MessageDirection::Outbound
                        && message.in_reply_to.as_ref() == Some(in_reply_to)
                })
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

        async fn list_messages_after(
            &self,
            thread_id: Uuid,
            after: Option<MessageCursor>,
            limit: usize,
        ) -> AppResult<Vec<Message>> {
            let mut messages: Vec<Message> = self
                .messages
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.thread_id == thread_id)
                .filter(|m| after.is_none_or(|cursor| m.cursor() > cursor))
                .cloned()
                .collect();
            messages.sort_by_key(|m| m.cursor());
            messages.truncate(limit);
            Ok(messages)
        }
    }

    struct MockTaskPersistence;

    use crate::adapters::persistence::task::{AgentDispatchCommit, DispatchCommit};
    use crate::entities::task::TaskLeaseRef;

    #[async_trait]
    impl crate::adapters::persistence::task::TaskPersistence for MockTaskPersistence {
        async fn commit_agent_dispatch(
            &self,
            commit: AgentDispatchCommit<'_>,
        ) -> AppResult<DispatchCommit> {
            let _ = commit;
            Ok(DispatchCommit::Committed { outbox_id: None })
        }

        async fn renew_task_lease(
            &self,
            _lease: TaskLeaseRef,
            _lock_expires_at: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<bool> {
            Ok(true)
        }
        async fn enqueue_task(
            &self,
            NewTask {
                company_id,
                channel_id,
                thread_id,
                task_type,
                payload,
                correlation_id,
            }: NewTask,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            Ok(crate::entities::task::BackgroundTask {
                id: Uuid::new_v4(),
                company_id,
                channel_id,
                thread_id,
                correlation_id,
                task_type,
                status: crate::entities::task::TaskStatus::Pending,
                payload,
                retry_count: 0,
                max_retries: 3,
                last_error: None,
                worker_id: None,
                execution_generation: None,
                locked_at: None,
                lock_expires_at: None,
                run_at: Utc::now(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
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
        async fn claim_pending_tasks(
            &self,
            _worker_id: Uuid,
            _lock_expires_at: chrono::DateTime<chrono::Utc>,
            _limit: i64,
        ) -> AppResult<Vec<crate::entities::task::BackgroundTask>> {
            Ok(vec![])
        }
        async fn claim_task(
            &self,
            _id: Uuid,
            _worker_id: Uuid,
            _lock_expires_at: chrono::DateTime<chrono::Utc>,
        ) -> AppResult<bool> {
            Ok(true)
        }
        async fn mark_task_completed(&self, _lease: TaskLeaseRef) -> AppResult<bool> {
            Ok(true)
        }
        async fn mark_task_failed(
            &self,
            _lease: TaskLeaseRef,
            _error_msg: &str,
            _next_run_at: chrono::DateTime<chrono::Utc>,
            _is_dead_letter: bool,
        ) -> AppResult<bool> {
            Ok(true)
        }
        async fn stop_task(&self, _id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn resume_task(&self, _id: Uuid) -> AppResult<crate::entities::task::BackgroundTask> {
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
            AuthVerdict::Unknown,
            AuthVerdict::Unknown,
            AuthVerdict::Unknown,
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
        assert_eq!(payload.spf, AuthVerdict::Unknown);
        assert_eq!(payload.dkim, AuthVerdict::Unknown);
    }

    #[tokio::test]
    async fn submitted_authentication_headers_cannot_authorize_smtp_mail() {
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Acme Corp".to_string(),
                slug: "acme".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                enabled: true,
                add_3rd_party: true,
                id: Uuid::new_v4(),
                company_id,
                name: "Inbound Flow".to_string(),
                description: None,
                slug: "inbound".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                agent_ids: None,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let task_persistence = Arc::new(MockTaskPersistence);

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
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
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
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
        assert!(
            response.starts_with("550 5.7.1"),
            "unexpected SMTP response: {response:?}"
        );
        assert!(response.contains("DMARC authentication did not pass"));

        // QUIT
        response.clear();
        writer.write_all(b"QUIT\r\n").await.unwrap();
        writer.flush().await.unwrap();
        buf_reader.read_line(&mut response).await.unwrap();
        assert!(response.contains("221"));

        // The DNS verifier result is authoritative; the submitted pass headers are inert.
        let threads = thread_persistence.threads.lock().unwrap();
        assert!(threads.is_empty());
        let messages = thread_persistence.messages.lock().unwrap();
        assert!(messages.is_empty());
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
    async fn stale_dkim_and_submitted_auth_headers_do_not_authorize_smtp_mail() {
        let company_id = Uuid::new_v4();
        let company_persistence = Arc::new(MockCompanyPersistence {
            companies: Mutex::new(vec![Company {
                id: company_id,
                user_id: Uuid::new_v4(),
                name: "Populus Network".to_string(),
                slug: "populus".into(),
                enable_llm_spam_guardrail: None,
                avatar_url: None,
                memory_provider: None,
                created_at: Utc::now(),
            }]),
        });

        let channel_persistence = Arc::new(MockChannelPersistence {
            channels: Mutex::new(vec![Channel {
                enabled: true,
                add_3rd_party: true,
                id: Uuid::new_v4(),
                company_id,
                name: "Reg Channel".to_string(),
                description: None,
                slug: "network".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                agent_ids: None,
                retrieve_company_memory: false,
                retrieve_agent_memory: false,
                retrieve_user_memory: false,
                persist_company_memory: false,
                persist_agent_memory: false,
                persist_user_memory: false,
                created_by: crate::entities::creation::CreationProvenance::system(),
                created_at: Utc::now(),
            }]),
        });

        let thread_persistence = Arc::new(MockThreadPersistence {
            threads: Mutex::new(Vec::new()),
            messages: Mutex::new(Vec::new()),
        });

        let task_persistence = Arc::new(MockTaskPersistence);

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
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
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
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
        assert!(
            response.starts_with("550 5.7.1"),
            "unexpected SMTP response: {response:?}"
        );
        assert!(response.contains("DMARC authentication did not pass"));

        writer.write_all(b"QUIT\r\n").await.unwrap();
        writer.flush().await.unwrap();

        let messages = thread_persistence.messages.lock().unwrap();
        assert!(messages.is_empty());
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
            sendgrid_inbound: None,
            hydradb: None,
            hindsight: None,
            refresh_token_ttl: time::Duration::days(30),
            app_domain_name: "mailagents.com".to_string(),
            cors_allowed_origins: vec![],
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
            secure_cookies: false,
            gcs: None,
            operator_emails: Vec::new(),
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
