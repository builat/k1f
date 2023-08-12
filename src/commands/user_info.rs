use crate::commands::bot_init::ChatRequest;
use teloxide::prelude::*;

pub struct UserInfo<'cr> {
    pub chat_request: &'cr ChatRequest,
}

impl UserInfo<'_> {
    pub fn new(chat_request: &ChatRequest) -> UserInfo {
        UserInfo { chat_request }
    }

    pub async fn respond(&self) -> Result<Message, teloxide::RequestError> {
        self.chat_request
            .bot
            .send_message(self.chat_request.msg.chat.id, self.format_user_info())
            .await
    }

    fn format_user_info(&self) -> String {
        let msg = &self.chat_request.msg;
        format!(
            "UserId: {}\nUserName: {}\ndate: {}",
            msg.chat.id,
            String::from(msg.chat.first_name().unwrap_or("n/a")),
            msg.date
        )
    }
}
