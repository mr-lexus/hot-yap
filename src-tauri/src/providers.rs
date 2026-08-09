use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use reqwest::multipart::{Form, Part};
use reqwest::{Client, Response, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const KEYRING_SERVICE: &str = "com.voxshift.app";
const DEFAULT_PROMPT: &str = "Correct punctuation and casing in this speech transcript. Preserve the original wording, language switches, technical terms, code, paths, and commands. Return only the corrected transcript.";

pub const PROVIDER_IDS: &[&str] = &[
    "openai",
    "deepgram",
    "groq",
    "elevenlabs",
    "assemblyai",
    "gemini",
    "openrouter",
    "anthropic",
    "xai",
    "bedrock",
    "ollama",
    "lmstudio",
];

pub const STT_PROVIDER_IDS: &[&str] = &[
    "local",
    "openai",
    "deepgram",
    "groq",
    "elevenlabs",
    "assemblyai",
    "gemini",
];

pub const TEXT_PROVIDER_IDS: &[&str] = &[
    "none",
    "openai",
    "groq",
    "openrouter",
    "anthropic",
    "gemini",
    "xai",
    "bedrock",
    "ollama",
    "lmstudio",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub endpoint: String,
    pub stt_model: String,
    pub text_model: String,
    #[serde(default)]
    pub api_key_set: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderSettings {
    pub stt_provider: String,
    pub text_provider: String,
    pub postprocess_prompt: String,
    pub providers: HashMap<String, ProviderConfig>,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert("openai".into(), config("https://api.openai.com/v1", "gpt-4o-mini-transcribe", "gpt-4.1-mini"));
        providers.insert("deepgram".into(), config("https://api.deepgram.com/v1", "nova-3", ""));
        providers.insert("groq".into(), config("https://api.groq.com/openai/v1", "whisper-large-v3-turbo", "llama-3.1-8b-instant"));
        providers.insert("elevenlabs".into(), config("https://api.elevenlabs.io/v1", "scribe_v2", ""));
        providers.insert("assemblyai".into(), config("https://api.assemblyai.com/v2", "universal-3-5-pro", ""));
        providers.insert("gemini".into(), config("https://generativelanguage.googleapis.com/v1beta", "gemini-2.5-flash", "gemini-2.5-flash"));
        providers.insert("openrouter".into(), config("https://openrouter.ai/api/v1", "", "openai/gpt-4.1-mini"));
        providers.insert("anthropic".into(), config("https://api.anthropic.com/v1", "", "claude-haiku-4-5-20251001"));
        providers.insert("xai".into(), config("https://api.x.ai/v1", "", "grok-4.3"));
        providers.insert("bedrock".into(), config("https://bedrock-runtime.us-east-1.amazonaws.com", "", "us.amazon.nova-2-lite-v1:0"));
        providers.insert("ollama".into(), config("http://127.0.0.1:11434", "", "qwen2.5:7b"));
        providers.insert("lmstudio".into(), config("http://127.0.0.1:1234/v1", "", "local-model"));
        Self {
            stt_provider: "local".into(),
            text_provider: "none".into(),
            postprocess_prompt: DEFAULT_PROMPT.into(),
            providers,
        }
    }
}

fn config(endpoint: &str, stt_model: &str, text_model: &str) -> ProviderConfig {
    ProviderConfig {
        endpoint: endpoint.into(),
        stt_model: stt_model.into(),
        text_model: text_model.into(),
        api_key_set: false,
    }
}

pub fn load_settings(path: &Path) -> ProviderSettings {
    let mut settings = std::fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str::<ProviderSettings>(&contents).ok())
        .unwrap_or_default();
    normalize(&mut settings);
    refresh_secret_statuses(&mut settings);
    settings
}

