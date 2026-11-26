use regex::Regex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};

pub const MODELS_REFRESH_TTL_MS: u64 = 60_000;
pub const MODELS_FETCH_TIMEOUT_SECS: u64 = 10;
pub const DEFAULT_FALLBACK_API_KEY: &str = "sk-novoid-proxy";

pub const GROUP_KEYS: [&str; 7] = [
    "novoid-claude",
    "novoid-default",
    "novoid-flash",
    "novoid-neon",
    "novoid-hydrogen",
    "novoid-carbon",
    "novoid-argon",
];

pub fn group_tiers(group_key: &str) -> &'static [&'static str] {
    match group_key {
        "novoid-claude" => &[
            "opus-high",
            "opus-medium",
            "opus-low",
            "sonnet-high",
            "sonnet-medium",
            "sonnet-low",
        ],
        "novoid-default" => &["low", "medium", "high"],
        "novoid-flash" => &["high", "medium", "low", "tiered", "thinking", "lite"],
        "novoid-neon" => &["xhigh", "high", "medium", "low"],
        "novoid-hydrogen" => &[
            "xhigh",
            "high",
            "medium",
            "low",
            "tiered-xhigh",
            "tiered-high",
            "sonar",
        ],
        "novoid-carbon" => &["high", "medium", "low"],
        "novoid-argon" => &["high", "medium", "low"],
        _ => &[],
    }
}

const FAMILY_PRIORITY: [&str; 20] = [
    "gemini-4-hydrogen",
    "gemini-4-neon",
    "gemini-4-carbon",
    "gemini-4-argon",
    "gemini-3.8-flash",
    "gemini-3.6-flash",
    "gemini-3.5-flash",
    "gemini-3.1-flash",
    "gemini-3-pro",
    "gemini-3-flash",
    "gemini-2.5-pro",
    "gemini-2.5-flash-thinking",
    "gemini-2.5-flash",
    "claude-opus-5-5",
    "claude-opus-4-6",
    "claude-opus",
    "claude-sonnet-5-5",
    "claude-sonnet-4-6",
    "claude-sonnet",
    "claude-haiku",
];

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn is_image_only_model(lower: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[-_.])image(?:$|[-_.])").expect("valid regex"))
        .is_match(lower)
}

fn is_group_4_variant(lower: &str, prefix: &str) -> bool {
    lower == prefix
        || lower.starts_with(&format!("{prefix}-"))
        || lower.starts_with(&format!("{prefix}_"))
        || lower.starts_with(&format!("{prefix}."))
}

pub fn normalize_tier(raw: &str) -> Option<String> {
    let lower = raw.trim().to_lowercase();
    if lower.is_empty() || lower == "default" {
        return None;
    }
    let mapped = match lower.as_str() {
        "max" => "xhigh",
        "minimal" => "low",
        other => other,
    };
    const VALID: [&str; 16] = [
        "low",
        "medium",
        "high",
        "xhigh",
        "tiered",
        "tiered-high",
        "tiered-xhigh",
        "sonar",
        "thinking",
        "lite",
        "opus-high",
        "opus-medium",
        "opus-low",
        "sonnet-high",
        "sonnet-medium",
        "sonnet-low",
    ];
    if VALID.contains(&mapped) {
        Some(mapped.to_string())
    } else {
        None
    }
}

pub fn resolve_group_and_tier_from_model(model_name: &str) -> Option<(String, Option<String>)> {
    let mut trimmed = model_name.trim();
    for pfx in ["novoid-core/", "anthropic/", "openai/"] {
        if trimmed.len() >= pfx.len() && trimmed[..pfx.len()].eq_ignore_ascii_case(pfx) {
            trimmed = &trimmed[pfx.len()..];
            break;
        }
    }

    if let Some((base, suffix)) = trimmed.split_once('#') {
        let base_key = base.trim().to_lowercase();
        if GROUP_KEYS.contains(&base_key.as_str()) {
            return Some((base_key, normalize_tier(suffix)));
        }
        return None;
    }

    let lower = trimmed.to_lowercase();
    if GROUP_KEYS.contains(&lower.as_str()) {
        return Some((lower, None));
    }

    for key in GROUP_KEYS {
        if let Some(suffix) = lower.strip_prefix(&format!("{key}-")) {
            if let Some(tier) = normalize_tier(suffix) {
                return Some((key.to_string(), Some(tier)));
            }
        }
    }

    None
}

