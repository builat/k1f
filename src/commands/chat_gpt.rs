use reqwest::Client;
use serde::{Deserialize, Serialize};
use teloxide::{prelude::*, types::ParseMode};

use crate::commands::bot_init::ChatRequest;

#[derive(Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<GptMessage>,
}

#[derive(Serialize)]
struct GptMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: String,
}

/// Shape of an OpenAI error body, e.g. on a 404 for an unknown model:
/// `{"error": {"message": "...", "type": "...", "code": "..."}}`.
#[derive(Deserialize)]
struct OpenAiError {
    error: OpenAiErrorBody,
}

#[derive(Deserialize)]
struct OpenAiErrorBody {
    message: String,
}

const SYSTEM_PROMPT: &str = "\
1. Act like a slightly ironic expert.
2. Skip the pleasantries—stick strictly to facts.
3. Flag any points you’re unsure about separately.
4. If the data are insufficient, say so.
5. Assume the questioner might be incompetent.";

/// Model used when GPT_MODEL is not set. Kept current with OpenAI's lineup;
/// gpt-4.1 was retired on 2026-02-13.
const DEFAULT_MODEL: &str = "gpt-5.5";

pub struct AskGpt<'cr, 'pr> {
    pub chat_request: &'cr ChatRequest,
    pub prompt: &'pr Option<String>,
}

impl<'cr, 'pr> AskGpt<'cr, 'pr> {
    pub fn new(chat_request: &'cr ChatRequest, prompt: &'pr Option<String>) -> AskGpt<'cr, 'pr> {
        AskGpt {
            chat_request,
            prompt,
        }
    }

    pub async fn respond(&self) -> Result<Message, teloxide::RequestError> {
        let answer = match self.prompt {
            Some(prompt) => self
                .ask_openai(prompt)
                .await
                .unwrap_or_else(|err| format!("Could not reach OpenAI: {err}")),
            None => "No prompt provided".to_string(),
        };

        self.chat_request
            .bot
            .send_message(
                self.chat_request.msg.chat.id,
                format_for_markdown_v2(&answer),
            )
            .parse_mode(ParseMode::MarkdownV2)
            .await
    }

    async fn ask_openai(&self, prompt: &str) -> Result<String, anyhow::Error> {
        let api_key = std::env::var("GPT_TOKEN")
            .expect("GPT_TOKEN must be set in the environment")
            .trim()
            .to_string();
        let model = std::env::var("GPT_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());

        let client = Client::new();
        let request = OpenAiRequest {
            model: model.clone(),
            messages: vec![
                GptMessage {
                    role: "user".to_string(),
                    content: prompt.to_string(),
                },
                GptMessage {
                    role: "developer".to_string(),
                    content: SYSTEM_PROMPT.to_string(),
                },
            ],
        };

        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(api_key)
            .json(&request)
            .send()
            .await?;

        // reqwest does not turn 4xx/5xx into Err by default, so a 404 for a
        // retired model would otherwise surface as a misleading "error
        // decoding response body". Check the status and surface OpenAI's own
        // error message instead.
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let detail = serde_json::from_str::<OpenAiError>(&body)
                .map(|e| e.error.message)
                .unwrap_or_else(|_| body.trim().to_string());
            anyhow::bail!("OpenAI {status}: {detail}");
        }

        let parsed = response.json::<OpenAiResponse>().await?;

        Ok(parsed
            .choices
            .into_iter()
            .next()
            .map(|c| format!("[GPT]: {}", c.message.content))
            .unwrap_or_else(|| "No response found".to_string()))
    }
}

/// Escape characters with special meaning in Telegram's MarkdownV2.
fn escape_markdown_v2(text: &str) -> String {
    const SPECIAL: &[char] = &[
        '\\', '*', '_', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.',
        '!',
    ];
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if SPECIAL.contains(&c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// Render a ChatGPT reply so that triple-fenced code blocks are passed through
/// verbatim and the surrounding prose is escaped for MarkdownV2.
fn format_for_markdown_v2(chatgpt_response: &str) -> String {
    let mut result = String::new();
    let mut in_code_block = false;

    for line in chatgpt_response.lines() {
        if line.trim_start().starts_with("```") {
            result.push_str("```\n");
            in_code_block = !in_code_block;
        } else if in_code_block {
            result.push_str(line);
            result.push('\n');
        } else {
            result.push_str(&escape_markdown_v2(line));
            result.push('\n');
        }
    }

    if in_code_block {
        result.push_str("```\n");
    }

    result
}
