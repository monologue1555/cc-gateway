use super::{server::ProxyServer, ProxyConfig};
use crate::{
    agent_gateway::{self, AgentGatewayConfigInput},
    database::Database,
    provider::{ClaudeDesktopMode, ClaudeDesktopModelRoute, Provider, ProviderMeta},
};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use serial_test::serial;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::{sync::oneshot, task::JoinHandle};

const UPSTREAM_KEY: &str = "upstream-secret-key";
const PUBLIC_MODEL: &str = "claude-fable-5";
const UPSTREAM_MODEL: &str = "fable-upstream-5";

#[derive(Clone, Debug)]
struct CapturedRequest {
    authorization: Option<String>,
    x_api_key: Option<String>,
    api_key: Option<String>,
    cookie: Option<String>,
    proxy_authorization: Option<String>,
    local_identity: Option<String>,
    anthropic_beta: Option<String>,
    body: Value,
}

#[derive(Clone, Default)]
struct MockState {
    requests: Arc<Mutex<Vec<CapturedRequest>>>,
}

struct MockAnthropicServer {
    origin: String,
    state: MockState,
    shutdown: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl MockAnthropicServer {
    async fn start() -> Self {
        let state = MockState::default();
        let app = Router::new()
            .route("/v1/messages", post(mock_anthropic_messages))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock Anthropic upstream");
        let address = listener.local_addr().expect("read mock upstream address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("serve mock Anthropic upstream");
        });

        Self {
            origin: format!("http://{address}"),
            state,
            shutdown: Some(shutdown_tx),
            handle: Some(handle),
        }
    }

    fn captured_requests(&self) -> Vec<CapturedRequest> {
        self.state
            .requests
            .lock()
            .expect("lock captured upstream requests")
            .clone()
    }

    async fn stop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.await.expect("join mock Anthropic upstream");
        }
    }
}

impl Drop for MockAnthropicServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

async fn mock_anthropic_messages(
    State(state): State<MockState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    state
        .requests
        .lock()
        .expect("lock captured upstream requests")
        .push(CapturedRequest {
            authorization: header_value(&headers, "authorization"),
            x_api_key: header_value(&headers, "x-api-key"),
            api_key: header_value(&headers, "api-key"),
            cookie: header_value(&headers, "cookie"),
            proxy_authorization: header_value(&headers, "proxy-authorization"),
            local_identity: header_value(&headers, "x-local-identity"),
            anthropic_beta: header_value(&headers, "anthropic-beta"),
            body,
        });

    (
        StatusCode::OK,
        Json(json!({
            "id": "msg_mock_1",
            "type": "message",
            "role": "assistant",
            "model": UPSTREAM_MODEL,
            "content": [{ "type": "text", "text": "mock reply" }],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {
                "input_tokens": 3,
                "output_tokens": 2,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        })),
    )
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

fn source_provider(upstream_origin: &str) -> Provider {
    let mut provider = Provider::with_id(
        "mock-anthropic".to_string(),
        "Mock Anthropic".to_string(),
        json!({
            "env": {
                "ANTHROPIC_AUTH_TOKEN": UPSTREAM_KEY,
                "ANTHROPIC_BASE_URL": upstream_origin
            }
        }),
        None,
    );
    provider.meta = Some(ProviderMeta {
        api_format: Some("anthropic".to_string()),
        api_key_field: Some("ANTHROPIC_AUTH_TOKEN".to_string()),
        claude_desktop_mode: Some(ClaudeDesktopMode::Proxy),
        claude_desktop_model_routes: HashMap::from([(
            PUBLIC_MODEL.to_string(),
            ClaudeDesktopModelRoute {
                model: UPSTREAM_MODEL.to_string(),
                label_override: Some("Claude Fable 5".to_string()),
                supports_1m: Some(false),
            },
        )]),
        ..ProviderMeta::default()
    });
    provider
}

async fn post_json(
    client: &reqwest::Client,
    url: &str,
    token_header: (&str, String),
    body: Value,
    include_private_headers: bool,
) -> (StatusCode, Value) {
    let mut request = client
        .post(url)
        .header(token_header.0, token_header.1)
        .json(&body);
    if include_private_headers {
        request = request
            .header("api-key", "private-local-api-key")
            .header("cookie", "session=private-local-cookie")
            .header("proxy-authorization", "Basic private-local-proxy-auth")
            .header("x-local-identity", "private-local-identity");
    }
    let response = request.send().await.expect("send request to Agent Gateway");
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .expect("read Agent Gateway response body");
    let body = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "Agent Gateway returned non-JSON body ({error}): {}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, body)
}

async fn get_json(client: &reqwest::Client, url: &str, token: &str) -> (StatusCode, Value) {
    let response = client
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .expect("send request to Agent Gateway");
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .expect("read Agent Gateway response body");
    let body = serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        panic!(
            "Agent Gateway returned non-JSON body ({error}): {}",
            String::from_utf8_lossy(&bytes)
        )
    });
    (status, body)
}

