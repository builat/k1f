use std::env;

use teloxide::{
    dispatching::dialogue::GetChatId, prelude::*, types::InputFile, utils::command::BotCommands,
};

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
    PING(String),
    GPT(String),
}

pub struct ChatRequest {
    pub bot: Bot,
    pub msg: Message,
}
use lazy_static::lazy_static;

lazy_static! {
    static ref MASTER_TG_ID: i64 = env::var("MASTER_TG_ID".to_string())
        .expect("To 'MASTER_TG_ID' to be set")
        .parse::<i64>()
        .expect("MASTER_TG_ID to be i64");
    static ref FROM_PLACEHOLDER: String = "From internet dweller:".to_string();
}

async fn cmd_answer(bot: Bot, msg: Message, cmd: Command) -> ResponseResult<()> {
    let chat_request: ChatRequest = ChatRequest { bot, msg };
    match cmd {
        Command::Help => HelpCmd::new(&chat_request).respond().await?,
        Command::Username => UserInfo::new(&chat_request).respond().await?,
        Command::PING(target) => PingCmd::new(&chat_request, &target).respond().await?,
        Command::GuS => UuidCmd::new(&chat_request, None).respond().await?,
        Command::GuN(qty) => UuidCmd::new(&chat_request, Some(qty)).respond().await?,
        Command::GPT(question) => {
            AskGpt::new(&chat_request, &Some(question))
                .respond()
                .await?
        }
    };
    Ok(())
}

async fn raw_messages(bot: Bot, msg: Message) -> ResponseResult<()> {
    let chat_request: ChatRequest = ChatRequest { bot, msg };

    match &chat_request {
        txt_msg if txt_msg.msg.text().is_some() => {
            let _ = &chat_request
                .bot
                .send_message(
                    ChatId(*MASTER_TG_ID),
                    format!(
                        "{:?}",
                        &chat_request.msg.text().unwrap_or("Unsupported media type")
                    ),
                )
                .await?;

            let _ = &chat_request
                .bot
                .send_message(
                    ChatId::from(chat_request.msg.chat_id().unwrap()),
                    "Message sent. Thanks, unknown internet dweller.",
                )
                .await?;
        }

        img_msg if img_msg.msg.photo().is_some() => {
            let largest_photo = img_msg.msg.photo().unwrap();
            let file_id = &largest_photo.last().unwrap().file.id;
            let photo = InputFile::file_id(file_id.clone());

            let _ = &chat_request
                .bot
                .send_photo(ChatId(*MASTER_TG_ID), photo)
                .await?;

            let _ = &chat_request
                .bot
                .send_message(
                    chat_request.msg.chat_id().unwrap(),
                    "Photo sent. Thanks, unknown internet dweller.",
                )
                .await?;
        }
        _ => {}
    };

    Ok(())
}

pub async fn init_bot(bot: Bot) {
    // Handling of command messages
    let cmd_brach = Update::filter_message()
        .filter_command::<Command>()
        .endpoint(cmd_answer);

    // Handling of raw messages
    let raw_branch = Update::filter_message().endpoint(raw_messages);

    let handler = dptree::entry().branch(cmd_brach).branch(raw_branch);

    Dispatcher::builder(bot, handler)
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