pub fn classify_model_group(model_id: &str) -> Option<&'static str> {
    let lower = model_id.trim().to_lowercase();
    if lower.is_empty()
        || GROUP_KEYS.contains(&lower.as_str())
        || resolve_group_and_tier_from_model(&lower).is_some()
        || lower.ends_with("-disabled")
        || is_image_only_model(&lower)
    {
        return None;
    }

    if lower.contains("claude") {
        return Some("novoid-claude");
    }
    if is_group_4_variant(&lower, "gemini-4-neon") {
        return Some("novoid-neon");
    }
    if is_group_4_variant(&lower, "gemini-4-hydrogen") {
        return Some("novoid-hydrogen");
    }
    if is_group_4_variant(&lower, "gemini-4-carbon") {
        return Some("novoid-carbon");
    }
    if is_group_4_variant(&lower, "gemini-4-argon") {
        return Some("novoid-argon");
    }
    if lower == "gemini-2.5-flash" || lower == "gemini-3-flash" {
        return Some("novoid-default");
    }
    if lower.contains("flash") || is_group_4_variant(&lower, "gemini-2.5") {
        return Some("novoid-flash");
    }
    Some("novoid-default")
}

fn get_family_rank(lower: &str) -> usize {
    FAMILY_PRIORITY
        .iter()
        .position(|p| lower.contains(p))
        .unwrap_or(999)
}

pub fn get_model_tier_tag(model_id: &str) -> Option<&'static str> {
    static RULES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let rules = RULES.get_or_init(|| {
        vec![
            (Regex::new(r"(?:^|[-_.])tiered[-_.]xhigh(?:$|[-_.])").unwrap(), "tiered-xhigh"),
            (Regex::new(r"(?:^|[-_.])tiered[-_.]high(?:$|[-_.])").unwrap(), "tiered-high"),
            (Regex::new(r"(?:^|[-_.])tiered(?:$|[-_.])").unwrap(), "tiered"),
            (Regex::new(r"(?:^|[-_.])xhigh(?:$|[-_.])").unwrap(), "xhigh"),
            (Regex::new(r"(?:^|[-_.])high(?:$|[-_.])").unwrap(), "high"),
            (Regex::new(r"(?:^|[-_.])medium(?:$|[-_.])").unwrap(), "medium"),
            (Regex::new(r"(?:^|[-_.])low(?:$|[-_.])").unwrap(), "low"),
            (Regex::new(r"(?:^|[-_.])sonar(?:$|[-_.])").unwrap(), "sonar"),
            (Regex::new(r"(?:^|[-_.])thinking(?:$|[-_.])").unwrap(), "thinking"),
            (Regex::new(r"(?:^|[-_.])lite(?:$|[-_.])").unwrap(), "lite"),
        ]
    });
    let lower = model_id.to_lowercase();
    for (re, tag) in rules {
        if re.is_match(&lower) {
            return Some(*tag);
        }
    }
    None
}

fn get_default_tier_rank(lower: &str) -> usize {
    match get_model_tier_tag(lower) {
        Some("xhigh") => 0,
        Some("high") => 1,
        Some("medium") => 2,
        Some("low") => 3,
        Some("tiered-xhigh") => 4,
        Some("tiered-high") => 5,
        Some("tiered") => 6,
        Some("sonar") => 7,
        Some("thinking") => 8,
        Some("lite") => 9,
        _ => 99,
    }
}

