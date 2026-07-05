use std::sync::Arc;

use teloxide::prelude::*;

mod commands;
mod crypto;
mod db;
mod menu;
mod state;

#[tokio::main]
async fn main() {
    // Make sure env tweaks are in place before any crate reads them.
    std::env::set_var("RUST_BACKTRACE", "1");
    pretty_env_logger::init();

    log::info!("Starting command bot...");

    // Open (or create) the SQLite database. The path is configurable so the
    // service can point at a persistent volume; default is local k1f.sqlite.
    let db_path = std::env::var("K1F_DB_PATH").unwrap_or_else(|_| "k1f.sqlite".to_string());
    let db = match db::DbHandle::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            log::error!("Failed to open database at {db_path}: {e}");
            std::process::exit(1);
        }
    };
    log::info!("Database ready at {db_path}");

    let app_state = Arc::new(state::AppState::new(db));

    let bot = Bot::from_env();
    commands::bot_init::init_bot(bot, app_state).await;
}
