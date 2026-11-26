use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use novoid_proxy::openai::{anthropic_to_open_ai_response, open_ai_to_anthropic_request};
use novoid_proxy::router::{
    classify_model_group, classify_upstream_failure, resolve_group_and_tier_from_model,
    sort_by_priority, sort_pool_for_tier, ModelRouter,
};
use novoid_proxy::server::{build_router, ProxyState};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn test_grouping_priority_and_tier_parsing() {
    assert_eq!(classify_model_group("claude-opus-5-5-high"), Some("novoid-claude"));
    assert_eq!(classify_model_group("gemini-4-hydrogen-sonar"), Some("novoid-hydrogen"));
    assert_eq!(classify_model_group("gemini-3.8-flash-tiered"), Some("novoid-flash"));
    assert_eq!(classify_model_group("gemini-3-flash"), Some("novoid-default"));
    assert_eq!(classify_model_group("gemini-3.1-flash-image"), None);
    assert_eq!(classify_model_group("gemini-4-argon-disabled"), None);

    assert_eq!(
        resolve_group_and_tier_from_model("novoid-hydrogen-xhigh"),
        Some(("novoid-hydrogen".to_string(), Some("xhigh".to_string())))
    );
    assert_eq!(
        resolve_group_and_tier_from_model("anthropic/novoid-claude-opus-high"),
        Some(("novoid-claude".to_string(), Some("opus-high".to_string())))
    );
    assert_eq!(
        resolve_group_and_tier_from_model("novoid-core/novoid-hydrogen#sonar"),
        Some(("novoid-hydrogen".to_string(), Some("sonar".to_string())))
    );

    let sorted = sort_by_priority(&[
        "gemini-4-hydrogen-low".into(),
        "gemini-4-hydrogen-tiered-xhigh".into(),
        "gemini-4-hydrogen-xhigh".into(),
        "gemini-4-hydrogen-high".into(),
    ]);
    assert_eq!(
        sorted,
        vec![
            "gemini-4-hydrogen-xhigh",
            "gemini-4-hydrogen-high",
            "gemini-4-hydrogen-low",
            "gemini-4-hydrogen-tiered-xhigh"
        ]
    );

    let claude_pool = vec![
        "claude-opus-5-5-high".to_string(),
        "claude-sonnet-5-5-low".to_string(),
        "claude-sonnet-5-5-high".to_string(),
    ];
    let sorted_sonnet_low = sort_pool_for_tier(&claude_pool, "novoid-claude", Some("sonnet-low"));
    assert_eq!(sorted_sonnet_low[0], "claude-sonnet-5-5-low");
}