#[tokio::test]
#[serial]
async fn agent_gateway_rejects_an_incompatible_desktop_route_before_upstream() {
    let mut upstream = MockAnthropicServer::start().await;
    let db = Arc::new(Database::memory().expect("create in-memory database"));
    let mut transformed_provider = source_provider(&upstream.origin);
    transformed_provider.id = "mock-openai".to_string();
    transformed_provider.name = "Mock OpenAI Chat".to_string();
    transformed_provider
        .meta
        .as_mut()
        .expect("provider meta")
        .api_format = Some("openai_chat".to_string());
    db.save_provider("claude-desktop", &transformed_provider)
        .expect("save transformed source provider");
    db.set_current_provider("claude-desktop", &transformed_provider.id)
        .expect("select transformed source provider");
    let token = "ccs-agent-incompatible-route-test".to_string();
    db.set_setting(
        agent_gateway::AGENT_GATEWAY_CONFIG_KEY,
        &json!({
            "enabled": true,
            "token": token,
            "emulateClaudeCode": false
        })
        .to_string(),
    )
    .expect("seed enabled gateway config");

    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_address: "127.0.0.1".to_string(),
            listen_port: 0,
            enable_logging: false,
            ..ProxyConfig::default()
        },
        db,
        None,
    );
    let proxy_info = proxy.start().await.expect("start CC Switch proxy");
    let base_url = format!("http://127.0.0.1:{}", proxy_info.port);
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("build loopback test client");
    let (models_status, models_body) =
        get_json(&client, &format!("{base_url}/agent/v1/models"), &token).await;
    let (messages_status, messages_body) = post_json(
        &client,
        &format!("{base_url}/agent/v1/messages"),
        ("authorization", format!("Bearer {token}")),
        json!({
            "model": PUBLIC_MODEL,
            "max_tokens": 32,
            "messages": [{ "role": "user", "content": "hello" }]
        }),
        false,
    )
    .await;
    let (responses_status, responses_body) = post_json(
        &client,
        &format!("{base_url}/agent/v1/responses"),
        ("authorization", format!("Bearer {token}")),
        json!({ "model": PUBLIC_MODEL, "input": "hello" }),
        false,
    )
    .await;
    let (chat_status, chat_body) = post_json(
        &client,
        &format!("{base_url}/agent/v1/chat/completions"),
        ("authorization", format!("Bearer {token}")),
        json!({
            "model": PUBLIC_MODEL,
            "messages": [{ "role": "user", "content": "hello" }]
        }),
        false,
    )
    .await;

    proxy.stop().await.expect("stop CC Switch proxy");
    upstream.stop().await;

    for (status, body) in [
        (models_status, models_body),
        (messages_status, messages_body),
        (responses_status, responses_body),
        (chat_status, chat_body),
    ] {
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        assert!(body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Anthropic Messages")));
    }
    assert!(
        upstream.captured_requests().is_empty(),
        "an incompatible route must be rejected before any upstream request"
    );
}