pub fn normalize(settings: &mut ProviderSettings) {
    let defaults = ProviderSettings::default();
    if !STT_PROVIDER_IDS.contains(&settings.stt_provider.as_str()) {
        settings.stt_provider = "local".into();
    }
    if !TEXT_PROVIDER_IDS.contains(&settings.text_provider.as_str()) {
        settings.text_provider = "none".into();
    }
    if settings.postprocess_prompt.trim().is_empty() {
        settings.postprocess_prompt = DEFAULT_PROMPT.into();
    } else {
        settings.postprocess_prompt = settings.postprocess_prompt.trim().to_string();
    }
    for (id, default_config) in defaults.providers {
        let saved = settings.providers.entry(id.clone()).or_insert_with(|| default_config.clone());
        if saved.endpoint.trim().is_empty() {
            saved.endpoint = default_config.endpoint;
        }
        if saved.stt_model.trim().is_empty() && !default_config.stt_model.is_empty() {
            saved.stt_model = default_config.stt_model;
        }
        if saved.text_model.trim().is_empty() && !default_config.text_model.is_empty() {
            saved.text_model = default_config.text_model;
        }
        if id == "anthropic" && saved.text_model == "claude-3-5-haiku-latest" {
            saved.text_model = "claude-haiku-4-5-20251001".into();
        }
        if id == "xai" && saved.text_model == "grok-3-mini" {
            saved.text_model = "grok-4.3".into();
        }
        if id == "bedrock" && saved.text_model == "us.amazon.nova-lite-v1:0" {
            saved.text_model = "us.amazon.nova-2-lite-v1:0".into();
        }
        saved.endpoint = saved.endpoint.trim_end_matches('/').trim().to_string();
        saved.stt_model = saved.stt_model.trim().to_string();
        saved.text_model = saved.text_model.trim().to_string();
    }
    settings.providers.retain(|id, _| PROVIDER_IDS.contains(&id.as_str()));
}

pub fn validate(settings: &ProviderSettings) -> Result<(), String> {
    if !STT_PROVIDER_IDS.contains(&settings.stt_provider.as_str()) {
        return Err("Unknown speech-to-text provider".into());
    }
    if !TEXT_PROVIDER_IDS.contains(&settings.text_provider.as_str()) {
        return Err("Unknown text-processing provider".into());
    }
    if settings.postprocess_prompt.len() > 4000 {
        return Err("Post-processing instruction is too long".into());
    }
    for (id, config) in &settings.providers {
        validate_endpoint(id, &config.endpoint)?;
    }
    if settings.stt_provider != "local" {
        let selected = selected_config(settings, &settings.stt_provider)?;
        if selected.stt_model.is_empty() {
            return Err("The selected speech-to-text model is empty".into());
        }
    }
    if settings.text_provider != "none" {
        let selected = selected_config(settings, &settings.text_provider)?;
        if selected.text_model.is_empty() {
            return Err("The selected text-processing model is empty".into());
        }
    }
    Ok(())
}

fn validate_endpoint(provider: &str, endpoint: &str) -> Result<(), String> {
    let url = Url::parse(endpoint).map_err(|_| format!("Invalid API endpoint for {provider}"))?;
    match url.scheme() {
        "https" => Ok(()),
        "http" if matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1")) => Ok(()),
        "http" => Err(format!("Insecure remote HTTP endpoint is not allowed for {provider}")),
        _ => Err(format!("Unsupported endpoint scheme for {provider}")),
    }
}

pub fn persist_settings(path: &Path, settings: &ProviderSettings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(settings).map_err(|e| format!("Cannot encode provider settings: {e}"))?;
    let temporary = temporary_settings_path(path);
    std::fs::write(&temporary, json).map_err(|e| format!("Cannot save provider settings: {e}"))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("Cannot replace provider settings: {e}"))
}

fn temporary_settings_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_os_string();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    temporary.push(format!(".{nonce}.tmp"));
    PathBuf::from(temporary)
}

pub fn refresh_secret_statuses(settings: &mut ProviderSettings) {
    for (provider, config) in &mut settings.providers {
        config.api_key_set = secret_available(provider);
    }
}

pub fn secret_available(provider: &str) -> bool {
    if env_secret(provider).is_some() {
        return true;
    }
    keyring::Entry::new(KEYRING_SERVICE, &format!("{provider}-api-key"))
        .and_then(|entry| entry.get_password())
        .map(|secret| !secret.trim().is_empty())
        .unwrap_or(false)
}

