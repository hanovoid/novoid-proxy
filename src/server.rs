use crate::openai::{
    anthropic_to_open_ai_response, open_ai_to_anthropic_request, pipe_anthropic_stream_to_openai,
};
use crate::router::{
    extract_requested_tier, group_tiers, resolve_group_and_tier_from_model, DispatchResult,
    ModelRouter, DEFAULT_FALLBACK_API_KEY, GROUP_KEYS,
};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

const BODY_LIMIT_BYTES: usize = 50 * 1024 * 1024;

#[derive(Clone)]
pub struct ProxyState {
    pub router: Arc<ModelRouter>,
    pub shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

pub fn build_router(state: ProxyState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/v1/models", get(handle_models))
        .route("/v1/messages", post(handle_messages))
        .route("/v1/messages/count_tokens", post(handle_count_tokens))
        .route("/v1/chat/completions", post(handle_chat_completions))
        .fallback(handle_passthrough)
        .layer(DefaultBodyLimit::max(BODY_LIMIT_BYTES))
        .layer(cors)
        .with_state(state)
}

fn enrich_forwarded_headers(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in headers {
        let lower = k.as_str().to_lowercase();
        if lower == "host" || lower == "content-length" {
            continue;
        }
        out.insert(k.clone(), v.clone());
    }
    if !out.contains_key("x-forwarded-host") {
        if let Some(host_val) = headers.get(header::HOST) {
            out.insert(HeaderName::from_static("x-forwarded-host"), host_val.clone());
        }
    }
    if !out.contains_key("x-forwarded-proto") {
        out.insert(
            HeaderName::from_static("x-forwarded-proto"),
            HeaderValue::from_static("http"),
        );
    }
    out
}

fn copy_filtered_response_headers(src: &HeaderMap, dst: &mut HeaderMap) {
    for (k, v) in src {
        let lower = k.as_str().to_lowercase();
        if lower == "transfer-encoding"
            || lower == "connection"
            || lower == "keep-alive"
            || lower == "content-encoding"
            || lower == "content-length"
        {
            continue;
        }
        dst.insert(k.clone(), v.clone());
    }
}

fn stream_reqwest_response(
    resp: reqwest::Response,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> Response {
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
    let mut builder = Response::builder().status(status);
    if let Some(hdrs) = builder.headers_mut() {
        copy_filtered_response_headers(resp.headers(), hdrs);
    }

    let mut byte_stream = resp.bytes_stream();
    let body_stream = async_stream::stream! {
        loop {
            if *shutdown_rx.borrow() {
                break;
            }
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    break;
                }
                next_chunk = byte_stream.next() => {
                    match next_chunk {
                        Some(Ok(bytes)) => yield Ok::<Bytes, std::io::Error>(bytes),
                        Some(Err(_)) | None => break,
                    }
                }
            }
        }
    };