#[test]
fn test_failure_classification_and_openai_conversion() {
    assert!(classify_upstream_failure(503, r#"{"error":{"message":"No accounts available"}}"#).is_fatal_pool_error);
    assert!(classify_upstream_failure(401, "unauthorized").is_fatal_pool_error);
    assert!(classify_upstream_failure(400, "RESOURCE_EXHAUSTED").is_quota_or_rate_limit);
    assert!(classify_upstream_failure(404, "MODEL_NOT_SUPPORTED").is_unsupported_model);

    let anth = open_ai_to_anthropic_request(&json!({
        "model": "novoid-claude-opus-high",
        "tool_choice": "required",
        "tools": [{ "type": "function", "function": { "name": "ls", "description": "list", "parameters": { "type": "object" } } }],
        "messages": [
            { "role": "system", "content": "Sys" },
            { "role": "user", "content": [{ "type": "text", "text": "Hi" }, { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }] },
            { "role": "assistant", "content": null, "tool_calls": [{ "id": "tc1", "type": "function", "function": { "name": "ls", "arguments": "{}" } }] },
            { "role": "tool", "tool_call_id": "tc1", "content": "a" },
            { "role": "tool", "tool_call_id": "tc2", "content": "b" }
        ]
    }));

    assert_eq!(anth["system"], "Sys");
    assert_eq!(anth["tool_choice"]["type"], "any");
    assert_eq!(anth["messages"][2]["role"], "user");
    assert_eq!(anth["messages"][2]["content"].as_array().unwrap().len(), 2);

    let oai = anthropic_to_open_ai_response(
        &json!({
            "id": "msg_1",
            "stop_reason": "tool_use",
            "content": [{ "type": "tool_use", "id": "tc1", "name": "ls", "input": { "dir": "." } }],
            "usage": { "input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 20 }
        }),
        "novoid-claude-opus-high",
    );
    assert_eq!(oai["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(oai["usage"]["prompt_tokens"], 30);
    assert_eq!(oai["usage"]["prompt_tokens_details"]["cached_tokens"], 20);
}

#[tokio::test]
async fn test_proxy_e2e_routing_tiers_and_rotation() {
    #[derive(Clone)]
    struct UpState {
        calls: Arc<AtomicUsize>,
        mode: Arc<tokio::sync::RwLock<String>>,
    }

    let up_state = UpState {
        calls: Arc::new(AtomicUsize::new(0)),
        mode: Arc::new(tokio::sync::RwLock::new("normal".to_string())),
    };

    let upstream_app = Router::new()
        .route(
            "/v1/models",
            get(|| async {
                Json(json!({
                    "data": [
                        { "id": "gemini-4-hydrogen-xhigh" },
                        { "id": "gemini-4-hydrogen-high" },
                        { "id": "gemini-4-hydrogen-low" },
                        { "id": "gemini-4-hydrogen-sonar" },
                        { "id": "claude-opus-5-5-high" },
                        { "id": "claude-sonnet-5-5-low" }
                    ]
                }))
            }),
        )
        .route(
            "/v1/messages",
            post(|State(st): State<UpState>, headers: HeaderMap, Json(body): Json<Value>| async move {
                st.calls.fetch_add(1, Ordering::SeqCst);
                let mode = st.mode.read().await.clone();
                if mode == "fatal503" {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({ "error": { "message": "No accounts available" } })),
                    )
                        .into_response();
                }
                if mode == "rotate429" && body["model"] == "claude-opus-5-5-high" {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(json!({ "error": { "message": "RATE_LIMITED" } })),
                    )
                        .into_response();
                }
                assert!(headers.get("x-novoid-tier").is_none());
                Json(json!({
                    "id": "msg_ok",
                    "content": [{ "type": "text", "text": body["model"] }],
                    "stop_reason": "end_turn",
                    "usage": { "input_tokens": 1, "output_tokens": 1 }
                }))
                .into_response()
            }),
        )
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .with_state(up_state.clone());

    let up_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let up_addr = up_listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(up_listener, upstream_app).await;
    });

    let (_tx, rx) = tokio::sync::watch::channel(false);
    let router = ModelRouter::new(format!("http://{up_addr}"));
    let proxy_app = build_router(ProxyState {
        router,
        shutdown_rx: rx,
    });
    let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy_listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(proxy_listener, proxy_app).await;
    });

    let client = reqwest::Client::new();

    // 1. /v1/models lists virtual group + tier IDs
    let models: Value = client
        .get(format!("http://{proxy_addr}/v1/models"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(ids.contains(&"novoid-hydrogen-xhigh"));
    assert!(ids.contains(&"novoid-hydrogen-sonar"));
    assert!(ids.contains(&"novoid-claude-opus-high"));

    // 2. Tier header & dash syntax
    let r1: Value = client
        .post(format!("http://{proxy_addr}/v1/messages"))
        .header("x-novoid-tier", "low")
        .json(&json!({ "model": "novoid-hydrogen", "messages": [{ "role": "user", "content": "hi" }] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r1["content"][0]["text"], "gemini-4-hydrogen-low");

    let r2: Value = client
        .post(format!("http://{proxy_addr}/v1/messages"))
        .json(&json!({ "model": "novoid-hydrogen-sonar", "messages": [{ "role": "user", "content": "hi" }] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r2["content"][0]["text"], "gemini-4-hydrogen-sonar");

    // 3. 429 rotation within group
    *up_state.mode.write().await = "rotate429".to_string();
    up_state.calls.store(0, Ordering::SeqCst);
    let r3: Value = client
        .post(format!("http://{proxy_addr}/v1/messages"))
        .json(&json!({ "model": "novoid-claude-opus-high", "messages": [{ "role": "user", "content": "hi" }] }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r3["content"][0]["text"], "claude-sonnet-5-5-low");
    assert_eq!(up_state.calls.load(Ordering::SeqCst), 2);

    // 4. Fatal 503 stops immediately after 1 call
    *up_state.mode.write().await = "fatal503".to_string();
    up_state.calls.store(0, Ordering::SeqCst);
    let r4 = client
        .post(format!("http://{proxy_addr}/v1/messages"))
        .json(&json!({ "model": "novoid-hydrogen", "messages": [{ "role": "user", "content": "hi" }] }))
        .send()
        .await
        .unwrap();
    assert_eq!(r4.status(), 503);
    assert_eq!(up_state.calls.load(Ordering::SeqCst), 1);

    // 5. Pass-through /health
    let h: Value = client
        .get(format!("http://{proxy_addr}/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["status"], "ok");

    // 6. OpenAI /v1/chat/completions with reasoning_effort & dash syntax
    *up_state.mode.write().await = "normal".to_string();
    let _: Value = client
        .get(format!("http://{proxy_addr}/v1/models?refresh=true"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let oai1: Value = client
        .post(format!("http://{proxy_addr}/v1/chat/completions"))
        .json(&json!({
            "model": "novoid-hydrogen",
            "reasoning_effort": "xhigh",
            "messages": [{ "role": "user", "content": "hi" }]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(oai1["choices"][0]["message"]["content"], "gemini-4-hydrogen-xhigh");
    assert_eq!(oai1["model"], "novoid-hydrogen");

    let oai2: Value = client
        .post(format!("http://{proxy_addr}/v1/chat/completions"))
        .json(&json!({
            "model": "novoid-claude-opus-high",
            "messages": [{ "role": "user", "content": "hi" }]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(oai2["choices"][0]["message"]["content"], "claude-opus-5-5-high");
}
