//! Inline-keyboard menus and `callback_data` routing.
//!
//! Telegram attaches a `callback_data` string to each inline button press; we
//! encode the intended action there and parse it in the callback handler. The
//! format is deliberately short (Telegram limits `callback_data` to 64 bytes):
//!
//! | callback_data   | meaning                                  |
//! |-----------------|------------------------------------------|
//! | `main`          | open the main menu                       |
//! | `key`           | key submenu                              |
//! | `key:set`       | start /key set (prompts for passphrase)  |
//! | `key:change`    | start /key change                        |
//! | `key:clear`     | clear key from memory                    |
//! | `ctx`           | context submenu                          |
//! | `ctx:add`       | prompt for a new chunk                   |
//! | `ctx:list`      | list chunk numbers as clickable buttons  |
//! | `ctx:clr`       | delete all chunks                        |
//! | `ctx:show:<n>`  | show chunk number n                      |
//! | `ctx:del:<n>`   | delete chunk number n                    |
//! | `gpt`           | prompt for a GPT question                |
//! | `ping`          | prompt for a ping target                 |
//! | `msg`           | prompt for a message to the owner        |
//! | `photo`         | prompt for a photo to the owner          |
//! | `uuid`          | uuid submenu                             |
//! | `uuid:<n>`      | generate <n> uuids                       |
//! | `profile`       | show user profile                        |
//! | `help`          | show help text                           |

use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

/// All menu actions, parsed from a `callback_data` string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Main,
    Key,
    KeySet,
    KeyChange,
    KeyClear,
    Ctx,
    CtxAdd,
    CtxList,
    CtxClear,
    /// Show the decrypted text of chunk number n.
    CtxShow(i64),
    /// Delete chunk number n.
    CtxDel(i64),
    Gpt,
    Ping,
    /// Send a text message to the bot owner.
    Msg,
    /// Send a photo to the bot owner.
    Photo,
    Uuid,
    UuidN(u8),
    Profile,
    Help,
}

impl MenuAction {
    /// Parse a `callback_data` payload. Returns `None` on unknown data.
    pub fn parse(data: &str) -> Option<Self> {
        Some(match data {
            "main" => Self::Main,
            "key" => Self::Key,
            "key:set" => Self::KeySet,
            "key:change" => Self::KeyChange,
            "key:clear" => Self::KeyClear,
            "ctx" => Self::Ctx,
            "ctx:add" => Self::CtxAdd,
            "ctx:list" => Self::CtxList,
            "ctx:clr" => Self::CtxClear,
            "gpt" => Self::Gpt,
            "ping" => Self::Ping,
            "msg" => Self::Msg,
            "photo" => Self::Photo,
            "uuid" => Self::Uuid,
            "profile" => Self::Profile,
            "help" => Self::Help,
            other if other.starts_with("uuid:") => {
                let n = other["uuid:".len()..].parse::<u8>().ok()?;
                Self::UuidN(n)
            }
            other if other.starts_with("ctx:show:") => {
                let n = other["ctx:show:".len()..].parse::<i64>().ok()?;
                Self::CtxShow(n)
            }
            other if other.starts_with("ctx:del:") => {
                let n = other["ctx:del:".len()..].parse::<i64>().ok()?;
                Self::CtxDel(n)
            }
            _ => return None,
        })
    }

    /// Serialize back to a `callback_data` string.
    pub fn to_data(&self) -> String {
        match self {
            Self::Main => "main".to_string(),
            Self::Key => "key".to_string(),
            Self::KeySet => "key:set".to_string(),
            Self::KeyChange => "key:change".to_string(),
            Self::KeyClear => "key:clear".to_string(),
            Self::Ctx => "ctx".to_string(),
            Self::CtxAdd => "ctx:add".to_string(),
            Self::CtxList => "ctx:list".to_string(),
            Self::CtxClear => "ctx:clr".to_string(),
            Self::CtxShow(n) => format!("ctx:show:{n}"),
            Self::CtxDel(n) => format!("ctx:del:{n}"),
            Self::Gpt => "gpt".to_string(),
            Self::Ping => "ping".to_string(),
            Self::Msg => "msg".to_string(),
            Self::Photo => "photo".to_string(),
            Self::Uuid => "uuid".to_string(),
            Self::UuidN(n) => format!("uuid:{n}"),
            Self::Profile => "profile".to_string(),
            Self::Help => "help".to_string(),
        }
    }
}

fn btn(label: &str, action: MenuAction) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(label.to_string(), action.to_data())
}

