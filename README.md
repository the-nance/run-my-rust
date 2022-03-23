# *Run My Rust*

A bot for running all Rust command blocks (\`\`\`rs or `\`\`\rust) in certain
channels. This bot was made for my new Rust related server, [the Late Night Rusting server](https://discord.com/invite/gqCfUZE7tY)
*note: use `cargo run --target x86_64-unknown-linux-gnu` to start*

### Config

```toml
# config.toml

token = "your token here"
# List of channels where the bot will run rust from
channels = []
# List of roles that are banned from interacting with the bot
banned_roles = []
```