pub fn provider_needs_key(provider: &str) -> bool {
    !matches!(provider, "local" | "none" | "ollama" | "lmstudio")
}

pub fn stt_ready(settings: &ProviderSettings, local_engine_ready: bool) -> bool {
    if settings.stt_provider == "local" {
        return local_engine_ready;
    }
    selected_config(settings, &settings.stt_provider)
        .map(|config| !config.endpoint.is_empty() && !config.stt_model.is_empty() && (!provider_needs_key(&settings.stt_provider) || config.api_key_set))
        .unwrap_or(false)
}

pub fn store_secret(provider: &str, secret: &str) -> Result<(), String> {
    if !PROVIDER_IDS.contains(&provider) {
        return Err("Unknown provider".into());
    }
    let value = secret.trim();
    if value.is_empty() {
        return Ok(());
    }
    keyring::Entry::new(KEYRING_SERVICE, &format!("{provider}-api-key"))
        .map_err(|e| format!("Cannot access the operating-system credential store: {e}"))?
        .set_password(value)
        .map_err(|e| format!("Cannot save the API key in the operating-system credential store: {e}"))
}

pub fn delete_secret(provider: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &format!("{provider}-api-key"))
        .map_err(|e| format!("Cannot access the operating-system credential store: {e}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Cannot delete the API key from the operating-system credential store: {e}")),
    }
}

fn secret_for(provider: &str) -> Result<String, String> {
    if let Some(secret) = env_secret(provider) {
        return Ok(secret);
    }
    keyring::Entry::new(KEYRING_SERVICE, &format!("{provider}-api-key"))
        .map_err(|e| format!("Cannot access the operating-system credential store: {e}"))?
        .get_password()
        .map_err(|e| match e {
            keyring::Error::NoEntry => format!("API key for {provider} is not configured"),
            other => format!("Cannot read the API key for {provider}: {other}"),
        })
}

fn env_secret(provider: &str) -> Option<String> {
    let name = match provider {
        "openai" => "OPENAI_API_KEY",
        "deepgram" => "DEEPGRAM_API_KEY",
        "groq" => "GROQ_API_KEY",
        "elevenlabs" => "ELEVENLABS_API_KEY",
        "assemblyai" => "ASSEMBLYAI_API_KEY",
        "gemini" => "GEMINI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "xai" => "XAI_API_KEY",
        "bedrock" => "AWS_BEARER_TOKEN_BEDROCK",
        "ollama" => "OLLAMA_API_KEY",
        "lmstudio" => "LM_API_TOKEN",
        _ => return None,
    };
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn selected_config<'a>(settings: &'a ProviderSettings, provider: &str) -> Result<&'a ProviderConfig, String> {
    settings.providers.get(provider).ok_or_else(|| format!("Missing settings for {provider}"))
}

fn client() -> Result<Client, String> {
    Client::builder()
        .user_agent("HotYap/0.1")
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|e| format!("Cannot initialize the API client: {e}"))
}

