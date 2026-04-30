use crate::error::{Result, SynthesisError};

pub struct LlmRequest<'a> {
    pub api_key: &'a str,
    pub model: &'a str,
    pub system: &'a str,
    pub user: &'a str,
    pub json_mode: bool,
    pub temperature: f32,
}

/// Call OpenRouter and return response text (code fences stripped).
/// When json_mode is true, retries once with stricter instruction on parse failure.
pub async fn call(req: LlmRequest<'_>) -> Result<String> {
    if req.api_key.is_empty() {
        return Err(SynthesisError::NoApiKey);
    }
    let client = reqwest::Client::new();
    do_request(&client, req.api_key, req.model, req.system, req.user, req.json_mode, req.temperature).await
}

/// Like call() but parses and returns serde_json::Value directly.
/// Retries with stricter prompt on first parse failure.
pub async fn call_json(
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<serde_json::Value> {
    if api_key.is_empty() {
        return Err(SynthesisError::NoApiKey);
    }
    let client = reqwest::Client::new();
    let raw = do_request(&client, api_key, model, system, user, true, 0.2).await?;

    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v) => Ok(v),
        Err(_) => {
            let retry_system = format!("{}\nReturn only valid JSON, no markdown", system);
            let raw2 = do_request(&client, api_key, model, &retry_system, user, true, 0.1).await?;
            serde_json::from_str::<serde_json::Value>(&raw2).map_err(|e| {
                SynthesisError::ParseError(e.to_string(), raw2[..raw2.len().min(300)].to_string())
            })
        }
    }
}

async fn do_request(
    client: &reqwest::Client,
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
    json_mode: bool,
    temperature: f32,
) -> Result<String> {
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user",   "content": user}
        ],
        "temperature": temperature,
    });
    if json_mode {
        body["response_format"] = serde_json::json!({"type": "json_object"});
    }

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .header("HTTP-Referer", "https://bamako.app")
        .header("X-Title", "Bamako")
        .json(&body)
        .send()
        .await
        .map_err(|e| SynthesisError::LlmError(format!("request failed: {}", e)))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(SynthesisError::LlmError(format!("OpenRouter {} — {}", status, text)));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| SynthesisError::LlmError(e.to_string()))?;

    let msg = &json["choices"][0]["message"];
    let raw = msg["content"].as_str().map(str::to_string)
        .or_else(|| msg["reasoning_content"].as_str().map(str::to_string))
        .or_else(|| msg["reasoning"].as_str().map(str::to_string))
        .or_else(|| {
            msg["reasoning_details"].as_array()?.iter()
                .find_map(|d| d["text"].as_str().map(str::to_string))
        })
        .ok_or_else(|| SynthesisError::LlmError(format!("no content in response: {}", json)))?;

    Ok(strip_code_fences(&raw))
}

pub fn strip_code_fences(s: &str) -> String {
    let trimmed = s.trim();
    if let Some(inner) = trimmed.strip_prefix("```json").or_else(|| trimmed.strip_prefix("```")) {
        if let Some(inner) = inner.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}