pub fn sort_by_priority(list: &[String]) -> Vec<String> {
    let mut out = list.to_vec();
    out.sort_by(|a, b| {
        let la = a.to_lowercase();
        let lb = b.to_lowercase();
        get_family_rank(&la)
            .cmp(&get_family_rank(&lb))
            .then_with(|| get_default_tier_rank(&la).cmp(&get_default_tier_rank(&lb)))
            .then_with(|| la.cmp(&lb))
    });
    out
}

fn tier_preference_order(tier: &str) -> &'static [&'static str] {
    match tier {
        "xhigh" => &["xhigh", "tiered-xhigh", "high", "tiered-high", "tiered", "medium", "low", "sonar", "thinking", "lite"],
        "high" => &["high", "tiered-high", "xhigh", "tiered-xhigh", "tiered", "medium", "low", "sonar", "thinking", "lite"],
        "medium" => &["medium", "tiered", "high", "low", "tiered-high", "xhigh", "tiered-xhigh", "sonar", "thinking", "lite"],
        "low" => &["low", "lite", "medium", "tiered", "high", "tiered-high", "xhigh", "tiered-xhigh", "sonar", "thinking"],
        "tiered-xhigh" => &["tiered-xhigh", "xhigh", "tiered-high", "high", "tiered", "medium", "low", "sonar", "thinking", "lite"],
        "tiered-high" => &["tiered-high", "high", "tiered-xhigh", "xhigh", "tiered", "medium", "low", "sonar", "thinking", "lite"],
        "tiered" => &["tiered", "tiered-high", "tiered-xhigh", "high", "medium", "low", "lite", "thinking", "sonar"],
        "sonar" => &["sonar", "medium", "high", "low", "xhigh", "tiered-high", "tiered-xhigh", "tiered", "thinking", "lite"],
        "thinking" => &["thinking", "high", "tiered", "medium", "low", "lite", "tiered-high", "xhigh", "tiered-xhigh", "sonar"],
        "lite" => &["lite", "low", "medium", "tiered", "high", "thinking", "tiered-high", "xhigh", "tiered-xhigh", "sonar"],
        _ => &[],
    }
}

pub fn sort_pool_for_tier(pool: &[String], group_key: &str, tier: Option<&str>) -> Vec<String> {
    let Some(t) = tier else {
        return pool.to_vec();
    };

    if group_key == "novoid-claude" {
        let (target_sub, effort) = if let Some(rest) = t.strip_prefix("opus-") {
            ("opus", rest)
        } else if let Some(rest) = t.strip_prefix("sonnet-") {
            ("sonnet", rest)
        } else if t == "low" || t == "medium" {
            ("sonnet", t)
        } else {
            ("opus", t)
        };
        let order = tier_preference_order(effort);
        let mut out = pool.to_vec();
        out.sort_by(|a, b| {
            let la = a.to_lowercase();
            let lb = b.to_lowercase();
            let a_fam = if la.contains(target_sub) { 0 } else { 1 };
            let b_fam = if lb.contains(target_sub) { 0 } else { 1 };
            let rank_a = get_model_tier_tag(&la)
                .and_then(|tag| order.iter().position(|&o| o == tag))
                .unwrap_or(99);
            let rank_b = get_model_tier_tag(&lb)
                .and_then(|tag| order.iter().position(|&o| o == tag))
                .unwrap_or(99);
            a_fam.cmp(&b_fam).then_with(|| rank_a.cmp(&rank_b))
        });
        return out;
    }

    let order = tier_preference_order(t);
    if order.is_empty() {
        return pool.to_vec();
    }
    let mut out = pool.to_vec();
    out.sort_by(|a, b| {
        let rank_a = get_model_tier_tag(a)
            .and_then(|tag| order.iter().position(|&o| o == tag))
            .unwrap_or(99);
        let rank_b = get_model_tier_tag(b)
            .and_then(|tag| order.iter().position(|&o| o == tag))
            .unwrap_or(99);
        rank_a.cmp(&rank_b)
    });
    out
}

fn tier_from_thinking_budget(budget: u64) -> &'static str {
    if budget <= 6_000 {
        "low"
    } else if budget <= 16_000 {
        "medium"
    } else if budget <= 28_000 {
        "high"
    } else {
        "xhigh"
    }
}

