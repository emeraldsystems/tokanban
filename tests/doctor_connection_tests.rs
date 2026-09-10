use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tempfile::TempDir;
use tokanban::config::{save_config, AppConfig};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct Fixture {
    dir: TempDir,
    config: PathBuf,
}

impl Fixture {
    fn new(endpoint: &str) -> Self {
        let dir = TempDir::new().unwrap();
        let config = dir.path().join("tokanban.toml");
        let mut value = AppConfig::default();
        value.api.url = endpoint.to_string();
        value.auth.access_token = Some("cli-secret".into());
        value.auth.token = Some("refresh-secret".into());
        // An expired refreshable credential must be used once, never refreshed by doctor.
        value.auth.expires_at = Some(1);
        save_config(&value, Some(&config)).unwrap();
        Self { dir, config }
    }

    fn command(&self) -> Command {
        self.command_format("json")
    }

    fn command_format(&self, format: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_tokanban"));
        command
            .current_dir(self.dir.path())
            .env("HOME", self.dir.path())
            .env("USERPROFILE", self.dir.path())
            .env("APPDATA", self.dir.path().join("appdata"))
            .env("XDG_CONFIG_HOME", self.dir.path().join("xdg"))
            .env_remove("TOKANBAN_API_KEY")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("HTTP_PROXY")
            .env_remove("HTTPS_PROXY")
            .env_remove("ALL_PROXY")
            .env_remove("http_proxy")
            .env_remove("https_proxy")
            .env_remove("all_proxy")
            .arg("--config")
            .arg(&self.config)
            .args(["--format", format, "--no-color", "doctor"]);
        command
    }
}

fn request() -> Value {
    json!({"jsonrpc":"2.0","id":"tokanban-doctor","method":"tools/list","params":{}})
}

fn response() -> Value {
    json!({"jsonrpc":"2.0","id":"tokanban-doctor","result":{"tools":[{"name":"session_update","inputSchema":{"type":"object","properties":{}}}]}})
}

fn mcp_config(endpoint: &str, token: &str) -> Value {
    json!({"mcpServers":{"tokanban":{"url":format!("{endpoint}/mcp"),"headers":{"Authorization":format!("Bearer {token}")}}}})
}

async fn run(mut command: Command) -> Output {
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

async fn report(command: Command) -> Value {
    serde_json::from_slice(&run(command).await.stdout).unwrap()
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut output = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            output.extend(files(&path));
        } else {
            output.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    output
}

#[tokio::test]
async fn offline_default_never_probes_and_preserves_config_and_state() {
    let server = MockServer::start().await;
    let fixture = Fixture::new(&server.uri());
    let before = files(fixture.dir.path());
    let value = report(fixture.command()).await;
    assert_eq!(value["connection"]["status"], "not_checked");
    assert!(value["connection"]["checked_at"].is_null());
    assert!(value["connection"]["account_fingerprint"].is_null());
    assert!(server.received_requests().await.unwrap().is_empty());
    assert_eq!(files(fixture.dir.path()), before);
}

#[tokio::test]
async fn wired_online_command_uses_one_read_only_request_without_refresh_or_writes() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/mcp"))
        .and(header("authorization", "Bearer cli-secret"))
        .and(header("MCP-Protocol-Version", "2024-11-05"))
        .and(body_json(request()))
        .respond_with(ResponseTemplate::new(200).set_body_json(response()))
        .expect(1)
        .mount(&server)
        .await;
    let fixture = Fixture::new(&server.uri());
    let before = files(fixture.dir.path());
    let mut command = fixture.command();
    command.arg("--online");
    let value = report(command).await;
    assert_eq!(value["connection"]["status"], "connected");
    assert_eq!(
        value["connection"]["account_scope"],
        "current_reporter_account"
    );
    assert_eq!(value["connection"]["endpoint_origin"], server.uri());
    assert_eq!(value["connection"]["http_status"], 200);
    assert_eq!(
        value["connection"]["account_fingerprint"]
            .as_str()
            .unwrap()
            .len(),
        16
    );
    assert!(value["connection"]["checked_at"].is_string());
    assert!(value["reporter"]["last_report"].is_null());
    assert!(!value.to_string().contains("cli-secret"));
    assert!(!value.to_string().contains("refresh-secret"));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert_eq!(files(fixture.dir.path()), before);
}

