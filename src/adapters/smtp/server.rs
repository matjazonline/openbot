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
    adapters::{
        protocols::email::{
            EmailChannelSelectorParser, EmailIdentity, EmailIngressAdapter, EmailIngressTrust,
            VerifiedEmailAuth, parse_raw_mime_to_payload,
            parser::{MAX_INBOUND_MESSAGE_BYTES, RawInboundPayload, extract_email},
            verify_email_authentication,
        },
        storage::FileStorage,
    },
    application::use_cases::thread::{
        InboundIngestResult, InboundPreflight, IngestRejection, IngressOrigin, ReplyDelivery,
        ThreadUseCases,
    },
    domain::monitoring::{MonitoringService, SmtpConnectionMetrics, SmtpStatus},
    infra::config::AppConfig,
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
        if let Ok(mut lock) = self.conns.write()
            && let Some(count) = lock.get_mut(&self.ip)
        {
            if *count > 1 {
                *count -= 1;
            } else {
                lock.remove(&self.ip);
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
        if let Ok(mut addrs) = tokio::net::lookup_host((lookup_host.as_str(), 80)).await
            && addrs.next().is_some()
        {
            return Some(server.clone());
        }
    }

    None
}

pub struct SmtpServer {
    thread_use_cases: Arc<ThreadUseCases>,
    config: Arc<AppConfig>,
    /// Parses what arrives on the wire. Held rather than built per message: the app domain is
    /// deployment configuration read once, and the parser is the only thing on this path that
    /// understands the platform's address grammar.
    ingress: EmailIngressAdapter,
    /// Where inbound attachments are kept; `None` on a deployment with no private bucket.
    file_storage: Option<Arc<dyn FileStorage>>,
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
            ingress: EmailIngressAdapter::for_config(&config),
            config,
            file_storage: None,
            monitoring: None,
            active_conns: Arc::new(RwLock::new(HashMap::new())),
            connection_slots: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        }
    }

    pub fn with_monitoring(mut self, monitoring: Arc<dyn MonitoringService>) -> Self {
        self.monitoring = Some(monitoring);
        self
    }

    /// Where attachments on arriving mail are stored. Without it mail still arrives; its
    /// attachments are recorded but their bytes are not kept.
    pub fn with_file_storage(mut self, file_storage: Option<Arc<dyn FileStorage>>) -> Self {
        self.file_storage = file_storage;
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
                                &session,
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

        // The verdicts are this boundary's, established above from the connection and the
        // signatures. Nothing in the message itself can claim them.
        let trust = EmailIngressTrust::Verified(VerifiedEmailAuth {
            spf: raw_payload.spf,
            dkim: raw_payload.dkim,
            dmarc: raw_payload.dmarc,
            spam_score: raw_payload.spam_score,
        });
        let accepted = self.ingress.accept(raw_payload, trust);
        let ingest_result = match accepted {
            Ok(accepted) => {
                let (inbound, attachments) = accepted
                    .into_preflight_parts(IngressOrigin::ExternalTransport, ReplyDelivery::Send);
                match self.thread_use_cases.preflight_inbound(inbound).await? {
                    InboundPreflight::Rejected(result) => Ok(*result),
                    InboundPreflight::Accepted(mut prepared) => {
                        let persisted = attachments
                            .persist(&self.config, self.file_storage.as_deref())
                            .await?;
                        prepared.replace_attachments(
                            persisted.metadata,
                            persisted.stored_count,
                            persisted.failed_count,
                        );
                        self.thread_use_cases
                            .commit_prepared_inbound(*prepared)
                            .await
                    }
                }
            }
            // A message this adapter could not turn into a canonical one will not parse on a
            // retry either, so it is a permanent rejection rather than a transient failure.
            Err(error) => Err(error.into()),
        };
        match ingest_result {
            Ok(ingest) => {
                let answer = SmtpAnswer::for_ingest(&ingest);
                self.record_connection(
                    client_ip,
                    answer.status,
                    start_time,
                    session.mailfrom.clone(),
                    session.rcpts.first().cloned(),
                );
                writer.write_all(answer.line().as_bytes()).await?;
            }
            Err(error) => {
                let answer = SmtpAnswer::for_error(&error);
                warn!(%error, permanent = answer.is_permanent(), "Could not ingest an SMTP message");
                self.record_connection(
                    client_ip,
                    answer.status,
                    start_time,
                    session.mailfrom.clone(),
                    session.rcpts.first().cloned(),
                );
                writer.write_all(answer.line().as_bytes()).await?;
            }
        }
        writer.flush().await?;
        Ok(())
    }

    /// Spam scanning is for strangers: a sender the destination channel already trusts skips it.
    async fn sender_needs_spam_scan(&self, raw_payload: &RawInboundPayload) -> bool {
        let Some(selection) =
            EmailChannelSelectorParser::new(&self.config.app_domain_name).parse(&raw_payload.to)
        else {
            return true;
        };
        let Some(selector) = selection.selectors().first() else {
            return true;
        };
        let Some(company_slug) = selector.company() else {
            return true;
        };
        let channel_slug = selector.channel();
        let Ok(Some(company)) = self
            .thread_use_cases
            .company_persistence()
            .get_by_slug(company_slug)
            .await
        else {
            return true;
        };
        let Ok(Some(channel)) = self
            .thread_use_cases
            .channel_persistence()
            .get_by_company_slug_and_channel_slug(company_slug, channel_slug)
            .await
        else {
            return true;
        };

        let Ok(sender) = EmailIdentity::parse(crate::entities::value_objects::EmailAddress::from(
            extract_email(&raw_payload.from),
        ))
        .map(EmailIdentity::qualify_default) else {
            return true;
        };
        let context = match self
            .thread_use_cases
            .participant_persistence()
            .access_context_for_identity(company.id, &sender)
            .await
        {
            Ok(context) => context,
            Err(error) => {
                // Every other bail-out here scans too: a directory lookup that failed must not be
                // the reason a stranger is promoted to a trusted sender.
                warn!(%error, "Principal resolution failed; scanning sender as a stranger");
                return true;
            }
        };
        !channel.participant_access(context).trusted
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

fn extract_command_address(re: &regex::Regex, arg: &str) -> Option<String> {
    let cap = re.captures(arg)?;
    let raw = cap
        .get(1)
        .or_else(|| cap.get(2))
        .map(|m| m.as_str())
        .unwrap_or_default();
    Some(extract_email(raw))
}

/// What this session answers, and what the answer is counted as.
///
/// The three-way split is the point. A message this platform *decided* not to route is accepted
/// and dropped, because a 5xx would make the sending server retry or bounce -- and for an
/// auto-reply or a thread already ping-ponging, both make the loop worse. A message that was
/// refused on its merits gets a permanent 5xx so the sender learns. A message that failed for a
/// reason of ours gets a transient 4xx so the sending server keeps it: answering 250 there would
/// accept a message nothing had stored.
struct SmtpAnswer {
    code: &'static str,
    detail: String,
    status: SmtpStatus,
}

impl SmtpAnswer {
    fn accepted() -> Self {
        Self {
            code: "250 2.0.0",
            detail: "Message queued for delivery".into(),
            status: SmtpStatus::Accepted,
        }
    }

    fn for_ingest(ingest: &InboundIngestResult) -> Self {
        let Some(rejection) = ingest.rejection.as_ref() else {
            return Self::accepted();
        };
        let status = match rejection {
            IngestRejection::AuthenticationFailed => SmtpStatus::RejectedDmarc,
            IngestRejection::SpamScore => SmtpStatus::RejectedSpamScore,
            IngestRejection::SystemAddressAnswered
            | IngestRejection::AutoReply
            | IngestRejection::ThreadTurnLimit => SmtpStatus::Accepted,
            _ => SmtpStatus::Error,
        };
        match rejection {
            // Decided, not failed: the message reached the platform and the platform chose not to
            // route it. Telling the sender's server to retry would be a lie, and telling it to
            // bounce would answer a machine with another machine.
            IngestRejection::SystemAddressAnswered
            | IngestRejection::AutoReply
            | IngestRejection::ThreadTurnLimit => Self {
                status,
                ..Self::accepted()
            },
            _ => Self {
                code: "550 5.7.1",
                detail: format!("Message rejected ({rejection})"),
                status,
            },
        }
    }

    /// A failure of ours, or of the message's own syntax.
    ///
    /// Only malformed input is permanent. Everything else -- a database outage above all -- is
    /// transient, because the message is still in the sending server's queue and will be offered
    /// again; a 250 here would be an acknowledgement of something nothing had written.
    fn for_error(error: &crate::app_error::AppError) -> Self {
        match error {
            crate::app_error::AppError::BadRequest(detail) => Self {
                code: "550 5.6.0",
                detail: format!("Message rejected ({detail})"),
                status: SmtpStatus::Error,
            },
            _ => Self {
                code: "451 4.3.0",
                detail: "Local error in processing".into(),
                status: SmtpStatus::Error,
            },
        }
    }

    fn is_permanent(&self) -> bool {
        self.code.starts_with('5')
    }

    fn line(&self) -> String {
        format!("{} {}\r\n", self.code, self.detail)
    }
}

#[cfg(test)]
mod answer_tests {
    use super::*;
    use crate::app_error::AppError;
    use crate::use_cases::thread::{BounceInfo, InboundIngestResult, IngestRejection};

    fn rejected(rejection: IngestRejection) -> InboundIngestResult {
        InboundIngestResult::rejected(rejection)
    }

    fn accepted() -> InboundIngestResult {
        let mut result = InboundIngestResult::rejected(IngestRejection::AutoReply);
        result.accepted = true;
        result.rejection = None;
        result
    }

    fn bounce() -> BounceInfo {
        BounceInfo {
            source_message_key: crate::entities::transport::ExternalMessageKey::parse(
                "<rejected@example.com>",
            )
            .unwrap(),
            recipient_to: "sender@example.com".into(),
            company_slug: None,
            invalid_slugs: Vec::new(),
            disabled_slugs: Vec::new(),
            suggestions: Vec::new(),
            available_channels: Vec::new(),
            original_subject: "Hello".to_string(),
        }
    }

    #[test]
    fn an_accepted_message_is_queued() {
        let answer = SmtpAnswer::for_ingest(&accepted());
        assert!(answer.line().starts_with("250 "));
        assert_eq!(answer.status, SmtpStatus::Accepted);
    }

    /// The three the platform *decided* not to route. A 5xx would make the sending server retry
    /// or bounce, and for a machine-generated message or a thread already ping-ponging, both make
    /// the loop worse -- which is the loop these rejections exist to stop.
    #[test]
    fn a_message_the_platform_chose_not_to_route_is_accepted_and_dropped() {
        for rejection in [
            IngestRejection::SystemAddressAnswered,
            IngestRejection::AutoReply,
            IngestRejection::ThreadTurnLimit,
        ] {
            let answer = SmtpAnswer::for_ingest(&rejected(rejection.clone()));
            assert!(
                answer.line().starts_with("250 "),
                "{rejection:?} answered {}",
                answer.line().trim()
            );
            assert_eq!(answer.status, SmtpStatus::Accepted, "{rejection:?}");
        }
    }

    /// Everything refused on its merits is permanent, and the two the connection metric
    /// distinguishes are counted as themselves rather than as a generic error.
    #[test]
    fn a_message_refused_on_its_merits_is_a_permanent_rejection() {
        for (rejection, status) in [
            (
                IngestRejection::AuthenticationFailed,
                SmtpStatus::RejectedDmarc,
            ),
            (IngestRejection::SpamScore, SmtpStatus::RejectedSpamScore),
            (IngestRejection::UnknownRecipient, SmtpStatus::Error),
            (IngestRejection::Unauthorized, SmtpStatus::Error),
            (IngestRejection::HopLimitReached, SmtpStatus::Error),
            (
                IngestRejection::ThreadInjection(Box::new(bounce())),
                SmtpStatus::Error,
            ),
        ] {
            let answer = SmtpAnswer::for_ingest(&rejected(rejection.clone()));
            assert!(answer.is_permanent(), "{rejection:?}");
            assert!(answer.line().contains(rejection.as_str()), "{rejection:?}");
            assert_eq!(answer.status, status, "{rejection:?}");
        }
    }

    /// The case a synchronous transport must not get wrong. The message is still in the sending
    /// server's queue; answering 250 for a database outage accepts something nothing stored.
    #[test]
    fn a_failure_of_ours_is_transient_and_a_malformed_message_is_not() {
        let outage = SmtpAnswer::for_error(&AppError::Internal("connection refused".into()));
        assert!(
            outage.line().starts_with("451 "),
            "{}",
            outage.line().trim()
        );
        assert!(!outage.is_permanent());

        let malformed = SmtpAnswer::for_error(&AppError::BadRequest("unusable sender".into()));
        assert!(malformed.line().starts_with("550 "));
        assert!(malformed.is_permanent());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::task::NewTask;
    use crate::use_cases::thread::test_support::InMemoryThreads;

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

        let mut session = SmtpSession {
            data_buffer: vec![b'x'; MAX_INBOUND_MESSAGE_BYTES - 1],
            ..Default::default()
        };
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
            company::{Company, CompanyAccess},
            company_member::CompanyMembership,
        },
        use_cases::{
            channel::{ChannelPersistence, ChannelWrite},
            company::{CompanyPersistence, CompanyWrite},
            participant::test_support::{InMemoryParticipantDirectory, TeamFixture},
        },
    };

    struct MockCompanyPersistence {
        companies: Mutex<Vec<Company>>,
    }

    /// These tests are about SMTP, not team management: every sender is a colleague.
    #[async_trait]
    impl TeamFixture for MockCompanyPersistence {
        async fn membership_for_email(
            &self,
            _company_id: Uuid,
            _email: &str,
        ) -> AppResult<CompanyMembership> {
            Ok(CompanyMembership::Member)
        }

        async fn company_access(
            &self,
            _user_id: Uuid,
            _company_id: Uuid,
        ) -> AppResult<Option<CompanyAccess>> {
            unimplemented!("this double is not exercised on the signed-in path")
        }
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
        async fn list_company_team_emails(&self, _company_id: Uuid) -> AppResult<Vec<String>> {
            Ok(vec![])
        }

        async fn list_company_team_accounts(
            &self,
            _company_id: Uuid,
        ) -> AppResult<Vec<crate::entities::company::CompanyTeamAccount>> {
            unimplemented!("this double is not exercised on the team-account path")
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

    struct MockTaskPersistence;

    use crate::entities::task::{ResumeActor, StopActor, TaskFailure, TaskLeaseRef};
    use crate::task_queue::{AgentDispatchCommit, DispatchCommit};

    #[async_trait]
    impl crate::task_queue::TaskPersistence for MockTaskPersistence {
        /// No fixture here sends an outreach, so nothing ever asks one to be recorded.
        async fn record_outreach_request_message(
            &self,
            _delivery_id: crate::entities::transport::DeliveryId,
            _write: &crate::use_cases::thread::MessageWrite,
        ) -> AppResult<crate::entities::message::CanonicalMessageId> {
            unreachable!("no fixture here sends an outreach")
        }

        /// This fixture enqueues no task, so a run that asked for targets would be a bug in the
        /// test rather than an empty conversation.
        async fn list_task_channel_targets(
            &self,
            _company_id: Uuid,
            _task_id: Uuid,
        ) -> AppResult<Vec<crate::use_cases::thread::TaskChannelTarget>> {
            Ok(Vec::new())
        }

        async fn commit_agent_dispatch(
            &self,
            commit: AgentDispatchCommit<'_>,
        ) -> AppResult<DispatchCommit> {
            let _ = commit;
            Ok(DispatchCommit::Committed {
                deliveries: Vec::new(),
            })
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
                targets: _,
                task_type,
                payload,
                source: _,
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
        async fn mark_task_failed(&self, _failure: TaskFailure<'_>) -> AppResult<bool> {
            Ok(true)
        }
        async fn stop_task(
            &self,
            _id: Uuid,
            _actor: StopActor,
        ) -> AppResult<crate::entities::task::BackgroundTask> {
            unimplemented!()
        }
        async fn resume_task(
            &self,
            _id: Uuid,
            _actor: ResumeActor,
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
        use crate::entities::auth::AuthVerdict;
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
                channel_defaults: Default::default(),
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
                owner_agent_id: None,
                enabled: true,
                add_3rd_party: true,
                id: Uuid::new_v4(),
                company_id,
                name: "Inbound Flow".to_string(),
                description: None,
                slug: "inbound".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: crate::entities::channel::ChannelAccessMode::Team,
                principal_grants: Vec::new(),
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

        let thread_persistence = Arc::new(InMemoryThreads::new());

        let task_persistence = Arc::new(MockTaskPersistence);

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            resend_inbound: None,
            resend_outbound: None,
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

        let thread_use_cases = Arc::new(ThreadUseCases::for_test(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence.clone(),
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
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
        let threads = thread_persistence.threads();
        assert!(threads.is_empty());
        let messages = thread_persistence.messages();
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
                channel_defaults: Default::default(),
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
                owner_agent_id: None,
                enabled: true,
                add_3rd_party: true,
                id: Uuid::new_v4(),
                company_id,
                name: "Reg Channel".to_string(),
                description: None,
                slug: "network".into(),
                alias_slugs: Vec::new(),
                participant_emails: None,
                access_mode: crate::entities::channel::ChannelAccessMode::Team,
                principal_grants: Vec::new(),
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

        let thread_persistence = Arc::new(InMemoryThreads::new());

        let task_persistence = Arc::new(MockTaskPersistence);

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            resend_inbound: None,
            resend_outbound: None,
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

        let thread_use_cases = Arc::new(ThreadUseCases::for_test(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence.clone(),
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
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

        let messages = thread_persistence.messages();
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
        let thread_persistence = Arc::new(InMemoryThreads::new());
        let task_persistence = Arc::new(MockTaskPersistence);

        let config = Arc::new(AppConfig {
            jwt_secret: "secret".to_string(),
            sendgrid_inbound: None,
            resend_inbound: None,
            resend_outbound: None,
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

        let thread_use_cases = Arc::new(ThreadUseCases::for_test(
            thread_persistence.clone(),
            channel_persistence,
            company_persistence.clone(),
            Arc::new(InMemoryParticipantDirectory::new().with_team(company_persistence)),
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
