use teloxide::prelude::*;

mod commands;

#[tokio::main]
async fn main() {
    // Make sure env tweaks are in place before any crate reads them.
    std::env::set_var("RUST_BACKTRACE", "1");
    pretty_env_logger::init();

    log::info!("Starting command bot...");
    let bot = Bot::from_env();
    commands::bot_init::init_bot(bot).await;
}
