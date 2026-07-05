//! Dialogue (FSM) flows for key and context management.
//!
//! State is held in teloxide's `InMemStorage` — it does not survive a process
//! restart, which is fine for short "type your passphrase" / "type the context
//! text" exchanges. The encryption key itself, once derived, lives in
//! [`crate::state::AppState`].
//!
//! Wiring: the whole message tree is behind `enter_dialogue`, so every handler
//! (including `State::Start` command dispatch) receives a [`MyDialogue`].

use std::sync::Arc;

use teloxide::dispatching::dialogue::{Dialogue, GetChatId, InMemStorage};
use teloxide::prelude::*;
use teloxide::types::{CallbackQuery, ParseMode};

use crate::commands::bot_init::ChatRequest;
use crate::commands::key_ops::{self, change_key, set_key, KeyError};
use crate::crypto::{decrypt, encrypt};
use crate::menu::{self, MenuAction};
use crate::state::AppState;

pub type MyDialogue = Dialogue<State, InMemStorage<State>>;
pub type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
pub type BoxedErr = Box<dyn std::error::Error + Send + Sync>;

#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Start,
    /// Capturing a passphrase for first-time setup.
    AwaitingPassphraseSet,
    /// /key change, step 1: capture the old passphrase.
    AwaitingOldPassphrase,
    /// /key change, step 2: capture the new passphrase.
    AwaitingNewPassphrase { old: String },
    /// /ctx add | /ctx edit <seq>: capture the chunk text.
    AwaitingContextText { action: CtxAction },
    /// Waiting for a ping target (host/ip/url).
    AwaitingPingTarget,
    /// Waiting for a GPT question.
    AwaitingGptQuestion,
    /// Waiting for a text message to forward to the owner.
    AwaitingOwnerMessage,
    /// Waiting for a photo to forward to the owner.
    AwaitingOwnerPhoto,
}

/// Resolve the owner's telegram id from MASTER_TG_ID (set once per process).
fn master_id() -> i64 {
    static MASTER: std::sync::LazyLock<i64> = std::sync::LazyLock::new(|| {
        std::env::var("MASTER_TG_ID")
            .expect("MASTER_TG_ID must be set")
            .parse::<i64>()
            .expect("MASTER_TG_ID must be i64")
    });
    *MASTER
}

#[derive(Clone, Debug)]
pub enum CtxAction {
    Add,
}

// =========================================================================
// State::Start — command dispatch + raw messages.
// =========================================================================

/// Handles plain (non-command) messages while in `State::Start`. Forwarding to
/// the owner is intentional and goes through the explicit «Написать владельцу»
/// and «Фото владельцу» buttons; here we just nudge the user back to the menu.
pub async fn start_raw(bot: Bot, msg: Message) -> HandlerResult {
    let _ = msg;
    bot.send_message(
        msg.chat.id,
        "Чтобы выбрать действие — откройте меню кнопкой ниже или командой /start.",
    )
    .reply_markup(menu::main_menu())
    .await?;
    Ok(())
}

// =========================================================================
// Key FSM states (entered from the inline menu's key submenu).
// =========================================================================

/// State::AwaitingPassphraseSet
pub async fn receive_passphrase_set(
    bot: Bot,
    dialogue: MyDialogue,
    msg: Message,
    state: Arc<AppState>,
) -> HandlerResult {
    let Some(pass) = msg.text().map(str::to_owned) else {
        bot.send_message(msg.chat.id, "Пожалуйста, отправьте текст.")
            .await?;
        return Ok(());
    };
    let tg_id = msg.chat.id.0;

    let result = tokio::task::spawn_blocking(move || set_key(&state, tg_id, pass.as_bytes()))
        .await
        .map_err(|e| -> BoxedErr { format!("Task failed: {e}").into() })?;

    match result {
        Ok(()) => {
            bot.send_message(
                msg.chat.id,
                "Ключ установлен. Теперь доступны «Контекст» и «Спросить GPT».",
            )
            .reply_markup(menu::main_menu())
            .await?;
            dialogue.exit().await?;
        }
        Err(KeyError::WrongPassphrase) => {
            // Stay in the dialogue so the user can retry. Record exists but the
            // passphrase doesn't match — this is the post-restart restore path.
            bot.send_message(
                msg.chat.id,
                "Неверная passphrase. Запись уже существует — попробуйте ту же passphrase, что использовали ранее.",
            )
            .await?;
            // Do NOT exit: remain in AwaitingPassphraseSet for another attempt.
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("Не удалось установить ключ: {e}"))
                .reply_markup(menu::main_menu())
                .await?;
            dialogue.exit().await?;
        }
    }
    Ok(())
}