fn endpoint(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

async fn response_json(provider: &str, response: Response) -> Result<Value, String> {
    let status = response.status();
    let body = response.text().await.map_err(|e| format!("{provider} returned an unreadable response: {e}"))?;
    if !status.is_success() {
        let compact = body.chars().take(600).collect::<String>().replace(['\n', '\r'], " ");
        return Err(format!("{provider} API returned HTTP {}: {compact}", status.as_u16()));
    }
    serde_json::from_str(&body).map_err(|e| format!("{provider} returned invalid JSON: {e}"))
}

fn json_text(value: &Value, path: &[&str], provider: &str) -> Result<String, String> {
    let mut current = value;
    for part in path {
        current = current.get(*part).ok_or_else(|| format!("{provider} response did not contain a transcript"))?;
    }
    current.as_str().map(str::trim).filter(|text| !text.is_empty()).map(str::to_string)
        .ok_or_else(|| format!("{provider} returned an empty transcript"))
}

pub async fn transcribe(settings: &ProviderSettings, wav_path: &Path) -> Result<String, String> {
    let provider = settings.stt_provider.as_str();
    let config = selected_config(settings, provider)?;
    let audio = tokio::fs::read(wav_path).await.map_err(|e| format!("Cannot read recorded audio: {e}"))?;
    match provider {
        "openai" | "groq" => transcribe_openai_compatible(provider, config, audio).await,
        "deepgram" => transcribe_deepgram(config, audio).await,
        "elevenlabs" => transcribe_elevenlabs(config, audio).await,
        "assemblyai" => transcribe_assemblyai(config, audio).await,
        "gemini" => transcribe_gemini(config, audio).await,
        "local" => Err("Local transcription must use the local worker".into()),
        _ => Err(format!("Unsupported speech-to-text provider: {provider}")),
    }
}

async fn transcribe_openai_compatible(provider: &str, config: &ProviderConfig, audio: Vec<u8>) -> Result<String, String> {
    if audio.len() > 25 * 1024 * 1024 {
        return Err(format!("{provider} accepts direct audio uploads up to 25 MB"));
    }
    let key = secret_for(provider)?;
    let part = Part::bytes(audio).file_name("recording.wav").mime_str("audio/wav")
        .map_err(|e| format!("Cannot prepare audio upload: {e}"))?;
    let form = Form::new().part("file", part).text("model", config.stt_model.clone());
    let value = response_json(provider, client()?.post(endpoint(&config.endpoint, "/audio/transcriptions")).bearer_auth(key).multipart(form).send().await
        .map_err(|e| format!("Cannot reach {provider}: {e}"))?).await?;
    json_text(&value, &["text"], provider)
}

async fn transcribe_deepgram(config: &ProviderConfig, audio: Vec<u8>) -> Result<String, String> {
    let key = secret_for("deepgram")?;
    let value = response_json("Deepgram", client()?.post(endpoint(&config.endpoint, "/listen"))
        .query(&[("model", config.stt_model.as_str()), ("smart_format", "true"), ("language", "multi")])
        .header("Authorization", format!("Token {key}"))
        .header("Content-Type", "audio/wav")
        .body(audio).send().await.map_err(|e| format!("Cannot reach Deepgram: {e}"))?).await?;
    value.pointer("/results/channels/0/alternatives/0/transcript").and_then(Value::as_str)
        .map(str::trim).filter(|text| !text.is_empty()).map(str::to_string)
        .ok_or_else(|| "Deepgram returned an empty transcript".into())
}

async fn transcribe_elevenlabs(config: &ProviderConfig, audio: Vec<u8>) -> Result<String, String> {
    let key = secret_for("elevenlabs")?;
    let part = Part::bytes(audio).file_name("recording.wav").mime_str("audio/wav")
        .map_err(|e| format!("Cannot prepare audio upload: {e}"))?;
    let form = Form::new().part("file", part).text("model_id", config.stt_model.clone());
    let value = response_json("ElevenLabs", client()?.post(endpoint(&config.endpoint, "/speech-to-text"))
        .header("xi-api-key", key).multipart(form).send().await.map_err(|e| format!("Cannot reach ElevenLabs: {e}"))?).await?;
    json_text(&value, &["text"], "ElevenLabs")
}

async fn transcribe_assemblyai(config: &ProviderConfig, audio: Vec<u8>) -> Result<String, String> {
    let key = secret_for("assemblyai")?;
    let client = client()?;
    let uploaded = response_json("AssemblyAI", client.post(endpoint(&config.endpoint, "/upload"))
        .header("Authorization", &key).header("Content-Type", "application/octet-stream").body(audio).send().await
        .map_err(|e| format!("Cannot upload audio to AssemblyAI: {e}"))?).await?;
    let upload_url = uploaded.get("upload_url").and_then(Value::as_str).ok_or_else(|| "AssemblyAI did not return an upload URL".to_string())?;
    let mut speech_models = vec![config.stt_model.as_str()];
    if config.stt_model != "universal-2" {
        speech_models.push("universal-2");
    }
    let submitted = response_json("AssemblyAI", client.post(endpoint(&config.endpoint, "/transcript"))
        .header("Authorization", &key).json(&json!({
            "audio_url": upload_url,
            "speech_models": speech_models,
            "language_detection": true,
            "language_detection_options": {
                "expected_languages": ["ru", "en"],
                "fallback_language": "auto",
                "code_switching": true
            }
        })).send().await
        .map_err(|e| format!("Cannot start AssemblyAI transcription: {e}"))?).await?;
    let id = submitted.get("id").and_then(Value::as_str).ok_or_else(|| "AssemblyAI did not return a transcript ID".to_string())?.to_string();
    for _ in 0..300 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let value = response_json("AssemblyAI", client.get(endpoint(&config.endpoint, &format!("/transcript/{id}")))
            .header("Authorization", &key).send().await.map_err(|e| format!("Cannot poll AssemblyAI: {e}"))?).await?;
        match value.get("status").and_then(Value::as_str) {
            Some("completed") => return json_text(&value, &["text"], "AssemblyAI"),
            Some("error") => return Err(format!("AssemblyAI transcription failed: {}", value.get("error").and_then(Value::as_str).unwrap_or("unknown error"))),
            Some("queued" | "processing") => {}
            Some(status) => return Err(format!("AssemblyAI returned an unknown status: {status}")),
            None => return Err("AssemblyAI response did not contain a status".into()),
        }
    }
    Err("AssemblyAI transcription timed out".into())
}

