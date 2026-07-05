use std::sync::Arc;

use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::{prelude::*, utils::command::BotCommands};

use crate::commands::dialogue::{self, HandlerResult, State};
use crate::menu;
use crate::state::AppState;

/// The only slash command left: `/start` opens the main menu. `/reset` clears
/// GPT history without digging through menus.
#[derive(BotCommands, Clone, Debug)]
#[command(rename_rule = "lowercase", description = "")]
pub enum Command {
    /// открыть главное меню
    #[command(description = "open the main menu")]
    Start,
    /// очистить историю диалога с GPT
    #[command(description = "clear the GPT dialogue history")]
    Reset,
}

pub struct ChatRequest {
    pub bot: Bot,
    pub msg: Message,
    pub state: Arc<AppState>,
}

/// `/start` and `/reset` in `State::Start`.
async fn start_command(
    bot: Bot,
    msg: Message,
    state: Arc<AppState>,
    cmd: Command,
) -> HandlerResult {
    match cmd {
        Command::Start => {
            bot.send_message(msg.chat.id, "Привет! Выбери действие:")
                .reply_markup(menu::main_menu())
                .await?;
        }
        Command::Reset => {
            let tg_id = msg.chat.id.0;
            let result = tokio::task::spawn_blocking(move || state.db.clear_messages(tg_id)).await;
            let text = match result {
                Ok(Ok(())) => "История диалога очищена.".to_string(),
                Ok(Err(e)) => format!("Ошибка: {e}"),
                Err(e) => format!("Task failed: {e}"),
            };
            bot.send_message(msg.chat.id, text)
                .reply_markup(menu::main_menu())
                .await?;
        }
    }
    Ok(())
}

pub async fn init_bot(bot: Bot, state: Arc<AppState>) {
    let storage = InMemStorage::<State>::new();

    // ---- message tree (text replies + /start + /reset) ----
    let msg_start = dptree::case![State::Start]
        .branch(
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(start_command),
        )
        .branch(Update::filter_message().endpoint(dialogue::start_raw));

    let messages = Update::filter_message()
        .enter_dialogue::<Message, InMemStorage<State>, State>()
        .branch(msg_start)
        .branch(
            dptree::case![State::AwaitingPassphraseSet].endpoint(dialogue::receive_passphrase_set),
        )
        .branch(
            dptree::case![State::AwaitingOldPassphrase].endpoint(dialogue::receive_old_passphrase),
        )
        .branch(
            dptree::case![State::AwaitingNewPassphrase { old }]
                .endpoint(dialogue::receive_new_passphrase),
        )
        .branch(
            dptree::case![State::AwaitingContextText { action }]
                .endpoint(dialogue::receive_context_text),
        )
        .branch(dptree::case![State::AwaitingPingTarget].endpoint(dialogue::receive_ping_target))
        .branch(dptree::case![State::AwaitingGptQuestion].endpoint(dialogue::receive_gpt_question))
        .branch(
            dptree::case![State::AwaitingOwnerMessage].endpoint(dialogue::receive_owner_message),
        )
        .branch(dptree::case![State::AwaitingOwnerPhoto].endpoint(dialogue::receive_owner_photo));

    // ---- callback-query tree (inline-button presses) ----
    let callbacks = Update::filter_callback_query()
        .enter_dialogue::<CallbackQuery, InMemStorage<State>, State>()
        .endpoint(dialogue::callback_handler);

    let handler = dptree::entry().branch(messages).branch(callbacks);

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![storage, state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}