/// The top-level menu shown on `/start`.
pub fn main_menu() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            btn("🔑 Ключ шифрования", MenuAction::Key),
            btn("📦 Контекст", MenuAction::Ctx),
        ],
        vec![btn("💬 Спросить GPT", MenuAction::Gpt)],
        vec![
            btn("🌐 Ping", MenuAction::Ping),
            btn("🔮 UUID", MenuAction::Uuid),
        ],
        vec![
            btn("✉️ Написать владельцу", MenuAction::Msg),
            btn("📷 Фото владельцу", MenuAction::Photo),
        ],
        vec![
            btn("👤 Профиль", MenuAction::Profile),
            btn("❓ Справка", MenuAction::Help),
        ],
    ])
}

pub fn key_menu() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![btn("Установить ключ", MenuAction::KeySet)],
        vec![btn("Сменить ключ", MenuAction::KeyChange)],
        vec![btn("🗑 Удалить ключ и данные", MenuAction::KeyClear)],
        vec![btn("← Назад", MenuAction::Main)],
    ])
}

pub fn ctx_menu() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![btn("Добавить кусок", MenuAction::CtxAdd)],
        vec![btn("Просмотр кусков", MenuAction::CtxList)],
        vec![btn("Удалить все", MenuAction::CtxClear)],
        vec![btn("← Назад", MenuAction::Main)],
    ])
}

/// Keyboard listing existing chunk numbers as buttons, each opening a viewer
/// for that chunk. Built dynamically from the user's `chunk_seqs`.
pub fn chunks_list_menu(seqs: &[i64]) -> InlineKeyboardMarkup {
    let rows: Vec<Vec<InlineKeyboardButton>> = seqs
        .chunks(4)
        .map(|row| {
            row.iter()
                .map(|&n| btn(&format!("#{n}"), MenuAction::CtxShow(n)))
                .collect()
        })
        .collect();
    let mut keyboard = rows;
    keyboard.push(vec![btn("← К контексту", MenuAction::Ctx)]);
    InlineKeyboardMarkup::new(keyboard)
}

/// Keyboard for a single chunk viewer: delete this chunk or go back to the list.
pub fn chunk_viewer_menu(seq: i64) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![btn("🗑 Удалить этот кусок", MenuAction::CtxDel(seq))],
        vec![btn("← К списку", MenuAction::CtxList)],
    ])
}

pub fn uuid_menu() -> InlineKeyboardMarkup {
    let row: Vec<InlineKeyboardButton> = [1u8, 3, 5, 10]
        .into_iter()
        .map(|n| btn(&n.to_string(), MenuAction::UuidN(n)))
        .collect();
    InlineKeyboardMarkup::new(vec![row, vec![btn("← Назад", MenuAction::Main)]])
}

/// A single "back to main" keyboard, for messages that belong to a submenu
/// but should return the user to the top.
pub fn back_to_main() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![btn("← В главное меню", MenuAction::Main)]])
}

#[cfg(test)]
mod tests {
    use super::MenuAction;

    #[test]
    fn round_trip_all_variants() {
        let actions = [
            MenuAction::Main,
            MenuAction::Key,
            MenuAction::KeySet,
            MenuAction::KeyChange,
            MenuAction::KeyClear,
            MenuAction::Ctx,
            MenuAction::CtxAdd,
            MenuAction::CtxList,
            MenuAction::CtxClear,
            MenuAction::CtxShow(1),
            MenuAction::CtxShow(42),
            MenuAction::CtxDel(7),
            MenuAction::Gpt,
            MenuAction::Ping,
            MenuAction::Msg,
            MenuAction::Photo,
            MenuAction::Uuid,
            MenuAction::UuidN(5),
            MenuAction::UuidN(10),
            MenuAction::Profile,
            MenuAction::Help,
        ];
        for a in actions {
            let data = a.to_data();
            assert!(data.len() <= 64, "callback_data too long: {data}");
            assert_eq!(
                MenuAction::parse(&data),
                Some(a.clone()),
                "round-trip {a:?}"
            );
        }
    }

    #[test]
    fn unknown_data_is_none() {
        assert_eq!(MenuAction::parse("nonsense"), None);
        assert_eq!(MenuAction::parse("uuid:abc"), None);
        assert_eq!(MenuAction::parse("ctx:show:abc"), None);
        assert_eq!(MenuAction::parse(""), None);
    }

    #[test]
    fn uuid_n_keeps_count() {
        assert_eq!(MenuAction::parse("uuid:7"), Some(MenuAction::UuidN(7)));
        assert_eq!(MenuAction::UuidN(7).to_data(), "uuid:7");
    }

    #[test]
    fn ctx_show_and_del_keep_seq() {
        assert_eq!(
            MenuAction::parse("ctx:show:3"),
            Some(MenuAction::CtxShow(3))
        );
        assert_eq!(MenuAction::CtxShow(3).to_data(), "ctx:show:3");
        assert_eq!(MenuAction::parse("ctx:del:9"), Some(MenuAction::CtxDel(9)));
        assert_eq!(MenuAction::CtxDel(9).to_data(), "ctx:del:9");
    }
}
