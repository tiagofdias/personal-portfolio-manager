//! Streaming client for any OpenAI-compatible chat endpoint. Defaults to a
//! local Ollama server, so the AI is free, private and works offline.
//! Runs on a short-lived worker thread; the UI never blocks.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiSettings {
    pub base_url: String,
    pub model: String,
    #[serde(default)]
    pub api_key: String,
    pub temperature: f32,
    /// How many past chat messages are sent with each request.
    pub history: usize,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:11434/v1".into(),
            model: "portfolio-analyst".into(),
            api_key: String::new(),
            temperature: 0.3,
            history: 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

pub enum AiEvent {
    Token(String),
    Done,
    Error(String),
}

pub struct AiJob {
    pub rx: Receiver<AiEvent>,
    cancel: Arc<AtomicBool>,
}

impl AiJob {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub const SYSTEM_PROMPT: &str = "You are Personal Portfolio Manager's analyst: a careful, honest, plain-spoken investment educator.
Rules:
- The FACTS block below was computed by a deterministic engine. Treat its numbers as correct and never invent prices, returns or holdings.
- Portfolios are described only in percentages. Never ask for amounts of money.
- Explain good AND bad points, ranked by importance, in simple language. Be concise.
- Match every recommendation to the investor's risk profile and goals. If the profile is incomplete, ask one or two short questions to complete it.
- Favour diversification, low costs, a broad core plus limited satellites, and a plan the person can stick with during crashes. Nobody can guarantee returns; say so when people ask for 'the best' portfolio, and optimise for the best risk-adjusted fit instead.
- When the user tells you something new about themselves (goals, age, horizon, fears), take it into account from then on.
- When you propose a new allocation, ALWAYS include it in exactly this machine-readable block (tickers or names, percentages summing to 100):
<portfolio>
VWCE 60
AGGH 30
CASH 10
</portfolio>
- You are not a licensed financial adviser; this is education, not personal financial advice. Mention it briefly once, not in every message.
- Reply in the user's language. Plain text with '-' bullets and **bold** only: no emoji, no tables.";

/// Full system prompt: rules + investor profile + engine facts. Shared by the
/// app and the fine-tuning dataset generator so training matches inference.
pub fn build_system(profile: &crate::profile::Profile, facts: &str) -> String {
    let level = profile
        .risk_level()
        .map(|l| format!("{} ({}/5). {}", l.label(), l.level(), l.description()))
        .unwrap_or_else(|| "Unknown - questionnaire not answered yet.".into());
    format!(
        "{SYSTEM_PROMPT}\n- When the user reveals a durable fact about themselves (age, goal, horizon, ethics, fears), also output it as a short self-contained phrase, e.g. <note>34 years old</note> or <note>wants to keep some NVIDIA</note>, so it is remembered.\n\nINVESTOR PROFILE ({}/{} questions answered): {level}\n{}\nFACTS:\n{facts}",
        profile.answered(),
        crate::profile::QUESTIONS.len(),
        profile.summary()
    )
}

/// Remove hidden reasoning that some local models (e.g. Qwen3) emit.
pub fn strip_thinking(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => return out, // still thinking
        }
    }
    out.push_str(rest);
    out.trim_start().to_string()
}

/// Extract the last `<portfolio>...</portfolio>` block, if any.
pub fn extract_proposal(s: &str) -> Option<String> {
    let start = s.rfind("<portfolio>")? + "<portfolio>".len();
    let end = s[start..].find("</portfolio>")? + start;
    Some(s[start..end].trim().to_string())
}

fn url(settings: &AiSettings, path: &str) -> String {
    format!("{}/{}", settings.base_url.trim_end_matches('/'), path)
}