#[tokio::test]
async fn explicit_connection_alias_has_separate_current_account_human_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_json(request()))
        .respond_with(ResponseTemplate::new(200).set_body_json(response()))
        .expect(1)
        .mount(&server)
        .await;
    let fixture = Fixture::new(&server.uri());
    let mut command = fixture.command_format("table");
    command.arg("--check-connection");
    let output = run(command).await;
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Current reporter connection"));
    assert!(text.contains("Connected"));
    assert!(text.contains("recorded across local accounts"));
    assert!(text.contains("Account fingerprint:"));
    assert!(!text.contains("cli-secret"));
}

#[tokio::test]
async fn diagnoses_http_failures_without_exposing_response_bodies_or_retrying() {
    for (status, expected) in [
        (401, "auth_rejected"),
        (403, "forbidden"),
        (429, "rate_limited"),
        (503, "server_error"),
        (404, "endpoint_rejected"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(status).set_body_string("provider-secret-body cli-secret"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let fixture = Fixture::new(&server.uri());
        let mut command = fixture.command();
        command.arg("--online");
        let value = report(command).await;
        assert_eq!(value["connection"]["status"], expected);
        assert_eq!(value["connection"]["http_status"], status);
        assert!(!value.to_string().contains("provider-secret"));
        assert!(!value.to_string().contains("cli-secret"));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn refuses_redirects_without_forwarding_credentials() {
    let source = MockServer::start().await;
    let destination = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("authorization", "Bearer cli-secret"))
        .respond_with(ResponseTemplate::new(307).insert_header(
            "Location",
            format!("{}/private?secret=redirect-secret", destination.uri()).as_str(),
        ))
        .expect(1)
        .mount(&source)
        .await;
    let fixture = Fixture::new(&source.uri());
    let mut command = fixture.command();
    command.arg("--online");
    let value = report(command).await;
    assert_eq!(value["connection"]["status"], "redirect_rejected");
    assert!(destination.received_requests().await.unwrap().is_empty());
    assert!(!value.to_string().contains("redirect-secret"));
}

#[tokio::test]
async fn rejects_unsafe_endpoints_before_resolving_or_sending_credentials() {
    let server = MockServer::start().await;
    let fixture = Fixture::new(&server.uri());
    for endpoint in [
        "http://example.invalid/mcp".to_string(),
        format!("{}/private-sensitive-path?secret=url-secret", server.uri()),
        format!("http://user:password@{}/mcp", server.address()),
        format!("{}/mcp#fragment-secret", server.uri()),
    ] {
        let mut command = fixture.command();
        command.args(["--online", "--mcp-url", &endpoint]);
        let value = report(command).await;
        assert_eq!(value["connection"]["status"], "unsafe_endpoint");
        for secret in [
            "url-secret",
            "password",
            "fragment-secret",
            "/private-sensitive-path",
        ] {
            assert!(!value.to_string().contains(secret));
        }
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn rejects_malformed_or_oversized_success_responses() {
    for body in [
        json!({}),
        json!({"jsonrpc":"2.0","id":"wrong","result":{"tools":[]}}),
        json!({"jsonrpc":"2.0","id":"tokanban-doctor","error":{"code":-32603,"message":"secret-error"}}),
        json!({"jsonrpc":"2.0","id":"tokanban-doctor","result":{"tools":[{"name":"bad"}]}}),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        let fixture = Fixture::new(&server.uri());
        let mut command = fixture.command();
        command.arg("--online");
        let value = report(command).await;
        assert_eq!(value["connection"]["status"], "malformed_response");
        assert!(!value.to_string().contains("secret-error"));
    }
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 1024 * 1024 + 1]))
        .expect(1)
        .mount(&server)
        .await;
    let fixture = Fixture::new(&server.uri());
    let mut command = fixture.command();
    command.arg("--online");
    assert_eq!(
        report(command).await["connection"]["status"],
        "response_too_large"
    );
}

#[tokio::test]
async fn separates_accounts_and_never_falls_back_from_an_unusable_selected_entry() {
    let server = MockServer::start().await;
    let fixture = Fixture::new(&server.uri());
    let mut fingerprints = Vec::new();
    for account in ["account-a", "account-b"] {
        Mock::given(method("POST"))
            .and(header(
                "authorization",
                format!("Bearer {account}").as_str(),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(response()))
            .expect(1)
            .mount(&server)
            .await;
        let selected = fixture.dir.path().join(format!("{account}.json"));
        fs::write(&selected, mcp_config(&server.uri(), account).to_string()).unwrap();
        let mut command = fixture.command();
        command
            .arg("--online")
            .arg("--claude-config")
            .arg(&selected);
        let value = report(command).await;
        assert_eq!(value["connection"]["status"], "connected");
        fingerprints.push(value["connection"]["account_fingerprint"].clone());
        fs::write(
            &selected,
            json!({"mcpServers":{"tokanban":{"url":format!("{}/mcp",server.uri()),"headers":{}}}})
                .to_string(),
        )
        .unwrap();
        let mut command = fixture.command();
        command
            .arg("--online")
            .arg("--claude-config")
            .arg(&selected);
        assert_eq!(
            report(command).await["connection"]["status"],
            "credentials_missing"
        );
    }
    assert_ne!(fingerprints[0], fingerprints[1]);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn matches_reporter_local_project_user_and_environment_precedence() {
    let server = MockServer::start().await;
    let fixture = Fixture::new(&server.uri());
    let selected = fixture.dir.path().join(".claude.json");
    let mut root = mcp_config(&server.uri(), "user-secret");
    root["projects"][fixture.dir.path().canonicalize().unwrap().to_str().unwrap()] =
        mcp_config(&server.uri(), "local-secret");
    fs::write(&selected, root.to_string()).unwrap();
    fs::write(
        fixture.dir.path().join(".mcp.json"),
        mcp_config(&server.uri(), "project-secret").to_string(),
    )
    .unwrap();
    for token in [
        "env-secret",
        "local-secret",
        "project-secret",
        "user-secret",
    ] {
        Mock::given(method("POST"))
            .and(header("authorization", format!("Bearer {token}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(response()))
            .expect(1)
            .mount(&server)
            .await;
        let mut command = fixture.command();
        command
            .arg("--online")
            .arg("--claude-config")
            .arg(&selected);
        if token == "env-secret" {
            command.env("TOKANBAN_API_KEY", token);
        }
        if token == "project-secret" {
            fs::write(
                &selected,
                mcp_config(&server.uri(), "user-secret").to_string(),
            )
            .unwrap();
        }
        if token == "user-secret" {
            fs::remove_file(fixture.dir.path().join(".mcp.json")).unwrap();
        }
        assert_eq!(report(command).await["connection"]["status"], "connected");
    }
    let mut command = fixture.command();
    command.arg("--online").env("TOKANBAN_API_KEY", "");
    assert_eq!(
        report(command).await["connection"]["status"],
        "credentials_missing"
    );
    let mut command = fixture.command();
    command.arg("--online").env(
        "CLAUDE_CONFIG_DIR",
        fixture.dir.path().join("other-account"),
    );
    assert_eq!(
        report(command).await["connection"]["status"],
        "credentials_missing"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 4);
}

#[tokio::test]
async fn endpoint_override_cannot_reuse_credentials_for_another_endpoint() {
    let account = MockServer::start().await;
    let other = MockServer::start().await;
    let fixture = Fixture::new(&account.uri());
    let selected = fixture.dir.path().join("account.json");
    fs::write(
        &selected,
        mcp_config(&account.uri(), "account-secret").to_string(),
    )
    .unwrap();
    let mut command = fixture.command();
    command
        .arg("--online")
        .arg("--claude-config")
        .arg(selected)
        .args(["--mcp-url", &format!("{}/mcp", other.uri())]);
    assert_eq!(
        report(command).await["connection"]["status"],
        "credentials_missing"
    );
    assert!(account.received_requests().await.unwrap().is_empty());
    assert!(other.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_selected_cli_config_remains_diagnostic_and_never_probes() {
    let server = MockServer::start().await;
    let fixture = Fixture::new(&server.uri());
    fs::write(&fixture.config, "[auth]\naccess_token='parse-secret").unwrap();
    let mut command = fixture.command();
    command
        .arg("--online")
        .env("TOKANBAN_API_KEY", "env-secret");
    let value = report(command).await;
    assert_eq!(value["connection"]["status"], "invalid_config");
    assert_eq!(value["config"]["status"], "invalid_toml");
    assert!(!value.to_string().contains("parse-secret"));
    assert!(!value.to_string().contains("env-secret"));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn distinguishes_network_errors_and_a_five_second_deadline() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let fixture = Fixture::new(&format!("http://{address}"));
    let mut command = fixture.command();
    command.arg("--online");
    assert_eq!(
        report(command).await["connection"]["status"],
        "network_error"
    );
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(7))
                .set_body_json(response()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let fixture = Fixture::new(&server.uri());
    let mut command = fixture.command();
    command.arg("--online");
    let started = Instant::now();
    assert_eq!(report(command).await["connection"]["status"], "timeout");
    assert!(started.elapsed() < Duration::from_secs(7));
}
