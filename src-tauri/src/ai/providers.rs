//! HTTP calls to AI providers. The provider is chosen by the API key's
//! prefix; each provider is called through its own API. Requests run in the
//! backend so the key never reaches the UI.

use std::thread::sleep;
use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anthropic,
    OpenAI,
    OpenRouter,
    Gemini,
    Groq,
}

impl Provider {
    /// Recognizes a key by its prefix. Order matters: `sk-ant-` and `sk-or-`
    /// are more specific than OpenAI's `sk-`.
    pub fn detect(key: &str) -> Option<Provider> {
        let key = key.trim();
        if key.starts_with("sk-ant-") {
            Some(Provider::Anthropic)
        } else if key.starts_with("sk-or-") {
            Some(Provider::OpenRouter)
        } else if key.starts_with("sk-") {
            Some(Provider::OpenAI)
        } else if key.starts_with("AIza") {
            Some(Provider::Gemini)
        } else if key.starts_with("gsk_") {
            Some(Provider::Groq)
        } else {
            None
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic (Claude)",
            Provider::OpenAI => "OpenAI",
            Provider::OpenRouter => "OpenRouter",
            Provider::Gemini => "Google Gemini",
            Provider::Groq => "Groq",
        }
    }

    /// The model picked when a key is first added, if the key can use it.
    fn preferred_model(self) -> Option<&'static str> {
        match self {
            Provider::Anthropic => Some("claude-opus-5-5"),
            Provider::OpenRouter => Some("anthropic/claude-opus-5-5"),
            Provider::OpenAI | Provider::Gemini | Provider::Groq => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
}

pub struct Reply {
    pub text: String,
    pub usage: Usage,
    pub model: String,
}

/// What kind of answer is wanted.
pub enum Want<'a> {
    Text,
    /// JSON matching this schema.
    Json(&'a Value),
}

/// Start of the error for requests over a model's or plan's size limit, so
/// callers can retry with less context.
pub const TOO_LARGE: &str = "The request is too large for this model or plan.";

pub fn is_too_large(error: &str) -> bool {
    error.starts_with(TOO_LARGE)
}

const ANTHROPIC: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2026-10-21";
const OPENAI: &str = "https://api.openai.com/v1";
const OPENROUTER: &str = "https://openrouter.ai/api/v1";
const GROQ: &str = "https://api.groq.com/openai/v1";
const GEMINI: &str = "https://generativelanguage.googleapis.com/v1beta";

fn client() -> Result<Client, String> {
    Client::builder()
        // Long files at high effort can take minutes.
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())
}

/// Sends a request, retrying twice on rate limits, overload and network
/// errors, and turns failures into plain-language messages.
fn send(build: impl Fn() -> RequestBuilder) -> Result<Value, String> {
    let mut last = String::new();
    for attempt in 0..3 {
        if attempt > 0 {
            sleep(Duration::from_secs(2 << attempt));
        }
        let response = match build().send() {
            Ok(r) => r,
            Err(e) => {
                last = format!("Couldn't reach the AI provider: {e}");
                continue;
            }
        };
        let status = response.status().as_u16();
        let body: Value = response.json().unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(body);
        }
        let detail = body["error"]["message"]
            .as_str()
            .or_else(|| body["error"].as_str())
            .unwrap_or("")
            .to_string();
        // Some providers report a per-minute token limit as 413, others as
        // 429 with "too large" in the message; neither helps to retry as-is.
        let too_large = status == 413
            || detail.to_lowercase().contains("too large")
            || detail.contains("reduce your message size");
        let message = match status {
            _ if too_large => TOO_LARGE.to_string(),
            401 => "The API key was rejected. Check it in Settings.".to_string(),
            402 => "The provider reports a billing problem. Check your account's credit.".to_string(),
            403 => "This key isn't allowed to do that.".to_string(),
            404 => "That model isn't available to this key. Pick another in Settings.".to_string(),
            429 => "Rate limited by the provider. Try again in a moment.".to_string(),
            500..=599 => "The provider is busy or having problems. Try again in a moment.".to_string(),
            _ => format!("The provider returned an error ({status})."),
        };
        last = if detail.is_empty() { message } else { format!("{message} ({detail})") };
        if too_large || !(status == 429 || status >= 500) {
            break;
        }
    }
    Err(last)
}

// ---------------------------------------------------------------------------
// Models

