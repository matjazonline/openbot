use super::*;
use base64::Engine;
use std::{os::unix::fs::PermissionsExt, path::PathBuf, process::Stdio};

pub(super) struct NativePeer {
    child: tokio::process::Child,
    pub(super) directory: PathBuf,
    output: Option<PathBuf>,
}
impl NativePeer {
    pub(super) async fn spawn(s: &Setup, server: &TestTlsServer) -> Self {
        let source = PathBuf::from(
            std::env::var("OPENBOT_NATIVE_HARNESS_SOURCE")
                .expect("set owned pinned Cloudroom source path"),
        );
        let directory =
            std::env::temp_dir().join(format!("openbot-native-company-mcp-{}", Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let config = json!({"directory":directory,"pluginPath":root.join("integrations/cloudroom-plugin"),
            "journalHelper":root.join("integrations/cloudroom-plugin/build-tests/native-integration-peer.js"),
            "paired":{"state":"paired","version":1,"binding":{"origin":server.origin,"company_id":s.f.company,
            "machine_id":s.paired.receipt.machine.id,"installation_id":s.paired.receipt.machine.installation_id,
            "credential_generation":s.paired.receipt.machine.credential_generation,"journal_id":Uuid::new_v4()},
            "credential":s.paired.candidate.credential.expose_secret()}});
        let file = directory.join("config.json");
        std::fs::write(&file, serde_json::to_vec(&config).unwrap()).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let ca = directory.join("ca.pem");
        let encoded = base64::engine::general_purpose::STANDARD.encode(server.certificate.as_ref());
        std::fs::write(
            &ca,
            format!("-----BEGIN CERTIFICATE-----\n{encoded}\n-----END CERTIFICATE-----\n"),
        )
        .unwrap();
        let home = directory.join("home");
        let tmp = directory.join("tmp");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&tmp).unwrap();
        let node = PathBuf::from(
            std::env::var("OPENBOT_NATIVE_HARNESS_NODE").expect("set pinned Node24 executable"),
        );
        let log = std::fs::File::create(directory.join("native.log")).unwrap();
        let child = tokio::process::Command::new(&node)
            .arg(source.join("gui/node_modules/vitest/vitest.mjs"))
            .args([
                "run",
                "--config",
                "vitest.required-native.config.ts",
                "real/required-native-company-mcp.test.ts",
            ])
            .current_dir(source.join("gui/tests/integration"))
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:/opt/homebrew/bin:/usr/bin:/bin",
                    node.parent().unwrap().display()
                ),
            )
            .env("HOME", home)
            .env("TMPDIR", tmp)
            .env("NO_COLOR", "1")
            .env("NODE_EXTRA_CA_CERTS", ca)
            .env("OPENBOT_NATIVE_COMPANY_MCP_CONFIG", file)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        Self {
            child,
            directory,
            output: std::env::var_os("OPENBOT_NATIVE_HARNESS_OUTPUT").map(PathBuf::from),
        }
    }
    pub(super) async fn read(&mut self, name: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(150), async {
            loop {
                if let Ok(bytes) = std::fs::read(self.directory.join(name)) {
                    return serde_json::from_slice(&bytes).unwrap();
                }
                assert!(
                    self.child.try_wait().unwrap().is_none(),
                    "native fixture exited; see {}",
                    self.directory.join("native.log").display()
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("native fixture handshake deadline")
    }
    pub(super) fn write(&self, name: &str, value: Value) {
        let temporary = self.directory.join(format!("{name}.tmp"));
        std::fs::write(&temporary, serde_json::to_vec(&value).unwrap()).unwrap();
        std::fs::rename(temporary, self.directory.join(name)).unwrap();
    }
    pub(super) fn audit(
        &self,
        s: &Setup,
        run: crate::domain::workflow::RunId,
        cancel: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let p = s.f.persistence.clone();
        let company = CompanyId::new(s.f.company);
        let path = self.directory.join("sql-liveness.txt");
        tokio::spawn(async move {
            use std::io::Write;
            let mut output = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            loop {
                if cancel.is_cancelled() {
                    break;
                }
                let state = crate::adapters::persistence::local_agents::test_support::mcp_workflow::native_state(p.as_ref(), company, run).await;
                writeln!(output, "{}", json!({"at":chrono::Utc::now(),"state":state})).unwrap();
                if matches!(
                    state["state"].as_str(),
                    Some("queued" | "running" | "waiting")
                ) {
                    assert_eq!(
                        state["live"], true,
                        "native source lost authority during the healthy call: {state}"
                    );
                }
                tokio::select! { biased;
                    _ = cancel.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_millis(50)) => {},
                }
            }
        })
    }
    pub(super) async fn close(mut self) {
        self.write("finish.json", json!({"done":true}));
        let status = tokio::time::timeout(Duration::from_secs(20), self.child.wait())
            .await
            .unwrap()
            .unwrap();
        self.retain();
        assert!(status.success(), "native fixture failed");
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
    fn retain(&self) {
        if let Some(output) = &self.output {
            for name in [
                "proof.json",
                "native.log",
                "native-failure.json",
                "connector.json",
                "sql-liveness.txt",
            ] {
                let source = self.directory.join(name);
                if source.is_file() {
                    let _ = std::fs::copy(source, output.with_extension(name));
                }
            }
        }
    }
}
impl Drop for NativePeer {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            self.write("abort.json", json!({"abort":true}));
            for _ in 0..320 {
                if self.child.try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            let _ = self.child.start_kill();
        }
        self.retain();
    }
}
