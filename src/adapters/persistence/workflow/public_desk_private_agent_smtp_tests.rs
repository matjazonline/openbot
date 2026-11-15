//! Real TCP SMTP and DNS-backed authentication into the production workflow runtime. The
//! external spam service is replaced by a score; its policy consumer remains real.
use super::*;
use crate::adapters::protocols::email::test_support::dns::LocalEmailDns;
use crate::application::services::spam_scanner::{SpamScanInput, SpamScanResult, SpamScanner};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, tcp::OwnedReadHalf},
};

pub(super) struct Score(pub(super) f64);
#[async_trait::async_trait]
impl SpamScanner for Score {
    async fn scan(&self, _: SpamScanInput<'_>) -> SpamScanResult {
        let mut result = SpamScanResult::new();
        result.add(self.0, "fixture external spam score");
        result
    }
}

struct Mail {
    sender: EmailAddress,
    recipient: EmailAddress,
    message: Uuid,
    score: f64,
    cc: Option<EmailAddress>,
}

impl Mail {
    fn to(recipient: EmailAddress) -> Self {
        Self {
            sender: CUSTOMER.into(),
            recipient,
            message: Uuid::new_v4(),
            score: 0.0,
            cc: None,
        }
    }
}

async fn response(reader: &mut BufReader<OwnedReadHalf>) -> String {
    let mut received = String::new();
    loop {
        let mut line = String::new();
        assert!(
            reader.read_line(&mut line).await.unwrap() > 0,
            "SMTP peer closed before its reply"
        );
        let complete = line.as_bytes().get(3) == Some(&b' ');
        received.push_str(&line);
        assert!(received.len() <= 4096, "bounded fixture SMTP response");
        if complete {
            return received;
        }
    }
}

async fn submit(d: &Deployment, dns: &LocalEmailDns, mail: Mail) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = Arc::new(AppConfig {
        app_domain_name: APP_DOMAIN.into(),
        ..AppConfig::for_test()
    });
    let threads =
        Arc::new(crate::adapters::persistence::test_support::thread_use_cases(d.p(), APP_DOMAIN));
    let server = crate::adapters::smtp::server::SmtpServer::new(
        threads,
        config,
        Arc::new(Score(mail.score)),
    )
    .with_resolver_for_test(dns.authenticator());
    let serve = async {
        let (socket, peer) = listener.accept().await.unwrap();
        server.handle_connection(socket, peer).await.unwrap();
    };
    let client = async {
        let socket = TcpStream::connect(address).await.unwrap();
        let (read, mut write) = socket.into_split();
        let mut read = BufReader::new(read);
        assert!(response(&mut read).await.starts_with("220 "));
        for command in [
            "EHLO sender.partner.test\r\n".to_owned(),
            format!("MAIL FROM:<{}>\r\n", mail.sender),
            format!("RCPT TO:<{}>\r\n", mail.recipient),
        ] {
            write.write_all(command.as_bytes()).await.unwrap();
            assert!(response(&mut read).await.starts_with("250"));
        }
        write.write_all(b"DATA\r\n").await.unwrap();
        assert!(response(&mut read).await.starts_with("354 "));
        let cc = mail
            .cc
            .as_ref()
            .map(|address| format!("Cc: {address}\r\n"))
            .unwrap_or_default();
        let wire = format!(
            "From: {}\r\nTo: {}\r\n{cc}Message-ID: <{}@sender.test>\r\nSubject: Actual SMTP desk mail\r\nAuthentication-Results: attacker.test; spf=pass; dmarc=pass\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nPlease answer this message.\r\n.\r\n",
            mail.sender, mail.recipient, mail.message
        );
        write.write_all(wire.as_bytes()).await.unwrap();
        let answer = response(&mut read).await;
        write.write_all(b"QUIT\r\n").await.unwrap();
        assert!(response(&mut read).await.starts_with("221 "));
        answer
    };
    let (answer, ()) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(client, serve)
    })
    .await
    .expect("actual SMTP session completes within its fixture budget");
    answer
}

async fn rejections(d: &Deployment, dns: &LocalEmailDns, endpoint: &PrincipalEndpoint) {
    let direct = Mail::to(endpoint.address.clone());
    let mut forged = Mail::to(d.address("support").into());
    forged.sender = "stranger@blocked.test".into();
    let mut spam = Mail::to(d.address("support").into());
    spam.score = 99.0;
    for mail in [direct, forged, spam] {
        let answer = submit(d, dns, mail).await;
        assert!(answer.starts_with("550 "), "{answer}");
    }
    assert!(dns.queried("blocked.test."));
    assert!(dns.queried("_dmarc.blocked.test."));
    assert_eq!(
        d.count("SELECT count(*) FROM messages WHERE company_id=$1 AND direction='inbound'")
            .await,
        1
    );
}

pub(in super::super::super) async fn receive_member_mail(
    d: &Deployment,
    sender: EmailAddress,
    cc: Option<EmailAddress>,
) -> Uuid {
    let mut mail = Mail::to(d.address("support").into());
    mail.sender = sender;
    mail.cc = cc;
    let dns = LocalEmailDns::start().await;
    let answer = submit(d, &dns, mail).await;
    dns.stop().await;
    assert!(answer.starts_with("250 "), "{answer}");
    only_desk_thread(d).await
}

#[tokio::test]
async fn public_desk_private_agent_smtp_uses_real_dns_authentication_spam_policy_and_desk_reply() {
    let d = Box::pin(Deployment::new(vec![LlmTurn::text(
        r#"{"body":"SMTP desk answer"}"#,
    )]))
    .await;
    let endpoint = private_endpoint(&d).await;
    open_desk(&d).await;
    let mut selected = example(0);
    model(&mut selected);
    Box::pin(bind(&d, selected)).await;
    let dns = LocalEmailDns::start().await;
    let answer = submit(&d, &dns, Mail::to(d.address("support").into())).await;
    assert!(answer.starts_with("250 "), "{answer}");
    let delivery = Delivery::start(&d, ProviderCase::Accept);
    Box::pin(
        d.run_until("SMTP public desk reply by private agent", async || {
            responses_published(&d, 1).await && delivered(&d, 1).await
        }),
    )
    .await;
    let provider = delivery.stop().await;
    let rows = finals(&d).await;
    assert_eq!(rows.len(), 1);
    assert_desk_publication(&d, rows[0].source, rows[0].thread, &endpoint).await;
    assert_eq!(provider.messages()[0].email.body_text, "SMTP desk answer");
    rejections(&d, &dns, &endpoint).await;
    sqlx::query("UPDATE principals SET open_to_public=true WHERE company_id=$1 AND id=$2")
        .bind(d.fixture.target.company.as_uuid())
        .bind(endpoint.principal.as_uuid())
        .execute(d.p().pool())
        .await
        .unwrap();
    assert!(
        submit(&d, &dns, Mail::to(endpoint.address.clone()))
            .await
            .starts_with("250 ")
    );
    restricted_desk(&d).await;
    let restricted = submit(&d, &dns, Mail::to(d.address("restricted").into())).await;
    assert!(restricted.starts_with("550 "), "{restricted}");
    assert!(dns.queried("partner.test."));
    assert!(dns.queried("_dmarc.partner.test."));
    assert_eq!(
        d.count("SELECT count(*) FROM messages WHERE company_id=$1 AND direction='inbound'")
            .await,
        2
    );
    dns.stop().await;
    assert_eq!(d.llm.finish().await.unwrap(), 1);
}