pub fn extract_requested_tier(headers: &HeaderMap, body: &Value) -> Option<String> {
    if let Some(h) = headers.get("x-novoid-tier").and_then(|v| v.to_str().ok()) {
        if let Some(t) = normalize_tier(h) {
            return Some(t);
        }
    }

    if let Some(model_str) = body.get("model").and_then(|v| v.as_str()) {
        if let Some((_, Some(t))) = resolve_group_and_tier_from_model(model_str) {
            return Some(t);
        }
    }

    let effort = body
        .get("output_config")
        .and_then(|v| v.get("effort"))
        .and_then(|v| v.as_str())
        .or_else(|| body.get("reasoning_effort").and_then(|v| v.as_str()))
        .or_else(|| {
            body.get("reasoning")
                .and_then(|v| v.get("effort"))
                .and_then(|v| v.as_str())
        });
    if let Some(eff) = effort {
        if let Some(t) = normalize_tier(eff) {
            return Some(t);
        }
    }

    if body.get("thinking").and_then(|t| t.get("type")).and_then(|v| v.as_str()) == Some("enabled")
    {
        if let Some(budget) = body
            .get("thinking")
            .and_then(|t| t.get("budget_tokens"))
            .and_then(|v| v.as_u64())
        {
            return Some(tier_from_thinking_budget(budget).to_string());
        }
    }

    None
}