#[tokio::test]
#[serial]
async fn agent_gateway_serves_three_protocols_and_guards_access() {
    let mut upstream = MockAnthropicServer::start().await;
    let db = Arc::new(Database::memory().expect("create in-memory database"));
    let provider = source_provider(&upstream.origin);
    db.save_provider("claude-desktop", &provider)
        .expect("save Claude Desktop source provider");
    db.set_current_provider("claude-desktop", &provider.id)
        .expect("select Claude Desktop source provider");

    let gateway_config = agent_gateway::update_config(
        db.as_ref(),
        AgentGatewayConfigInput {
            enabled: true,
            emulate_claude_code: false,
        },
    )
    .expect("enable Agent Gateway");
    let gateway_token = gateway_config.token;
    let desktop_token = crate::claude_desktop_config::get_or_create_gateway_token(db.as_ref())
        .expect("create Claude Desktop gateway token");

    let proxy = ProxyServer::new(
        ProxyConfig {
            listen_address: "127.0.0.1".to_string(),
            listen_port: 0,
            enable_logging: false,
            ..ProxyConfig::default()
        },
        db.clone(),
        None,
    );
    let proxy_info = proxy.start().await.expect("start CC Switch proxy");
    let base_url = format!("http://127.0.0.1:{}", proxy_info.port);
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("build loopback test client");

    let desktop_request = json!({
        "model": PUBLIC_MODEL,
        "max_tokens": 64,
        "stream": false,
        "messages": [{ "role": "user", "content": "hello" }]
    });
    let (desktop_status, desktop_body) = post_json(
        &client,
        &format!("{base_url}/claude-desktop/v1/messages"),
        ("authorization", format!("Bearer {desktop_token}")),
        desktop_request.clone(),
        false,
    )
    .await;

    let (bad_token_status, _) = post_json(
        &client,
        &format!("{base_url}/agent/v1/messages"),
        ("authorization", "Bearer wrong-local-key".to_string()),
        json!({
            "model": PUBLIC_MODEL,
            "max_tokens": 64,
            "stream": false,
            "messages": [{ "role": "user", "content": "hello" }]
        }),
        true,
    )
    .await;

    let (unknown_model_status, _) = post_json(
        &client,
        &format!("{base_url}/agent/v1/messages"),
        ("x-api-key", gateway_token.clone()),
        json!({
            "model": "claude-unknown",
            "max_tokens": 64,
            "stream": false,
            "messages": [{ "role": "user", "content": "hello" }]
        }),
        true,
    )
    .await;

    let (compact_status, compact_body) = post_json(
        &client,
        &format!("{base_url}/agent/v1/responses/compact"),
        ("authorization", format!("Bearer {gateway_token}")),
        json!({
            "model": PUBLIC_MODEL,
            "input": "hello"
        }),
        true,
    )
    .await;

    let (messages_status, messages_body) = post_json(
        &client,
        &format!("{base_url}/agent/v1/messages"),
        ("authorization", format!("Bearer {gateway_token}")),
        json!({
            "model": PUBLIC_MODEL,
            "max_tokens": 64,
            "stream": false,
            "messages": [{ "role": "user", "content": "hello" }]
        }),
        true,
    )
    .await;

    let (responses_status, responses_body) = post_json(
        &client,
        &format!("{base_url}/agent/v1/responses"),
        ("authorization", format!("Bearer {gateway_token}")),
        json!({
            "model": PUBLIC_MODEL,
            "stream": false,
            "max_output_tokens": 64,
            "input": [{
                "role": "user",
                "content": [{ "type": "input_text", "text": "hello" }]
            }]
        }),
        true,
    )
    .await;

    let (chat_status, chat_body) = post_json(
        &client,
        &format!("{base_url}/agent/v1/chat/completions"),
        ("x-api-key", gateway_token.clone()),
        json!({
            "model": PUBLIC_MODEL,
            "stream": false,
            "max_tokens": 64,
            "messages": [{ "role": "user", "content": "hello" }]
        }),
        true,
    )
    .await;

    proxy.stop().await.expect("stop CC Switch proxy");
    upstream.stop().await;
    let captured = upstream.captured_requests();

    assert_eq!(desktop_status, StatusCode::OK, "{desktop_body}");
    assert_eq!(desktop_body["type"], "message");
    assert_eq!(bad_token_status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown_model_status, StatusCode::BAD_REQUEST);
    assert_eq!(compact_status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(
        compact_body["error"]["code"],
        "agent_gateway_compaction_unsupported"
    );

    assert_eq!(messages_status, StatusCode::OK, "{messages_body}");
    assert_eq!(messages_body["type"], "message");
    assert_eq!(messages_body["content"][0]["type"], "text");
    assert_eq!(messages_body["content"][0]["text"], "mock reply");

    assert_eq!(responses_status, StatusCode::OK, "{responses_body}");
    assert_eq!(responses_body["object"], "response");
    assert_eq!(responses_body["status"], "completed");
    assert_eq!(responses_body["output"][0]["type"], "message");
    assert_eq!(
        responses_body["output"][0]["content"][0]["type"],
        "output_text"
    );

    assert_eq!(chat_status, StatusCode::OK, "{chat_body}");
    assert_eq!(chat_body["object"], "chat.completion");
    assert_eq!(chat_body["choices"][0]["message"]["role"], "assistant");
    assert_eq!(chat_body["choices"][0]["message"]["content"], "mock reply");

    assert_eq!(
        captured.len(),
        4,
        "auth/model validation failures must not reach upstream"
    );
    assert_eq!(
        captured[0].body, captured[1].body,
        "Agent Messages must converge on the same Anthropic request as the proven Desktop route"
    );
    let desktop_beta = captured[0].anthropic_beta.clone();
    assert!(desktop_beta
        .as_deref()
        .is_some_and(|value| value.contains("claude-code-20250219")));
    for request in captured {
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer upstream-secret-key")
        );
        assert_ne!(
            request.authorization.as_deref(),
            Some(format!("Bearer {gateway_token}").as_str())
        );
        assert_ne!(request.x_api_key.as_deref(), Some(gateway_token.as_str()));
        assert!(request.api_key.is_none());
        assert!(request.cookie.is_none());
        assert!(request.proxy_authorization.is_none());
        assert!(request.local_identity.is_none());
        assert_eq!(request.anthropic_beta, desktop_beta);
        assert_eq!(request.body["model"], UPSTREAM_MODEL);
    }
}
