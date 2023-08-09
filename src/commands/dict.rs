use teloxide::types::ParseMode::MarkdownV2;
use teloxide::{prelude::*, utils::command::BotCommands};

use crate::commands::uuid::{gun, gus};

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
pub enum Command {
    #[command(description = "display this text.")]
    Help,
    #[command(description = "Show client id.")]
    Username,
    #[command(description = "Generate uuid v4. Max - 9")]
    GuN(u8),
    #[command(description = "Generate single uuid v4.")]
    GuS,
}

async fn answer(bot: Bot, msg: Message, cmd: Command) -> ResponseResult<()> {
    match cmd {
        Command::Help => {
            bot.send_message(msg.chat.id, Command::descriptions().to_string())
                .await?
        }
        Command::Username => {
            bot.send_message(msg.chat.id, format!("Client id: {}.", msg.chat.id))
                .await?
        }
        Command::GuN(qty) => {
            bot.send_message(msg.chat.id, gun(qty))
                .parse_mode(MarkdownV2)
                .await?
        }
        Command::GuS => {
            bot.send_message(msg.chat.id, gus())
                .parse_mode(MarkdownV2)
                .await?
        }
    };

    Ok(())
}

pub async fn init_bot(bot: Bot) {
    Command::repl(bot, answer).await
}
