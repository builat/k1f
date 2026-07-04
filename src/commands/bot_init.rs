use std::sync::LazyLock;

use teloxide::{prelude::*, types::InputFile, utils::command::BotCommands};

use crate::commands::{
    chat_gpt::AskGpt, help::HelpCmd, ping::PingCmd, user_info::UserInfo, uuid::UuidCmd,
};

#[derive(BotCommands, Clone, Debug)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
pub enum Command {
    Help,
    Username,
    GuS,
    GuN(u8),
    Ping(String),
    Gpt(String),
}

pub struct ChatRequest {
    pub bot: Bot,
    pub msg: Message,
}

static MASTER_TG_ID: LazyLock<i64> = LazyLock::new(|| {
    std::env::var("MASTER_TG_ID")
        .expect("MASTER_TG_ID must be set")
        .parse::<i64>()
        .expect("MASTER_TG_ID must be i64")
});

async fn cmd_answer(bot: Bot, msg: Message, cmd: Command) -> ResponseResult<()> {
    let chat_request = ChatRequest { bot, msg };
    match cmd {
        Command::Help => HelpCmd::new(&chat_request).respond().await?,
        Command::Username => UserInfo::new(&chat_request).respond().await?,
        Command::Ping(target) => PingCmd::new(&chat_request, &target).respond().await?,
        Command::GuS => UuidCmd::new(&chat_request, None).respond().await?,
        Command::GuN(qty) => UuidCmd::new(&chat_request, Some(qty)).respond().await?,
        Command::Gpt(question) => {
            AskGpt::new(&chat_request, &Some(question))
                .respond()
                .await?
        }
    };
    Ok(())
}

async fn raw_messages(bot: Bot, msg: Message) -> ResponseResult<()> {
    let chat_request = ChatRequest { bot, msg };
    let chat_id = chat_request.msg.chat.id;

    if let Some(text) = chat_request.msg.text() {
        chat_request
            .bot
            .send_message(ChatId(*MASTER_TG_ID), text)
            .await?;
        chat_request
            .bot
            .send_message(chat_id, "Message sent. Thanks, unknown internet dweller.")
            .await?;
    } else if let Some(photos) = chat_request.msg.photo() {
        // Telegram sends several photo sizes; the last is the largest.
        let file_id = &photos.last().expect("photo array is non-empty").file.id;
        chat_request
            .bot
            .send_photo(ChatId(*MASTER_TG_ID), InputFile::file_id(file_id.clone()))
            .await?;
        chat_request
            .bot
            .send_message(chat_id, "Photo sent. Thanks, unknown internet dweller.")
            .await?;
    }

    Ok(())
}

pub async fn init_bot(bot: Bot) {
    // Handling of command messages
    let cmd_branch = Update::filter_message()
        .filter_command::<Command>()
        .endpoint(cmd_answer);

    // Handling of raw messages
    let raw_branch = Update::filter_message().endpoint(raw_messages);

    let handler = dptree::entry().branch(cmd_branch).branch(raw_branch);

    Dispatcher::builder(bot, handler)
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