/// State::AwaitingOldPassphrase — /key change step 1.
pub async fn receive_old_passphrase(bot: Bot, dialogue: MyDialogue, msg: Message) -> HandlerResult {
    let Some(old) = msg.text().map(str::to_owned) else {
        bot.send_message(msg.chat.id, "Please send text.").await?;
        return Ok(());
    };
    bot.send_message(msg.chat.id, "Send the NEW passphrase.")
        .await?;
    dialogue
        .update(State::AwaitingNewPassphrase { old })
        .await?;
    Ok(())
}

/// State::AwaitingNewPassphrase — /key change step 2.
pub async fn receive_new_passphrase(
    bot: Bot,
    dialogue: MyDialogue,
    msg: Message,
    state: Arc<AppState>,
    old: String,
) -> HandlerResult {
    let Some(new) = msg.text().map(str::to_owned) else {
        bot.send_message(msg.chat.id, "Please send text.").await?;
        return Ok(());
    };
    let tg_id = msg.chat.id.0;

    let result = tokio::task::spawn_blocking(move || {
        change_key(&state, tg_id, old.as_bytes(), new.as_bytes())
    })
    .await
    .map_err(|e| -> BoxedErr { format!("Task failed: {e}").into() })?;

    match result {
        Ok(()) => {
            bot.send_message(
                msg.chat.id,
                "Key changed; all data re-encrypted under the new key.",
            )
            .await?;
        }
        Err(KeyError::WrongPassphrase) => {
            bot.send_message(msg.chat.id, "Wrong old passphrase; key unchanged.")
                .await?;
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("Could not change key: {e}"))
                .await?;
        }
    }
    dialogue.exit().await?;
    Ok(())
}

// =========================================================================
// Context FSM state (entered from the inline menu's context submenu).
// =========================================================================

/// State::AwaitingContextText — add a new chunk (callback «Добавить кусок»).
pub async fn receive_context_text(
    bot: Bot,
    dialogue: MyDialogue,
    msg: Message,
    state: Arc<AppState>,
    action: CtxAction,
) -> HandlerResult {
    let Some(text) = msg.text().map(str::to_owned) else {
        bot.send_message(msg.chat.id, "Please send text.").await?;
        return Ok(());
    };
    let tg_id = msg.chat.id.0;
    let _ = action; // currently always Add; kept for future edit support.

    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let key = state
            .with_key(tg_id, |k| k.clone())
            .ok_or("No key loaded")?;
        let blob = encrypt(&key, text.as_bytes()).map_err(|e| e.to_string())?;
        let seq = state.db.next_seq(tg_id).map_err(|e| e.to_string())?;
        state
            .db
            .insert_chunk(tg_id, seq, &blob)
            .map_err(|e| e.to_string())?;
        Ok(format!("Context chunk {seq} added."))
    })
    .await
    .map_err(boxed_join_err)?;

    match result {
        Ok(text) => {
            bot.send_message(msg.chat.id, text)
                .reply_markup(menu::main_menu())
                .await?;
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("Error: {e}"))
                .reply_markup(menu::main_menu())
                .await?;
        }
    }
    dialogue.exit().await?;
    Ok(())
}

// =========================================================================
// State::AwaitingPingTarget / State::AwaitingGptQuestion
// =========================================================================

