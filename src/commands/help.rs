use teloxide::prelude::*;

use crate::commands::bot_init::ChatRequest;

const HELP_MSG: &str = "\
/help — display this text.
/username — echo client id.
/gun N — generate up to 50 uuid v4 (one per line).
/gus — generate a single uuid v4.
/ping target — PING (8.8.8.8 | https://google.com | google.com)
/gpt question — ask ChatGPT
";

pub struct HelpCmd<'cr> {
    pub chat_request: &'cr ChatRequest,
}

impl HelpCmd<'_> {
    pub fn new(chat_request: &ChatRequest) -> HelpCmd<'_> {
        HelpCmd { chat_request }
    }

    pub async fn respond(&self) -> Result<Message, teloxide::RequestError> {
        self.chat_request
            .bot
            .send_message(self.chat_request.msg.chat.id, HELP_MSG)
            .await
    }
}
