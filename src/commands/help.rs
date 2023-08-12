use crate::commands::bot_init::ChatRequest;
use teloxide::prelude::*;

const HELP_MSG: &str = "
/help — display this text.
/username — echo client id.
/gun 1-9 — Generate uuid v4. Max - 9
/gus — Generate single uuid v4.
/ping — PING (8.8.8.8 | https://google.com | google.com)
";

pub struct HelpCmd<'cr> {
    pub chat_request: &'cr ChatRequest,
}

impl HelpCmd<'_> {
    pub fn new(chat_request: &ChatRequest) -> HelpCmd {
        HelpCmd { chat_request }
    }

    pub async fn respond(&self) -> Result<Message, teloxide::RequestError> {
        self.chat_request
            .bot
            .send_message(self.chat_request.msg.chat.id, HELP_MSG)
            .await
    }
}
