use crate::commands::bot_init::ChatRequest;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use teloxide::{prelude::*, types::ParseMode};
#[derive(Serialize)]

struct OpenAIRequest {
    model: String,
    messages: Vec<GptMessage>,
}

#[derive(Serialize)]
struct GptMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct OpenAIResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: String,
}

pub struct AskGpt<'cr, 'pr> {
    pub chat_request: &'cr ChatRequest,
    pub prompt: &'pr Option<String>,
}
fn escape_markdown_v2(text: &str) -> String {
    let special_chars = [
        '_', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    ];
    let mut escaped = String::with_capacity(text.len());

    for c in text.chars() {
        if special_chars.contains(&c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }

    escaped
}
fn format_for_markdown_v2(chatgpt_response: &str) -> String {
    let mut result = String::new();
    let mut in_code_block = false;

    for line in chatgpt_response
        .trim_matches('"')
        .replace("\\n", "\n")
        .replace("\\\"", "\"")
        .lines()
    {
        if line.trim_start().starts_with("```") {
            if !in_code_block {
                result.push_str("```\n");
                in_code_block = true;
            } else {
                result.push_str("```\n");
                in_code_block = false;
            }
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

impl AskGpt<'_, '_> {
    pub fn new<'cr, 'pr>(
        chat_request: &'cr ChatRequest,
        prompt: &'pr Option<String>,
    ) -> AskGpt<'cr, 'pr> {
        AskGpt {
            chat_request,
            prompt,
        }
    }

    pub async fn respond(&self) -> Result<Message, teloxide::RequestError> {
        let response = match self.prompt {
            Some(prompt) => self.ask_openai(prompt.clone()).await,
            None => Err(anyhow::anyhow!("No prompt provided")),
        };

        let escaped_response = response.unwrap_or(">Unexpected empty response<".to_string());
        self.chat_request
            .bot
            .send_message(
                self.chat_request.msg.chat.id,
                format_for_markdown_v2(&escaped_response),
            )
            .parse_mode(ParseMode::MarkdownV2)
            .await
            .map_err(|e| e.into())
    }

    pub async fn ask_openai(&self, prompt: String) -> Result<String, anyhow::Error> {
        let client = Client::new();
        let request = OpenAIRequest {
            model: "gpt-4.1".to_string(),
            messages: vec![
                GptMessage {
                    role: "user".to_string(),
                    content: prompt,
                },
                GptMessage {
                    role: "developer".to_string(),
                    content: "1. Act like a slightly ironic expert. 2. Skip the pleasantries—stick strictly to facts. 3. Flag any points you’re unsure about separately. 4. If the data are insufficient, say so. 5. Assume the questioner might be incompetent.".to_string(),
                },
            ],
        };
        let api_key = std::env::var("GPT_TOKEN")
            .expect("OPENAI_API_KEY must be set in the environment")
            .trim()
            .to_string();

        let res = client
            .post("https://api.openai.com/v1/chat/completions")
            .bearer_auth(api_key)
            .json(&request)
            .send()
            .await?;

        Ok(res
            .json::<OpenAIResponse>()
            .await?
            .choices
            .get(0)
            .map(|c| format!("[GPT]: {:?}", c.message.content.clone()))
            .unwrap_or("No response found".to_string()))
    }
}
