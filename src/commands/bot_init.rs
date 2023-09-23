use std::env;

use teloxide::{prelude::*, utils::command::BotCommands};

use crate::commands::{help::HelpCmd, ping::PingCmd, user_info::UserInfo, uuid::UuidCmd};

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
}

pub struct ChatRequest {
    pub bot: Bot,
    pub msg: Message,
}

async fn cmd_answer(bot: Bot, msg: Message, cmd: Command) -> ResponseResult<()> {
    let chat_request: ChatRequest = ChatRequest { bot, msg };
    match cmd {
        Command::Help => HelpCmd::new(&chat_request).respond().await?,
        Command::Username => UserInfo::new(&chat_request).respond().await?,
        Command::PING(target) => PingCmd::new(&chat_request, &target).respond().await?,
        Command::GuS => UuidCmd::new(&chat_request, None).respond().await?,
        Command::GuN(qty) => UuidCmd::new(&chat_request, Some(qty)).respond().await?,
    };
    Ok(())
}

async fn raw_messages(bot: Bot, msg: Message) -> ResponseResult<()> {
    let chat_request: ChatRequest = ChatRequest { bot, msg };

    chat_request
        .bot
        .send_message(
            ChatId(
                env::var("MASTER_TG_ID")
                    .expect("To 'MASTER_TG_ID' to be set")
                    .parse::<i64>()
                    .expect("MASTER_TG_ID to be i64"),
            ),
            format!(
                "From internet dweller: {:?}",
                &chat_request.msg.text().unwrap_or("Unsupported media type")
            ),
        )
        .await?;

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
