use teloxide::prelude::*;
use teloxide::types::ParseMode::MarkdownV2;
use uuid::Uuid;

use crate::commands::bot_init::ChatRequest;

const MAX_UUIDS: u8 = 50;

pub struct UuidCmd<'cr> {
    pub chat_request: &'cr ChatRequest,
    pub qty: u8,
}

impl UuidCmd<'_> {
    pub fn new(chat_request: &ChatRequest, qty: Option<u8>) -> UuidCmd<'_> {
        UuidCmd {
            chat_request,
            qty: qty.unwrap_or(1),
        }
    }

    fn gen_uuid(&self, qty: u8) -> String {
        // Treat 0 as "one"; cap at MAX_UUIDS so the message stays readable.
        let count = qty.clamp(1, MAX_UUIDS) as usize;

        (0..count)
            .map(|idx| format!("{}\\.  `{}`", idx + 1, Uuid::new_v4()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub async fn respond(&self) -> Result<Message, teloxide::RequestError> {
        self.chat_request
            .bot
            .send_message(self.chat_request.msg.chat.id, self.gen_uuid(self.qty))
            .parse_mode(MarkdownV2)
            .await
    }
}