/// State::AwaitingPingTarget — user typed a host after pressing «Ping».
pub async fn receive_ping_target(
    bot: Bot,
    dialogue: MyDialogue,
    msg: Message,
    state: Arc<AppState>,
) -> HandlerResult {
    let Some(target) = msg.text().map(str::to_owned) else {
        bot.send_message(msg.chat.id, "Please send text.").await?;
        return Ok(());
    };
    let chat_request = ChatRequest {
        bot: bot.clone(),
        msg: msg.clone(),
        state,
    };
    crate::commands::ping::PingCmd::new(&chat_request, &target)
        .respond()
        .await?;
    // Re-open the main menu for the next action.
    bot.send_message(msg.chat.id, "Что дальше?")
        .reply_markup(menu::main_menu())
        .await?;
    let _ = dialogue; // exit not strictly needed; state already Start.
    Ok(())
}

/// State::AwaitingGptQuestion — user typed a question after pressing «GPT».
pub async fn receive_gpt_question(
    bot: Bot,
    dialogue: MyDialogue,
    msg: Message,
    state: Arc<AppState>,
) -> HandlerResult {
    let Some(question) = msg.text().map(str::to_owned) else {
        bot.send_message(msg.chat.id, "Please send text.").await?;
        return Ok(());
    };
    let chat_request = ChatRequest {
        bot: bot.clone(),
        msg: msg.clone(),
        state,
    };
    crate::commands::chat_gpt::AskGpt::new(&chat_request, &Some(question))
        .respond()
        .await?;
    bot.send_message(msg.chat.id, "Что дальше?")
        .reply_markup(menu::main_menu())
        .await?;
    let _ = dialogue;
    Ok(())
}

/// State::AwaitingOwnerMessage — forward the typed text to the owner.
pub async fn receive_owner_message(bot: Bot, dialogue: MyDialogue, msg: Message) -> HandlerResult {
    let Some(text) = msg.text().map(str::to_owned) else {
        bot.send_message(msg.chat.id, "Пожалуйста, отправьте текст.")
            .await?;
        return Ok(());
    };
    // Forward the text to the owner (MASTER_TG_ID) and acknowledge to the user.
    bot.send_message(ChatId(master_id()), format!("✉️ {text}"))
        .await?;
    bot.send_message(
        msg.chat.id,
        "Message sent to the owner. Thanks unknown internet dweller.",
    )
    .reply_markup(menu::main_menu())
    .await?;
    dialogue.exit().await?;
    Ok(())
}

/// State::AwaitingOwnerPhoto — forward the photo to the owner.
pub async fn receive_owner_photo(bot: Bot, dialogue: MyDialogue, msg: Message) -> HandlerResult {
    let Some(photos) = msg.photo().map(|p| p.to_vec()) else {
        bot.send_message(msg.chat.id, "Пожалуйста, отправьте фото.")
            .await?;
        return Ok(());
    };
    // Telegram sends several photo sizes; the last is the largest.
    let file_id = &photos.last().expect("photo array is non-empty").file.id;
    bot.send_photo(
        ChatId(master_id()),
        teloxide::types::InputFile::file_id(file_id.clone()),
    )
    .await?;
    bot.send_message(msg.chat.id, "Фото отправлено в астрал.")
        .reply_markup(menu::main_menu())
        .await?;
    dialogue.exit().await?;
    Ok(())
}

// =========================================================================
// Callback router — handles inline-button presses.
// =========================================================================