pub fn start_chat(
    settings: &AiSettings,
    system: String,
    history: &[ChatMessage],
    wake: impl Fn() + Send + 'static,
) -> AiJob {
    let (tx, rx) = channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let c2 = cancel.clone();
    let settings = settings.clone();

    let mut messages = vec![serde_json::json!({ "role": "system", "content": system })];
    let skip = history.len().saturating_sub(settings.history.max(2));
    for m in &history[skip..] {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        messages.push(serde_json::json!({ "role": role, "content": m.content }));
    }
    let body = serde_json::json!({
        "model": settings.model,
        "messages": messages,
        "stream": true,
        "temperature": settings.temperature,
    });

    std::thread::Builder::new()
        .name("ai-chat".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            let res = stream(&settings, &body, &tx, &c2, &wake);
            let _ = tx.send(match res {
                Ok(()) => AiEvent::Done,
                Err(e) => AiEvent::Error(e),
            });
            wake();
        })
        .expect("spawn ai thread");
    AiJob { rx, cancel }
}

fn stream(
    settings: &AiSettings,
    body: &serde_json::Value,
    tx: &Sender<AiEvent>,
    cancel: &AtomicBool,
    wake: &dyn Fn(),
) -> Result<(), String> {
    let mut req = ureq::post(&url(settings, "chat/completions")).header("Accept", "text/event-stream");
    if !settings.api_key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {}", settings.api_key));
    }
    let resp = req.send_json(body).map_err(|e| friendly_error(settings, &e.to_string()))?;
    let reader = BufReader::new(resp.into_body().into_reader());

    #[derive(Deserialize)]
    struct Delta {
        content: Option<String>,
    }
    #[derive(Deserialize)]
    struct Choice {
        delta: Delta,
    }
    #[derive(Deserialize)]
    struct Chunk {
        choices: Vec<Choice>,
    }

    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let line = line.map_err(|e| e.to_string())?;
        let Some(data) = line.strip_prefix("data:") else { continue };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        if let Ok(chunk) = serde_json::from_str::<Chunk>(data) {
            if let Some(text) = chunk.choices.into_iter().next().and_then(|c| c.delta.content) {
                if !text.is_empty() && tx.send(AiEvent::Token(text)).is_err() {
                    return Ok(());
                }
                wake();
            }
        }
    }
    Ok(())
}

fn friendly_error(settings: &AiSettings, e: &str) -> String {
    if e.contains("onnect") || e.contains("refused") || e.contains("os error") {
        format!(
            "Could not reach the AI at {}.\nInstall the free Ollama app (ollama.com), then run:\n  ollama pull qwen3:4b\n  ollama create portfolio-analyst -f ai/Modelfile\nOr change the endpoint in Settings. ({e})",
            settings.base_url
        )
    } else if e.contains("404") {
        format!("Model \"{}\" not found on the server. Create it with `ollama create portfolio-analyst -f ai/Modelfile` or pick another model in Settings. ({e})", settings.model)
    } else {
        e.to_string()
    }
}

/// Lists available model names (blocking; call from a worker thread).
pub fn list_models(settings: &AiSettings) -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct M {
        id: String,
    }
    #[derive(Deserialize)]
    struct L {
        data: Vec<M>,
    }
    let mut req = ureq::get(&url(settings, "models"));
    if !settings.api_key.is_empty() {
        req = req.header("Authorization", &format!("Bearer {}", settings.api_key));
    }
    let l: L = req
        .call()
        .map_err(|e| friendly_error(settings, &e.to_string()))?
        .into_body()
        .read_json()
        .map_err(|e| e.to_string())?;
    Ok(l.data.into_iter().map(|m| m.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_think_blocks() {
        assert_eq!(strip_thinking("<think>hmm</think>\nHello"), "Hello");
        assert_eq!(strip_thinking("<think>still going"), "");
        assert_eq!(strip_thinking("plain"), "plain");
    }

    #[test]
    fn finds_last_proposal() {
        let s = "a <portfolio>X 1</portfolio> b <portfolio>\nVWCE 70\nAGGH 30\n</portfolio>";
        assert_eq!(extract_proposal(s).unwrap(), "VWCE 70\nAGGH 30");
    }
}