    builder
        .body(Body::from_stream(body_stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn handle_models(
    State(state): State<ProxyState>,
    Query(params): Query<HashMap<String, String>>,
) -> Json<Value> {
    let force = params.get("refresh").map(|v| v == "true").unwrap_or(false);
    let upstream_json = state.router.refresh_models(force).await;
    let existing_data = upstream_json
        .as_ref()
        .and_then(|v| v.get("data"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let existing_ids: HashSet<String> = existing_data
        .iter()
        .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect();

    let mut virtual_models = Vec::new();
    for key in GROUP_KEYS {
        if !existing_ids.contains(key) {
            virtual_models.push(json!({
                "id": key,
                "object": "model",
                "created": 1700000000,
                "owned_by": "novoid-proxy"
            }));
        }
        for tier in group_tiers(key) {
            let vid = format!("{key}-{tier}");
            if !existing_ids.contains(&vid) {
                virtual_models.push(json!({
                    "id": vid,
                    "object": "model",
                    "created": 1700000000,
                    "owned_by": "novoid-proxy"
                }));
            }
        }
    }

    let mut combined = virtual_models;
    combined.extend(existing_data);
    Json(json!({
        "object": "list",
        "data": combined
    }))
}

async fn handle_count_tokens(
    State(state): State<ProxyState>,
    req: Request,
) -> Response {
    let (parts, body) = req.into_parts();
    let uri = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/v1/messages/count_tokens")
        .to_string();
    let raw_bytes = match axum::body::to_bytes(body, BODY_LIMIT_BYTES).await {
        Ok(b) => b,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let forward_headers = enrich_forwarded_headers(&parts.headers);

    if let Ok(mut parsed) = serde_json::from_slice::<Value>(&raw_bytes) {
        if let Some((group_key, suffix_tier)) = parsed
            .get("model")
            .and_then(|v| v.as_str())
            .and_then(resolve_group_and_tier_from_model)
        {
            let tier = extract_requested_tier(&parts.headers, &parsed).or(suffix_tier);
            if state.router.get_pool(&group_key, tier.as_deref()).await.is_empty() {
                state.router.refresh_models(false).await;
            }
            let pool = state.router.get_pool(&group_key, tier.as_deref()).await;
            let target_model = pool
                .first()
                .cloned()
                .unwrap_or_else(|| "claude-sonnet-4-6".to_string());
            parsed["model"] = Value::String(target_model);

            match state
                .router
                .client
                .post(format!("{}{}", state.router.upstream_url, uri))
                .headers(forward_headers)
                .json(&parsed)
                .send()
                .await
            {
                Ok(resp) => return stream_reqwest_response(resp, state.shutdown_rx.clone()),
                Err(err) => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(json!({
                            "type": "error",
                            "error": { "type": "api_error", "message": format!("Upstream error: {err}") }
                        })),
                    )
                        .into_response();
                }
            }
        }
    }

    passthrough_inner(&state, parts.method, &uri, forward_headers, raw_bytes).await
}

async fn handle_messages(State(state): State<ProxyState>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let uri = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/v1/messages")
        .to_string();
    let raw_bytes = match axum::body::to_bytes(body, BODY_LIMIT_BYTES).await {
        Ok(b) => b,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let forward_headers = enrich_forwarded_headers(&parts.headers);

    if let Ok(parsed) = serde_json::from_slice::<Value>(&raw_bytes) {
        if let Some((group_key, suffix_tier)) = parsed
            .get("model")
            .and_then(|v| v.as_str())
            .and_then(resolve_group_and_tier_from_model)
        {
            let requested_model = parsed
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tier = extract_requested_tier(&parts.headers, &parsed).or(suffix_tier);

            return match state
                .router
                .dispatch_with_group_rotation(
                    &group_key,
                    &requested_model,
                    &forward_headers,
                    &parsed,
                    tier.as_deref(),
                )
                .await
            {
                DispatchResult::UpstreamOk { response, .. } => {
                    stream_reqwest_response(response, state.shutdown_rx.clone())
                }
                DispatchResult::UpstreamRawError { status, body } => {
                    let st = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
                    Response::builder()
                        .status(st)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(body))
                        .unwrap_or_else(|_| st.into_response())
                }
                DispatchResult::RouterError { status, body } => {
                    let st = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
                    (st, Json(body)).into_response()
                }
            };
        }
    }

    passthrough_inner(&state, parts.method, &uri, forward_headers, raw_bytes).await
}

async fn handle_chat_completions(State(state): State<ProxyState>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let uri = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/v1/chat/completions")
        .to_string();
    let raw_bytes = match axum::body::to_bytes(body, BODY_LIMIT_BYTES).await {
        Ok(b) => b,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let forward_headers = enrich_forwarded_headers(&parts.headers);

    if let Ok(parsed) = serde_json::from_slice::<Value>(&raw_bytes) {
        if let Some((group_key, suffix_tier)) = parsed
            .get("model")
            .and_then(|v| v.as_str())
            .and_then(resolve_group_and_tier_from_model)
        {
            let requested_model = parsed
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tier = extract_requested_tier(&parts.headers, &parsed).or(suffix_tier);
            let anth_payload = open_ai_to_anthropic_request(&parsed);

            let api_key = parts
                .headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| {
                    s.strip_prefix("Bearer ")
                        .or_else(|| s.strip_prefix("bearer "))
                })
                .or_else(|| {
                    parts
                        .headers
                        .get("x-api-key")
                        .and_then(|v| v.to_str().ok())
                })
                .filter(|s| !s.is_empty())
                .unwrap_or(DEFAULT_FALLBACK_API_KEY);

            let mut anth_headers = HeaderMap::new();
            if let Ok(val) = HeaderValue::from_str(api_key) {
                anth_headers.insert(HeaderName::from_static("x-api-key"), val);
            }
            anth_headers.insert(
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_static("2023-06-01"),
            );

            match state
                .router
                .dispatch_with_group_rotation(
                    &group_key,
                    &requested_model,
                    &anth_headers,
                    &anth_payload,
                    tier.as_deref(),
                )
                .await
            {
                DispatchResult::UpstreamOk { response, .. } => {
                    let is_stream = parsed
                        .get("stream")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if is_stream {
                        let oai_stream = pipe_anthropic_stream_to_openai(response, requested_model);
                        return Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
                            .header(header::CACHE_CONTROL, "no-cache")
                            .header(header::CONNECTION, "keep-alive")
                            .body(Body::from_stream(oai_stream))
                            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
                    } else {
                        return match response.json::<Value>().await {
                            Ok(anth_json) => {
                                Json(anthropic_to_open_ai_response(&anth_json, &requested_model))
                                    .into_response()
                            }
                            Err(err) => (
                                StatusCode::BAD_GATEWAY,
                                Json(json!({
                                    "error": { "message": format!("Upstream JSON error: {err}"), "type": "api_error" }
                                })),
                            )
                                .into_response(),
                        };
                    }
                }
                DispatchResult::UpstreamRawError { status, body } => {
                    let st = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
                    let msg = serde_json::from_str::<Value>(&body)
                        .ok()
                        .and_then(|v| {
                            v.get("error")
                                .and_then(|e| e.get("message"))
                                .and_then(|m| m.as_str())
                                .map(|s| s.to_string())
                        })
                        .unwrap_or(body);
                    return (
                        st,
                        Json(json!({ "error": { "message": msg, "type": "api_error" } })),
                    )
                        .into_response();
                }
                DispatchResult::RouterError { status, body } => {
                    let st = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
                    let msg = body
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("Upstream error")
                        .to_string();
                    return (
                        st,
                        Json(json!({ "error": { "message": msg, "type": "api_error" } })),
                    )
                        .into_response();
                }
            }
        }
    }

    passthrough_inner(&state, parts.method, &uri, forward_headers, raw_bytes).await
}

async fn handle_passthrough(State(state): State<ProxyState>, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let uri = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();
    let raw_bytes = match axum::body::to_bytes(body, BODY_LIMIT_BYTES).await {
        Ok(b) => b,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let forward_headers = enrich_forwarded_headers(&parts.headers);
    passthrough_inner(&state, parts.method, &uri, forward_headers, raw_bytes).await
}

async fn passthrough_inner(
    state: &ProxyState,
    method: axum::http::Method,
    uri: &str,
    headers: HeaderMap,
    raw_bytes: Bytes,
) -> Response {
    let req_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let url = format!("{}{}", state.router.upstream_url, uri);

    let mut builder = state.router.client.request(req_method, &url).headers(headers);
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        builder = builder.body(raw_bytes);
    }

    match builder.send().await {
        Ok(resp) => stream_reqwest_response(resp, state.shutdown_rx.clone()),
        Err(err) => {
            eprintln!("[NovoidProxy] Loi xu ly request {uri}: {err}");
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": format!("Upstream error: {err}")
                    }
                })),
            )
                .into_response()
        }
    }
}