pub fn list_models(provider: Provider, key: &str) -> Result<Vec<ModelInfo>, String> {
    let http = client()?;
    let mut models: Vec<ModelInfo> = match provider {
        Provider::Anthropic => {
            let body = send(|| {
                http.get(format!("{ANTHROPIC}/models?limit=1000"))
                    .header("x-api-key", key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
            })?;
            items(&body["data"])
                .map(|m| model(m["id"].as_str(), m["display_name"].as_str()))
                .collect()
        }
        Provider::OpenAI => {
            let body = send(|| http.get(format!("{OPENAI}/models")).bearer_auth(key))?;
            let mut list: Vec<ModelInfo> = items(&body["data"])
                .filter_map(|m| m["id"].as_str())
                .filter(|id| is_openai_chat_model(id))
                .map(|id| model(Some(id), None))
                .collect();
            list.sort_by(|a, b| b.id.cmp(&a.id));
            list
        }
        Provider::Groq => {
            let body = send(|| http.get(format!("{GROQ}/models")).bearer_auth(key))?;
            let mut list: Vec<ModelInfo> = items(&body["data"])
                .filter_map(|m| m["id"].as_str())
                .filter(|id| !id.contains("whisper") && !id.contains("tts") && !id.contains("guard"))
                .map(|id| model(Some(id), None))
                .collect();
            list.sort_by(|a, b| a.id.cmp(&b.id));
            list
        }
        Provider::OpenRouter => {
            // The model list is public, so check the key separately.
            send(|| http.get(format!("{OPENROUTER}/key")).bearer_auth(key))
                .or_else(|_| send(|| http.get(format!("{OPENROUTER}/auth/key")).bearer_auth(key)))?;
            let body = send(|| http.get(format!("{OPENROUTER}/models")).bearer_auth(key))?;
            items(&body["data"])
                .map(|m| model(m["id"].as_str(), m["name"].as_str()))
                .collect()
        }
        Provider::Gemini => {
            let body = send(|| http.get(format!("{GEMINI}/models?pageSize=1000")).header("x-goog-api-key", key))?;
            items(&body["models"])
                .filter(|m| {
                    m["supportedGenerationMethods"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|x| x == "generateContent"))
                })
                .map(|m| model(m["name"].as_str(), m["displayName"].as_str()))
                .collect()
        }
    };
    models.retain(|m| !m.id.is_empty());
    if let Some(preferred) = provider.preferred_model() {
        if let Some(i) = models.iter().position(|m| m.id == preferred) {
            let m = models.remove(i);
            models.insert(0, m);
        }
    }
    if models.is_empty() {
        return Err("This key works, but the provider listed no models it can use.".into());
    }
    Ok(models)
}

