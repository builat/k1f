# k1f-bot Is telegram bot with swiss knife of utils (should become in future)

## k1f - stands for k33p 1nt3rn3t fr33 (keep internet free).

Also it's a project to improve my personal rust skills.


## Start the bot.
0. Clone the repo
1. Create token in tg bot: https://t.me/BotFather 
2. ```bash
    $ export TELOXIDE_TOKEN=*YOUR-TOKEN*
    $ cargo run
   ```
3. For "production"  
   ```bash
   $ export TELOXIDE_TOKEN=*YOUR-TOKEN*
   $ cargo build --release
   $ ./target/release/k1f-bot
   ```
4. In case you want to keep it running
   ```bash
   $ export TELOXIDE_TOKEN=*YOUR-TOKEN*
   $ cargo build --release
   $ ./target/release/k1f-bot&
   $ ps -aux | grep k1f
   $ disown *kif-pid*
   ```