async fn transcribe_gemini(config: &ProviderConfig, audio: Vec<u8>) -> Result<String, String> {
    if audio.len() > 14 * 1024 * 1024 {
        return Err("Gemini inline audio is limited to about 14 MB before base64 encoding".into());
    }
    let key = secret_for("gemini")?;
    let data = base64::engine::general_purpose::STANDARD.encode(audio);
    let body = json!({
        "contents": [{ "parts": [
            { "text": "Transcribe the speech exactly. Preserve Russian and English technical terms. Return only the transcript." },
            { "inline_data": { "mime_type": "audio/wav", "data": data } }
        ] }]
    });
    let value = response_json("Gemini", client()?.post(endpoint(&config.endpoint, &format!("/models/{}:generateContent", config.stt_model)))
        .header("x-goog-api-key", key).json(&body).send().await.map_err(|e| format!("Cannot reach Gemini: {e}"))?).await?;
    gemini_text(&value)
}

pub async fn postprocess(settings: &ProviderSettings, transcript: &str) -> Result<String, String> {
    let provider = settings.text_provider.as_str();
    if provider == "none" {
        return Ok(transcript.to_string());
    }
    let config = selected_config(settings, provider)?;
    let input = format!("{}\n\nTranscript:\n{}", settings.postprocess_prompt, transcript);
    match provider {
        "openai" | "groq" | "openrouter" | "xai" | "lmstudio" => postprocess_openai_compatible(provider, config, &input).await,
        "anthropic" => postprocess_anthropic(config, &input).await,
        "gemini" => postprocess_gemini(config, &input).await,
        "bedrock" => postprocess_bedrock(config, &input).await,
        "ollama" => postprocess_ollama(config, &input).await,
        _ => Err(format!("Unsupported text-processing provider: {provider}")),
    }
}

async fn postprocess_openai_compatible(provider: &str, config: &ProviderConfig, input: &str) -> Result<String, String> {
    let client = client()?;
    let mut request = client.post(endpoint(&config.endpoint, "/chat/completions"))
        .json(&json!({ "model": config.text_model, "messages": [{ "role": "user", "content": input }], "temperature": 0.1 }));
    if provider_needs_key(provider) || config.api_key_set {
        request = request.bearer_auth(secret_for(provider)?);
    }
    if provider == "openrouter" {
        request = request.header("X-OpenRouter-Title", "HotYap");
    }
    let value = response_json(provider, request.send().await.map_err(|e| format!("Cannot reach {provider}: {e}"))?).await?;
    value.pointer("/choices/0/message/content").and_then(Value::as_str)
        .map(str::trim).filter(|text| !text.is_empty()).map(str::to_string)
        .ok_or_else(|| format!("{provider} returned empty text"))
}

