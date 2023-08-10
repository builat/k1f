use teloxide::prelude::*;

mod commands;

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    std::env::set_var("RUST_BACKTRACE", "1");
    log::info!("Starting command bot...");
    let bot = Bot::from_env();
    commands::dict::init_bot(bot).await;
}