fn apply_tier_thinking_if_needed(payload: &mut Value, target_model: &str, tier: Option<&str>) {
    let Some(t) = tier else {
        return;
    };
    if payload.get("thinking").is_some() {
        return;
    }
    if let Some(tag) = get_model_tier_tag(target_model) {
        if tag != "thinking" {
            return;
        }
    }

    let base_effort = t.rsplit('-').next().unwrap_or(t);
    let budget = match base_effort {
        "low" => Some(4096u64),
        "medium" => Some(12000),
        "high" => Some(24576),
        "xhigh" => Some(32000),
        _ => None,
    };
    let Some(b) = budget else {
        return;
    };

    let lower = target_model.to_lowercase();
    let is_thinking_capable = (lower.contains("claude") && lower.contains("thinking"))
        || lower.contains("gemini-2.5")
        || lower.contains("gemini-3")
        || lower.contains("gemini-4");
    if is_thinking_capable {
        payload["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": b
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailureClassification {
    pub is_fatal_pool_error: bool,
    pub is_unsupported_model: bool,
    pub is_quota_or_rate_limit: bool,
    pub is_server_error: bool,
}

pub fn classify_upstream_failure(status: u16, body: &str) -> FailureClassification {
    let lower = body.to_lowercase();
    let is_fatal_pool_error = status == 401
        || status == 403
        || lower.contains("no accounts available")
        || lower.contains("all accounts are invalid")
        || lower.contains("invalid or missing api key");

    if is_fatal_pool_error {
        return FailureClassification {
            is_fatal_pool_error: true,
            is_unsupported_model: false,
            is_quota_or_rate_limit: false,
            is_server_error: false,
        };
    }

    let is_unsupported_model = (status == 400 || status == 404)
        && (lower.contains("model_not_supported")
            || lower.contains("invalid model")
            || lower.contains("not found")
            || lower.contains("not supported")
            || lower.contains("unknown model"));

    let is_quota_or_rate_limit = status == 429
        || (status == 400
            && (lower.contains("resource_exhausted")
                || lower.contains("quota_exhausted")
                || lower.contains("capacity_exhausted")
                || lower.contains("rate_limit_error")
                || lower.contains("exhausted your capacity")
                || lower.contains("rate limited")));

    let is_server_error = status >= 500;

    FailureClassification {
        is_fatal_pool_error: false,
        is_unsupported_model,
        is_quota_or_rate_limit,
        is_server_error,
    }
}

pub enum DispatchResult {
    UpstreamOk {
        response: reqwest::Response,
        target_model: String,
    },
    UpstreamRawError {
        status: u16,
        body: String,
    },
    RouterError {
        status: u16,
        body: Value,
    },
}

struct RouterInner {
    groups: HashMap<String, Vec<String>>,
    group_index: HashMap<String, usize>,
    tier_index: HashMap<String, usize>,
    cached_models_json: Option<Value>,
    last_refresh_at: u64,
}

pub struct ModelRouter {
    pub upstream_url: String,
    pub client: Client,
    refresh_lock: Mutex<()>,
    inner: RwLock<RouterInner>,
}

impl ModelRouter {
    pub fn new(upstream_url: String) -> Arc<Self> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .tcp_keepalive(Duration::from_secs(30))
            .tcp_nodelay(true)
            .http2_adaptive_window(true)
            .http2_keep_alive_interval(Some(Duration::from_secs(20)))
            .http2_keep_alive_timeout(Duration::from_secs(10))
            .http2_keep_alive_while_idle(true)
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(32)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_else(|_| Client::new());

        let mut groups = HashMap::new();
        let mut group_index = HashMap::new();
        for key in GROUP_KEYS {
            groups.insert(key.to_string(), Vec::new());
            group_index.insert(key.to_string(), 0);
        }

        Arc::new(Self {
            upstream_url: upstream_url.trim_end_matches('/').to_string(),
            client,
            refresh_lock: Mutex::new(()),
            inner: RwLock::new(RouterInner {
                groups,
                group_index,
                tier_index: HashMap::new(),
                cached_models_json: None,
                last_refresh_at: 0,
            }),
        })
    }

    pub async fn get_groups_snapshot(&self) -> HashMap<String, Vec<String>> {
        self.inner.read().await.groups.clone()
    }

    pub async fn get_or_refresh_models(self: &Arc<Self>, force: bool) -> Option<Value> {
        if !force {
            let guard = self.inner.read().await;
            if guard.cached_models_json.is_some() && guard.groups.values().any(|v| !v.is_empty()) {
                let stale =
                    now_ms().saturating_sub(guard.last_refresh_at) > MODELS_REFRESH_TTL_MS;
                let cached = guard.cached_models_json.clone();
                drop(guard);
                if stale {
                    let bg = Arc::clone(self);
                    tokio::spawn(async move {
                        bg.refresh_models(false).await;
                    });
                }
                return cached;
            }
        }
        self.refresh_models(force).await
    }

    pub async fn refresh_models(&self, force: bool) -> Option<Value> {
        if !force {
            let guard = self.inner.read().await;
            if now_ms().saturating_sub(guard.last_refresh_at) < MODELS_REFRESH_TTL_MS
                && guard.groups.values().any(|v| !v.is_empty())
            {
                return guard.cached_models_json.clone();
            }
        }
        let _guard = self.refresh_lock.lock().await;
        if !force {
            let guard = self.inner.read().await;
            if now_ms().saturating_sub(guard.last_refresh_at) < MODELS_REFRESH_TTL_MS
                && guard.groups.values().any(|v| !v.is_empty())
            {
                return guard.cached_models_json.clone();
            }
        }

        let url = if force {
            format!("{}/v1/models?refresh=true", self.upstream_url)
        } else {
            format!("{}/v1/models", self.upstream_url)
        };

        let resp = match self
            .client
            .get(&url)
            .timeout(Duration::from_secs(MODELS_FETCH_TIMEOUT_SECS))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                eprintln!("[NovoidProxy] Loi lay /v1/models: HTTP {}", r.status());
                return self.inner.read().await.cached_models_json.clone();
            }
            Err(err) => {
                eprintln!("[NovoidProxy] Loi lay /v1/models: {err}");
                return self.inner.read().await.cached_models_json.clone();
            }
        };

        let json_val: Value = match resp.json().await {
            Ok(v) => v,
            Err(err) => {
                eprintln!("[NovoidProxy] Loi parse JSON /v1/models: {err}");
                return self.inner.read().await.cached_models_json.clone();
            }
        };

        let mut seen = HashSet::new();
        let mut next_groups: HashMap<String, Vec<String>> = GROUP_KEYS
            .iter()
            .map(|k| (k.to_string(), Vec::new()))
            .collect();

        if let Some(arr) = json_val.get("data").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    if !id.is_empty() && seen.insert(id.to_string()) {
                        if let Some(gk) = classify_model_group(id) {
                            if let Some(list) = next_groups.get_mut(gk) {
                                list.push(id.to_string());
                            }
                        }
                    }
                }
            }
        }

        {
            let mut inner = self.inner.write().await;
            for key in GROUP_KEYS {
                let prev_idx = *inner.group_index.get(key).unwrap_or(&0);
                let prev_model = inner
                    .groups
                    .get(key)
                    .and_then(|v| v.get(prev_idx))
                    .cloned();
                let sorted = sort_by_priority(
                    next_groups.get(key).map(|v| v.as_slice()).unwrap_or(&[]),
                );
                let next_idx = prev_model
                    .and_then(|pm| sorted.iter().position(|m| m == &pm))
                    .unwrap_or(0);
                inner.groups.insert(key.to_string(), sorted);
                inner.group_index.insert(key.to_string(), next_idx);
            }
            inner.tier_index.clear();
            inner.cached_models_json = Some(json_val.clone());
            inner.last_refresh_at = now_ms();
        }

        Some(json_val)
    }

    pub async fn get_pool(&self, group_key: &str, tier: Option<&str>) -> Vec<String> {
        let inner = self.inner.read().await;
        let base = inner.groups.get(group_key).cloned().unwrap_or_default();
        sort_pool_for_tier(&base, group_key, tier)
    }

    async fn get_cursor(&self, group_key: &str, tier: Option<&str>, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let inner = self.inner.read().await;
        let raw = if let Some(t) = tier {
            let k = format!("{group_key}:{t}");
            *inner.tier_index.get(&k).unwrap_or(&0)
        } else {
            *inner.group_index.get(group_key).unwrap_or(&0)
        };
        raw % len
    }

    async fn set_cursor(&self, group_key: &str, tier: Option<&str>, val: usize) {
        let mut inner = self.inner.write().await;
        if let Some(t) = tier {
            inner.tier_index.insert(format!("{group_key}:{t}"), val);
        } else {
            inner.group_index.insert(group_key.to_string(), val);
        }
    }

    pub async fn dispatch_with_group_rotation(
        self: &Arc<Self>,
        group_key: &str,
        requested_model: &str,
        forward_headers: &HeaderMap,
        anth_payload: &Value,
        tier: Option<&str>,
    ) -> DispatchResult {
        let (is_empty, stale) = {
            let inner = self.inner.read().await;
            let empty = inner
                .groups
                .get(group_key)
                .map(|v| v.is_empty())
                .unwrap_or(true);
            let stale = now_ms().saturating_sub(inner.last_refresh_at) > MODELS_REFRESH_TTL_MS;
            (empty, stale)
        };

        if is_empty {
            self.refresh_models(false).await;
        } else if stale {
            let bg = Arc::clone(self);
            tokio::spawn(async move {
                bg.refresh_models(false).await;
            });
        }

        let current_len = {
            let inner = self.inner.read().await;
            inner.groups.get(group_key).map(|v| v.len()).unwrap_or(0)
        };

        if current_len == 0 {
            return DispatchResult::RouterError {
                status: 404,
                body: json!({
                    "type": "error",
                    "error": {
                        "type": "not_found_error",
                        "message": format!("Nhom '{requested_model}' hien khong co model nao kha dung.")
                    }
                }),
            };
        }

        let mut clean_headers = HeaderMap::new();
        for (k, v) in forward_headers {
            let lower = k.as_str().to_lowercase();
            if lower == "host"
                || lower == "content-length"
                || lower == "x-novoid-tier"
                || lower == "content-type"
                || lower == "x-novoid-no-fallback"
            {
                continue;
            }
            clean_headers.insert(k.clone(), v.clone());
        }
        clean_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        clean_headers.insert(
            HeaderName::from_static("x-novoid-no-fallback"),
            HeaderValue::from_static("1"),
        );

        let mut last_status: u16 = 429;
        let mut last_body = String::new();
        let mut saw_quota_or_server = false;
        let mut saw_unsupported = false;
        let mut tried_models: HashSet<String> = HashSet::new();

        for pass in 0..2 {
            if pass == 1 {
                if !saw_unsupported {
                    break;
                }
                let last_refresh = self.inner.read().await.last_refresh_at;
                if now_ms().saturating_sub(last_refresh) <= 5_000 {
                    break;
                }
                self.refresh_models(true).await;
            }

            let pool = self.get_pool(group_key, tier).await;
            if pool.is_empty() {
                continue;
            }

            for _ in 0..pool.len() {
                let idx = self.get_cursor(group_key, tier, pool.len()).await;
                let target_model = pool[idx].clone();

                if !tried_models.insert(target_model.clone()) {
                    self.set_cursor(group_key, tier, (idx + 1) % pool.len())
                        .await;
                    continue;
                }

                let mut final_payload = anth_payload.clone();
                final_payload["model"] = Value::String(target_model.clone());
                apply_tier_thinking_if_needed(&mut final_payload, &target_model, tier);

                let resp = match self
                    .client
                    .post(format!("{}/v1/messages", self.upstream_url))
                    .headers(clean_headers.clone())
                    .json(&final_payload)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(err) => {
                        last_status = 502;
                        last_body = err.to_string();
                        saw_quota_or_server = true;
                        let next_idx = (idx + 1) % pool.len();
                        self.set_cursor(group_key, tier, next_idx).await;
                        continue;
                    }
                };

                let status = resp.status().as_u16();
                if resp.status().is_success() {
                    self.set_cursor(group_key, tier, idx).await;
                    return DispatchResult::UpstreamOk {
                        response: resp,
                        target_model,
                    };
                }

                last_status = status;
                last_body = resp.text().await.unwrap_or_default();
                let cls = classify_upstream_failure(status, &last_body);

                if cls.is_fatal_pool_error
                    || (!cls.is_unsupported_model
                        && !cls.is_quota_or_rate_limit
                        && !cls.is_server_error)
                {
                    return DispatchResult::UpstreamRawError {
                        status: last_status,
                        body: last_body,
                    };
                }

                if cls.is_unsupported_model {
                    saw_unsupported = true;
                } else {
                    saw_quota_or_server = true;
                }

                let next_idx = (idx + 1) % pool.len();
                self.set_cursor(group_key, tier, next_idx).await;
                let reason = if cls.is_unsupported_model {
                    "Khong ho tro".to_string()
                } else {
                    format!("Het quota/Loi {last_status}")
                };
                let tier_tag = tier.map(|t| format!("#{t}")).unwrap_or_default();
                println!(
                    "[NovoidProxy] [{reason}] ({group_key}{tier_tag}) {target_model} -> Chuyen sang: {}",
                    pool[next_idx]
                );
            }
        }

        let pool_size = self.get_pool(group_key, tier).await.len();
        if saw_unsupported && !saw_quota_or_server {
            return DispatchResult::RouterError {
                status: 404,
                body: json!({
                    "type": "error",
                    "error": {
                        "type": "not_found_error",
                        "message": format!("Tat ca {pool_size} model trong nhom '{requested_model}' deu khong duoc tai khoan ho tro. Hay chuyen sang nhom khac trong CLI.")
                    }
                }),
            };
        }

        DispatchResult::RouterError {
            status: last_status,
            body: json!({
                "type": "error",
                "error": {
                    "type": "rate_limit_error",
                    "message": format!("Tat ca {pool_size} model trong nhom '{requested_model}' da het quota. Hay chuyen sang nhom khac trong CLI."),
                    "detail": last_body
                }
            }),
        }
    }
}