async fn postprocess_anthropic(config: &ProviderConfig, input: &str) -> Result<String, String> {
    let value = response_json("Anthropic", client()?.post(endpoint(&config.endpoint, "/messages"))
        .header("x-api-key", secret_for("anthropic")?)
        .header("anthropic-version", "2023-06-01")
        .json(&json!({ "model": config.text_model, "max_tokens": 2048, "messages": [{ "role": "user", "content": input }] }))
        .send().await.map_err(|e| format!("Cannot reach Anthropic: {e}"))?).await?;
    joined_text_parts(value.get("content"), "Anthropic")
}

async fn postprocess_gemini(config: &ProviderConfig, input: &str) -> Result<String, String> {
    let value = response_json("Gemini", client()?.post(endpoint(&config.endpoint, &format!("/models/{}:generateContent", config.text_model)))
        .header("x-goog-api-key", secret_for("gemini")?)
        .json(&json!({ "contents": [{ "parts": [{ "text": input }] }] }))
        .send().await.map_err(|e| format!("Cannot reach Gemini: {e}"))?).await?;
    gemini_text(&value)
}

async fn postprocess_bedrock(config: &ProviderConfig, input: &str) -> Result<String, String> {
    let value = response_json("Amazon Bedrock", client()?.post(endpoint(&config.endpoint, &format!("/model/{}/converse", config.text_model)))
        .bearer_auth(secret_for("bedrock")?)
        .json(&json!({ "messages": [{ "role": "user", "content": [{ "text": input }] }] }))
        .send().await.map_err(|e| format!("Cannot reach Amazon Bedrock: {e}"))?).await?;
    joined_text_parts(value.pointer("/output/message/content"), "Amazon Bedrock")
}

async fn postprocess_ollama(config: &ProviderConfig, input: &str) -> Result<String, String> {
    let client = client()?;
    let mut request = client.post(endpoint(&config.endpoint, "/api/chat"))
        .json(&json!({ "model": config.text_model, "messages": [{ "role": "user", "content": input }], "stream": false }));
    if config.api_key_set || env_secret("ollama").is_some() {
        request = request.bearer_auth(secret_for("ollama")?);
    }
    let value = response_json("Ollama", request.send().await.map_err(|e| format!("Cannot reach Ollama: {e}"))?).await?;
    json_text(&value, &["message", "content"], "Ollama")
}

fn gemini_text(value: &Value) -> Result<String, String> {
    joined_text_parts(value.pointer("/candidates/0/content/parts"), "Gemini")
}

fn joined_text_parts(parts: Option<&Value>, provider: &str) -> Result<String, String> {
    let text = parts.and_then(Value::as_array).map(|parts| {
        parts.iter().filter_map(|part| part.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("")
    }).unwrap_or_default();
    let text = text.trim();
    if text.is_empty() {
        Err(format!("{provider} returned empty text"))
    } else {
        Ok(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_every_requested_provider() {
        let settings = ProviderSettings::default();
        assert!(PROVIDER_IDS.iter().all(|provider| settings.providers.contains_key(*provider)));
        assert_eq!(settings.stt_provider, "local");
        assert_eq!(settings.text_provider, "none");
    }

    #[test]
    fn remote_plain_http_endpoints_are_rejected() {
        assert!(validate_endpoint("ollama", "http://127.0.0.1:11434").is_ok());
        assert!(validate_endpoint("lmstudio", "http://localhost:1234/v1").is_ok());
        assert!(validate_endpoint("openai", "http://example.com/v1").is_err());
    }

    #[test]
    fn cloud_stt_requires_key_metadata() {
        let mut settings = ProviderSettings::default();
        settings.stt_provider = "openai".into();
        assert!(!stt_ready(&settings, true));
        settings.providers.get_mut("openai").unwrap().api_key_set = true;
        assert!(stt_ready(&settings, false));
    }

    #[test]
    fn joins_text_blocks_from_provider_responses() {
        let parts = json!([{ "text": "Hello, " }, { "text": "world." }]);
        assert_eq!(joined_text_parts(Some(&parts), "test").unwrap(), "Hello, world.");
    }
}