/// The single entry point for all inline-button presses. Parses `callback_data`
/// and either performs the action immediately or transitions into an FSM state
/// that waits for text input.
pub async fn callback_handler(
    bot: Bot,
    dialogue: MyDialogue,
    q: CallbackQuery,
    state: Arc<AppState>,
) -> HandlerResult {
    // Always acknowledge so the button stops spinning.
    let query_id = q.id.clone();
    let data = q.data.clone();
    let chat_id = q.chat_id();
    let from_id = q.from.id.0;

    // For messages with an attached keyboard we can edit them in place; for
    // inline-message-id callbacks we cannot. We try regular_message first.
    let regular = q.regular_message().cloned();

    // Acknowledge regardless of what we do next.
    if let Err(e) = bot.answer_callback_query(query_id).await {
        log::warn!("answer_callback_query failed: {e}");
    }

    let Some(chat_id) = chat_id else {
        return Ok(());
    };
    let Some(data) = data else {
        return Ok(());
    };
    let Some(action) = MenuAction::parse(&data) else {
        bot.send_message(chat_id, format!("Unknown action: {data}"))
            .await?;
        return Ok(());
    };

    let tg_id = chat_id.0;

    match action {
        MenuAction::Main => {
            show_menu(&bot, chat_id, regular, "Главное меню", menu::main_menu()).await?
        }
        MenuAction::Help => {
            // Edit-in-place the message text with help, keep a back button.
            let text = crate::commands::help::HELP_MSG;
            edit_or_send(&bot, chat_id, regular, text, menu::back_to_main()).await?;
        }
        MenuAction::Profile => {
            let text = format!(
                "UserId: {}\nUserName: {}",
                from_id,
                q.from.username.as_deref().unwrap_or("n/a"),
            );
            edit_or_send(&bot, chat_id, regular, &text, menu::back_to_main()).await?;
        }

        // ----- key -----
        MenuAction::Key => {
            let has = state.has_key(tg_id);
            let status = if has {
                "Ключ загружен."
            } else {
                "Ключ не задан. Либо произошла перезагрузка и ключ был стерт из памяти приложения, но его хэш сохранился в базе. Введите тот же ключ или смените его на новый."
            };
            edit_or_send(
                &bot,
                chat_id,
                regular,
                &format!("🔑 Ключ шифрования\n{status}"),
                menu::key_menu(),
            )
            .await?;
        }
        MenuAction::KeySet => {
            if state.has_key(tg_id) {
                bot.send_message(chat_id, "Ключ уже загружен. Используйте «Сменить ключ».")
                    .await?;
            } else {
                bot.send_message(
                    chat_id,
                    "Отправьте passphrase. Она не сохраняется — только её Argon2id-верификатор. НЕ ИСПОЛЬЗУЙТЕ СВОЙ НАСТОЯЩИЙ ПАРОЛЬ! ЛУЧШЕ ВЫДУМАТЬ СЛУЧАЙНУЮ ФРАЗУ, КОТОРУЮ ВЫ НЕ ЗАБУДЕТЕ.",
                )
                .await?;
                dialogue.update(State::AwaitingPassphraseSet).await?;
            }
        }
        MenuAction::KeyChange => {
            if state.has_key(tg_id) {
                bot.send_message(chat_id, "Отправьте ТЕКУЩУЮ passphrase.")
                    .await?;
                dialogue.update(State::AwaitingOldPassphrase).await?;
            } else {
                bot.send_message(chat_id, "Ключ не загружен. Сначала «Установить ключ». Если Вы уже загружали ключ и видите это сообщение, значит произошла перезагрузка и ключ был стерт из памяти приложения, но его хэш сохранился в базе. Введите тот же ключ или смените его на новый.")
                    .await?;
            }
        }
        MenuAction::KeyClear => {
            // Full wipe: key from memory + user record + all chunks/messages
            // (ON DELETE CASCADE). This is irreversible — needed so /key set
            // can start fresh after a restart where the in-memory key is gone.
            let removed = match tokio::task::spawn_blocking(move || {
                key_ops::delete_user(&state, tg_id)
            })
            .await
            {
                Ok(Ok(true)) => true,
                Ok(Ok(false)) => false,
                Ok(Err(e)) => {
                    bot.send_message(chat_id, format!("Ошибка очистки: {e}"))
                        .reply_markup(menu::main_menu())
                        .await?;
                    return Ok(());
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("Task failed: {e}"))
                        .reply_markup(menu::main_menu())
                        .await?;
                    return Ok(());
                }
            };
            let text = if removed {
                "Ключ и все зашифрованные данные (контекст, история) полностью удалены. Теперь «Установить ключ» создаст всё с чистого листа."
            } else {
                "Нечего удалять — ключ не был задан."
            };
            bot.send_message(chat_id, text)
                .reply_markup(menu::main_menu())
                .await?;
        }

        // ----- context -----
        MenuAction::Ctx => {
            if !state.has_key(tg_id) {
                bot.send_message(chat_id, "Сначала задайте ключ.")
                    .reply_markup(menu::key_menu())
                    .await?;
                return Ok(());
            }
            edit_or_send(
                &bot,
                chat_id,
                regular,
                "📦 Контекст для GPT",
                menu::ctx_menu(),
            )
            .await?;
        }
        MenuAction::CtxAdd => {
            bot.send_message(chat_id, "Отправьте текст чанка контекста.")
                .await?;
            dialogue
                .update(State::AwaitingContextText {
                    action: CtxAction::Add,
                })
                .await?;
        }
        MenuAction::CtxList => {
            let seqs = spawn_db(state.clone(), move |app| {
                app.db.chunk_seqs(tg_id).map_err(|e| e.to_string())
            })
            .await?;
            if seqs.is_empty() {
                bot.send_message(chat_id, "Чанков контекста пока нет.")
                    .reply_markup(menu::ctx_menu())
                    .await?;
            } else {
                edit_or_send(
                    &bot,
                    chat_id,
                    regular,
                    "Выберите чанк для просмотра:",
                    menu::chunks_list_menu(&seqs),
                )
                .await?;
            }
        }
        MenuAction::CtxShow(seq) => {
            // Decrypt and show the chunk; offer delete / back-to-list.
            let text = spawn_db(
                state.clone(),
                move |app| -> Result<Option<String>, String> {
                    let chunks = app.db.chunks(tg_id).map_err(|e| e.to_string())?;
                    Ok(app
                        .with_key(tg_id, |key| {
                            chunks
                                .iter()
                                .find(|c| c.seq == seq)
                                .and_then(|c| String::from_utf8(decrypt(key, &c.blob).ok()?).ok())
                        })
                        .flatten())
                },
            )
            .await?;
            match text {
                Some(body) => {
                    bot.send_message(chat_id, format!("Чанк #{seq}:\n\n{body}"))
                        .reply_markup(menu::chunk_viewer_menu(seq))
                        .await?;
                }
                None => {
                    bot.send_message(chat_id, format!("Чанк {seq} не найден."))
                        .reply_markup(menu::ctx_menu())
                        .await?;
                }
            }
        }
        MenuAction::CtxDel(seq) => {
            let removed = spawn_db(state.clone(), move |app| {
                app.db.delete_chunk(tg_id, seq).map_err(|e| e.to_string())
            })
            .await?;
            let text = if removed {
                format!("Чанк {seq} удалён.")
            } else {
                format!("Чанк {seq} не найден.")
            };
            // After deletion, refresh the list (or show the empty notice).
            let seqs = spawn_db(state.clone(), move |app| {
                app.db.chunk_seqs(tg_id).map_err(|e| e.to_string())
            })
            .await?;
            if seqs.is_empty() {
                bot.send_message(chat_id, format!("{text} Это была последняя часть."))
                    .reply_markup(menu::ctx_menu())
                    .await?;
            } else {
                bot.send_message(chat_id, format!("{text} Остальные части промта:"))
                    .reply_markup(menu::chunks_list_menu(&seqs))
                    .await?;
            }
        }
        MenuAction::CtxClear => {
            // Delete all chunks for this user.
            let removed = spawn_db(state.clone(), move |app| -> Result<usize, String> {
                let seqs = app.db.chunk_seqs(tg_id).map_err(|e| e.to_string())?;
                let mut n = 0;
                for s in seqs {
                    if app.db.delete_chunk(tg_id, s).map_err(|e| e.to_string())? {
                        n += 1;
                    }
                }
                Ok(n)
            })
            .await?;
            bot.send_message(chat_id, format!("Удалено чанков: {removed}."))
                .reply_markup(menu::ctx_menu())
                .await?;
        }

        // ----- gpt -----
        MenuAction::Gpt => {
            if !state.has_key(tg_id) {
                bot.send_message(
                    chat_id,
                    "Сначала задайте ключ — GPT хранит историю в зашифрованном виде.",
                )
                .reply_markup(menu::key_menu())
                .await?;
                return Ok(());
            }
            bot.send_message(chat_id, "Отправьте вопрос для GPT.")
                .await?;
            dialogue.update(State::AwaitingGptQuestion).await?;
        }

        // ----- ping -----
        MenuAction::Ping => {
            bot.send_message(
                chat_id,
                "Отправьте цель для ping (8.8.8.8 | https://google.com | google.com).",
            )
            .await?;
            dialogue.update(State::AwaitingPingTarget).await?;
        }

        // ----- uuid -----
        MenuAction::Uuid => {
            edit_or_send(
                &bot,
                chat_id,
                regular,
                "Сколько UUID сгенерировать?",
                menu::uuid_menu(),
            )
            .await?;
        }
        MenuAction::UuidN(n) => {
            let text = crate::commands::uuid::render(n);
            bot.send_message(chat_id, text)
                .parse_mode(ParseMode::MarkdownV2)
                .reply_markup(menu::main_menu())
                .await?;
        }

        // ----- owner contact -----
        MenuAction::Msg => {
            bot.send_message(chat_id, "Отправьте текст сообщения в ноосферу")
                .await?;
            dialogue.update(State::AwaitingOwnerMessage).await?;
        }
        MenuAction::Photo => {
            bot.send_message(chat_id, "Отправьте фото в ноосферу")
                .await?;
            dialogue.update(State::AwaitingOwnerPhoto).await?;
        }
    }

    Ok(())
}