fn items(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

fn model(id: Option<&str>, name: Option<&str>) -> ModelInfo {
    let id = id.unwrap_or_default().to_string();
    ModelInfo {
        name: name.filter(|n| !n.is_empty()).unwrap_or(&id).to_string(),
        id,
    }
}

/// OpenAI lists every kind of model; keep the ones that chat.
fn is_openai_chat_model(id: &str) -> bool {
    let chat = id.starts_with("gpt-")
        || id.starts_with("chatgpt")
        || (id.starts_with('o') && id[1..].starts_with(|c: char| c.is_ascii_digit()));
    let other = ["audio", "realtime", "tts", "transcribe", "image", "embedding", "whisper", "dall", "search", "moderation"];
    chat && !other.iter().any(|o| id.contains(o))
}

// ---------------------------------------------------------------------------
// Claude feature support, by model family

fn claude_family(model: &str, families: &[&str]) -> bool {
    families.iter().any(|f| model.contains(f))
}

/// `output_config.effort`
fn claude_supports_effort(model: &str) -> bool {
    claude_family(model, &["fable", "mythos", "opus-5", "sonnet-5", "haiku-5", "opus-4-6", "opus-4-7", "opus-4-8", "sonnet-4-6", "opus-4-5"])
}

/// `output_config.format` (structured outputs)
fn claude_supports_json_schema(model: &str) -> bool {
    claude_family(
        model,
        &["fable", "mythos", "opus-5", "sonnet-5", "haiku-5", "opus-4-8", "haiku-4-5", "opus-4-5", "opus-4-1"],
    )
}

/// Server-side refusal fallback (`fallbacks: "default"`).
fn claude_supports_fallback(model: &str) -> bool {
    ["claude-fable-5-1", "claude-opus-5-5", "claude-opus-5", "claude-sonnet-5-5"].contains(&model)
}

// ---------------------------------------------------------------------------
// Completion

/// One request, one answer. `effort` is "low" | "medium" | "high"; it only
/// applies where the provider supports it.
pub fn complete(
    provider: Provider,
    key: &str,
    model: &str,
    system: &str,
    user: &str,
    want: &Want,
    effort: &str,
) -> Result<Reply, String> {
    let http = client()?;
    let schema = match want {
        Want::Json(s) => Some(*s),
        Want::Text => None,
    };
    // Where the provider can't enforce the schema, describe it in the prompt.
    let with_schema = |native: bool| match schema {
        Some(s) if !native => format!(
            "{user}\n\nReply with only a JSON object (no code fence) that matches this JSON Schema:\n{s}"
        ),
        _ => user.to_string(),
    };

    match provider {
        Provider::Anthropic => {
            let native = schema.is_some() && claude_supports_json_schema(model);
            let mut body = json!({
                "model": model,
                "max_tokens": 16000,
                "system": system,
                "messages": [{"role": "user", "content": with_schema(native)}],
            });
            let mut output_config = serde_json::Map::new();
            if claude_supports_effort(model) {
                output_config.insert("effort".into(), json!(effort));
            }
            if let (true, Some(s)) = (native, schema) {
                output_config.insert("format".into(), json!({"type": "json_schema", "schema": s}));
            }
            if !output_config.is_empty() {
                body["output_config"] = Value::Object(output_config);
            }
            let fallback = claude_supports_fallback(model);
            if fallback {
                body["fallbacks"] = json!("default");
            }
            let res = send(|| {
                let req = http
                    .post(format!("{ANTHROPIC}/messages"))
                    .header("x-api-key", key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
                    .json(&body);
                if fallback {
                    req.header("anthropic-beta", "server-side-fallback-2026-07-01")
                } else {
                    req
                }
            })?;
            match res["stop_reason"].as_str() {
                Some("refusal") => return Err(declined()),
                Some("max_tokens") => return Err(cut_off()),
                _ => {}
            }
            let text = items(&res["content"])
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            Ok(Reply {
                text,
                usage: Usage {
                    input: res["usage"]["input_tokens"].as_u64().unwrap_or(0),
                    output: res["usage"]["output_tokens"].as_u64().unwrap_or(0),
                },
                model: res["model"].as_str().unwrap_or(model).to_string(),
            })
        }
        Provider::OpenAI | Provider::OpenRouter | Provider::Groq => {
            let base = match provider {
                Provider::OpenAI => OPENAI,
                Provider::Groq => GROQ,
                _ => OPENROUTER,
            };
            let mut body = json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": with_schema(false)},
                ],
            });
            if schema.is_some() {
                body["response_format"] = json!({"type": "json_object"});
            }
            let res = send(|| {
                http.post(format!("{base}/chat/completions"))
                    .bearer_auth(key)
                    .header("X-Title", "GameX")
                    .json(&body)
            })?;
            let choice = &res["choices"][0];
            match choice["finish_reason"].as_str() {
                Some("length") => return Err(cut_off()),
                Some("content_filter") => return Err(declined()),
                _ => {}
            }
            Ok(Reply {
                text: choice["message"]["content"].as_str().unwrap_or_default().to_string(),
                usage: Usage {
                    input: res["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
                    output: res["usage"]["completion_tokens"].as_u64().unwrap_or(0),
                },
                model: res["model"].as_str().unwrap_or(model).to_string(),
            })
        }
        Provider::Gemini => {
            let mut body = json!({
                "systemInstruction": {"parts": [{"text": system}]},
                "contents": [{"role": "user", "parts": [{"text": with_schema(false)}]}],
            });
            if schema.is_some() {
                body["generationConfig"] = json!({"responseMimeType": "application/json"});
            }
            let name = if model.starts_with("models/") { model.to_string() } else { format!("models/{model}") };
            let res = send(|| {
                http.post(format!("{GEMINI}/{name}:generateContent"))
                    .header("x-goog-api-key", key)
                    .json(&body)
            })?;
            if res["promptFeedback"]["blockReason"].is_string() {
                return Err(declined());
            }
            let candidate = &res["candidates"][0];
            match candidate["finishReason"].as_str() {
                Some("MAX_TOKENS") => return Err(cut_off()),
                Some("SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST") => return Err(declined()),
                _ => {}
            }
            let text = items(&candidate["content"]["parts"])
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            Ok(Reply {
                text,
                usage: Usage {
                    input: res["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0),
                    output: res["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0),
                },
                model: model.to_string(),
            })
        }
    }
}

fn declined() -> String {
    "The model declined this request. Try rephrasing it, or pick another model in Settings.".into()
}

fn cut_off() -> String {
    "The answer was cut off because it got too long. Select a smaller part of the file and try again.".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_providers_from_keys() {
        assert_eq!(Provider::detect("sk-ant-api03-abc"), Some(Provider::Anthropic));
        assert_eq!(Provider::detect("  sk-or-v1-abc "), Some(Provider::OpenRouter));
        assert_eq!(Provider::detect("sk-proj-abc"), Some(Provider::OpenAI));
        assert_eq!(Provider::detect("AIzaSyAbc"), Some(Provider::Gemini));
        assert_eq!(Provider::detect("gsk_abc"), Some(Provider::Groq));
        assert_eq!(Provider::detect("hello"), None);
    }

    #[test]
    fn filters_openai_models() {
        assert!(is_openai_chat_model("gpt-5"));
        assert!(is_openai_chat_model("o3-mini"));
        assert!(!is_openai_chat_model("gpt-4o-realtime-preview"));
        assert!(!is_openai_chat_model("text-embedding-3-large"));
        assert!(!is_openai_chat_model("omni-moderation-latest"));
    }

    #[test]
    fn knows_claude_features() {
        assert!(claude_supports_effort("claude-opus-5-5"));
        assert!(claude_supports_json_schema("claude-haiku-5-5"));
        assert!(claude_supports_fallback("claude-opus-5-5"));
        assert!(!claude_supports_fallback("claude-haiku-5-5"));
        assert!(!claude_supports_json_schema("claude-3-haiku"));
    }
}
