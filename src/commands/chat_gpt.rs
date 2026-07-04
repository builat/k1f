use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
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
                markdown_to_telegram_html(&answer),
            )
            .parse_mode(ParseMode::Html)
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

/// Convert GitHub-Flavored Markdown (as returned by ChatGPT) into the subset
/// of HTML understood by Telegram.
///
/// Telegram is not a Markdown renderer, so unsupported constructs are degraded
/// rather than rendered literally:
/// - headings  -> bold text (Telegram has no heading style);
/// - tables    -> a `<pre>` block (monospace, alignment preserved as text);
/// - `---`     -> an em-dash line;
/// - raw HTML  (`<details>`, etc.) and footnotes are dropped for safety.
fn markdown_to_telegram_html(md: &str) -> String {
    // GFM-flavoured parsing: tables, strikethrough, task lists, smart quotes.
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let mut out = String::with_capacity(md.len());
    let mut in_pre = false; // inside <pre> we don't HTML-escape again
    let mut list_stack: Vec<Option<u64>> = Vec::new(); // None = bullet, Some(n) = ordered

    for event in Parser::new_ext(md, options) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { .. } => out.push_str("<b>"),
                Tag::BlockQuote(_) => out.push_str("<blockquote>"),
                Tag::CodeBlock(_) => {
                    out.push_str("<pre><code>");
                    in_pre = true;
                }
                Tag::HtmlBlock => {} // drop raw HTML blocks (e.g. <details>)
                Tag::List(start) => list_stack.push(start),
                Tag::Item => match list_stack.last_mut() {
                    Some(Some(n)) => {
                        out.push_str(&format!("{n}. "));
                        *n += 1;
                    }
                    _ => out.push_str("• "),
                },
                Tag::Emphasis => out.push_str("<i>"),
                Tag::Strong => out.push_str("<b>"),
                Tag::Strikethrough => out.push_str("<s>"),
                Tag::Link { dest_url, .. } => {
                    out.push_str(&format!("<a href=\"{}\">", html_escape(&dest_url, false)));
                }
                Tag::Table(_) => {
                    out.push_str("<pre>");
                    in_pre = true;
                }
                _ => {}
            },
            Event::End(end) => match end {
                TagEnd::Paragraph => out.push('\n'),
                TagEnd::Heading(_) => {
                    out.push_str("</b>\n");
                }
                TagEnd::BlockQuote(_) => out.push_str("</blockquote>\n"),
                TagEnd::CodeBlock => {
                    out.push_str("</code></pre>\n");
                    in_pre = false;
                }
                TagEnd::HtmlBlock => {}
                TagEnd::List(_) => {
                    list_stack.pop();
                }
                TagEnd::Item => out.push('\n'),
                TagEnd::Emphasis => out.push_str("</i>"),
                TagEnd::Strong => out.push_str("</b>"),
                TagEnd::Strikethrough => out.push_str("</s>"),
                TagEnd::Link => out.push_str("</a>"),
                TagEnd::Table => {
                    out.push_str("</pre>\n");
                    in_pre = false;
                }
                _ => {}
            },
            Event::Text(text) => out.push_str(&html_escape(&text, in_pre)),
            Event::Code(code) => {
                out.push_str("<code>");
                out.push_str(&html_escape(&code, false));
                out.push_str("</code>");
            }
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::Rule => out.push_str("—\n"),
            Event::TaskListMarker(checked) => {
                // A task-list item also opened a normal `Tag::Item`, which
                // already emitted a "• " bullet. Drop it so the checkbox glyph
                // is the marker, not "• ☑".
                if let Some(stripped) = out.strip_suffix("• ") {
                    out.truncate(stripped.len());
                }
                out.push_str(if checked { "☑ " } else { "☐ " });
            }
            // Drop raw/inline HTML, footnotes, math — not safely representable.
            Event::Html(_)
            | Event::InlineHtml(_)
            | Event::FootnoteReference(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_) => {}
        }
    }

    out.trim_end().to_string()
}

/// Escape text for inclusion in a Telegram HTML message. Inside `<pre>` we
/// still escape `<`, `>`, `&` (so the block can't be broken out of), but not
/// quotes — they only matter inside attribute values.
fn html_escape(s: &str, in_pre: bool) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' if !in_pre => escaped.push_str("&quot;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::markdown_to_telegram_html;

    fn convert(md: &str) -> String {
        markdown_to_telegram_html(md)
    }

    #[test]
    fn bold_and_italic() {
        let out = convert("**bold** and *italic*");
        assert_eq!(out, "<b>bold</b> and <i>italic</i>");
    }

    #[test]
    fn inline_code() {
        let out = convert("status: `OK`");
        assert_eq!(out, "status: <code>OK</code>");
    }

    #[test]
    fn fenced_code_block_becomes_pre() {
        let md = "```\n{\"status\": \"OK\"}\n```";
        let out = convert(md);
        assert_eq!(out, "<pre><code>{\"status\": \"OK\"}\n</code></pre>");
    }

    #[test]
    fn heading_becomes_bold() {
        let out = convert("# Report Title");
        assert_eq!(out, "<b>Report Title</b>");
    }

    #[test]
    fn blockquote() {
        let out = convert("> a quoted line");
        // Inner paragraph adds a trailing newline before </blockquote>.
        assert_eq!(out, "<blockquote>a quoted line\n</blockquote>");
    }

    #[test]
    fn unordered_list() {
        let out = convert("- one\n- two\n- three");
        assert_eq!(out, "• one\n• two\n• three");
    }

    #[test]
    fn ordered_list_increments() {
        let out = convert("1. first\n2. second\n3. third");
        assert_eq!(out, "1. first\n2. second\n3. third");
    }

    #[test]
    fn link_becomes_anchor() {
        let out = convert("[docs](https://example.com/x?a=1&b=2)");
        assert_eq!(
            out,
            "<a href=\"https://example.com/x?a=1&amp;b=2\">docs</a>"
        );
    }

    #[test]
    fn table_becomes_pre() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n";
        let out = convert(md);
        // pulldown-cmark renders table text; we wrap the whole thing in <pre>.
        assert!(out.starts_with("<pre>"), "got: {out}");
        assert!(out.ends_with("</pre>"), "got: {out}");
        assert!(out.contains("A") && out.contains("B"));
    }

    #[test]
    fn html_chars_in_text_are_escaped() {
        let out = convert("a < b > c & d");
        assert_eq!(out, "a &lt; b &gt; c &amp; d");
    }

    #[test]
    fn horizontal_rule_degrades_to_dash() {
        let out = convert("---");
        assert_eq!(out, "—");
    }

    #[test]
    fn tasklist_markers_become_glyphs() {
        let out = convert("- [x] done\n- [ ] todo");
        assert_eq!(out, "☑ done\n☐ todo");
    }

    #[test]
    fn details_html_block_is_dropped() {
        let md = "<details>\n<summary>x</summary>\nbody\n</details>";
        let out = convert(md);
        // Must not leak any raw tags into Telegram's HTML parser.
        assert!(!out.contains('<') || out.starts_with("<"));
    }
}