/// Edit the message the button was attached to, or send a fresh one if it's
/// not available (e.g. inline mode).
async fn edit_or_send(
    bot: &Bot,
    chat_id: ChatId,
    regular: Option<Message>,
    text: &str,
    keyboard: teloxide::types::InlineKeyboardMarkup,
) -> HandlerResult {
    if let Some(msg) = regular {
        bot.edit_message_text(chat_id, msg.id, text)
            .reply_markup(keyboard)
            .await?;
    } else {
        bot.send_message(chat_id, text)
            .reply_markup(keyboard)
            .await?;
    }
    Ok(())
}

/// Show a menu: prefer editing the attached message, otherwise send a new one.
async fn show_menu(
    bot: &Bot,
    chat_id: ChatId,
    regular: Option<Message>,
    text: &str,
    keyboard: teloxide::types::InlineKeyboardMarkup,
) -> HandlerResult {
    edit_or_send(bot, chat_id, regular, text, keyboard).await
}

// =========================================================================
// Helpers
// =========================================================================

/// Run a sync closure on the blocking pool with access to the whole
/// `AppState`. Flattens both the JoinError and the closure's own error into a
/// single boxed error, so callers need one `?`.
async fn spawn_db<T, F>(state: Arc<AppState>, f: F) -> Result<T, BoxedErr>
where
    T: Send + 'static,
    F: FnOnce(&AppState) -> Result<T, String> + Send + 'static,
{
    match tokio::task::spawn_blocking(move || f(&state)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(s)) => Err(boxed_str_err(s)),
        Err(join_err) => Err(boxed_join_err(join_err)),
    }
}

// The `state` is already Arc<AppState>.

fn boxed_join_err<E: std::fmt::Display + Send + Sync + 'static>(e: E) -> BoxedErr {
    format!("Background task failed: {e}").into()
}

fn boxed_str_err(e: String) -> BoxedErr {
    e.into()
}
