use crate::commands::bot_init::ChatRequest;
use teloxide::prelude::*;
use teloxide::types::ParseMode::MarkdownV2;
use uuid::Uuid;

pub struct UuidCmd<'cr> {
    pub chat_request: &'cr ChatRequest,
    pub qty: u8,
}

impl UuidCmd<'_> {
    pub fn new(chat_request: &ChatRequest, qty: Option<u8>) -> UuidCmd {
        UuidCmd {
            chat_request,
            qty: qty.unwrap_or(1),
        }
    }

    fn gen_uuid(&self, qty: u8) -> String {
        let uuid_limit = match qty {
            0 => 1,
            x if x > 50 => 50,
            x => x
        };

        let mut uuids: Vec<(u8, Uuid)> = vec![];

        for idx in 0..uuid_limit {
            uuids.push((idx + 1, Uuid::new_v4()));
        }

        uuids
            .iter()
            .map(|(idx, u)| format!("{}\\.  `{}`", idx, u))
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
