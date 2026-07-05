//! Help text shown by the «❓ Справка» button.

pub const HELP_MSG: &str = "\
🔑 Ключ — управление ключом шифрования (set / change / clear).
📦 Контекст — куски контекста для GPT (add / list / clear).
💬 Спросить GPT — вопрос с использованием контекста и истории (история шифруется).
🌐 Ping — проверить хост/IP/URL.
🔮 UUID — сгенерировать uuid v4.
👤 Профиль — ваш id.
 Сбросить историю GPT можно командой /reset.

Данные шифруются per-user ключом (Argon2id из вашей passphrase).
";
