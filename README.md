# brig-discord

Discord gateway for [Brig](https://github.com/jmspring/brig) — bridges Discord
bot messages to Brig's unix domain socket.

This is a small, standalone binary. No async runtime, no Discord bot framework.
Just synchronous websocket (tungstenite) and HTTP (ureq) calls.

## Prerequisites

- Brig running in daemon mode (`brig -d`)
- A Discord bot token from the [Discord Developer Portal](https://discord.com/developers/applications)

### Creating a Discord Bot

1. Go to https://discord.com/developers/applications
2. Click "New Application", give it a name
3. Go to "Bot" in the sidebar
4. Click "Reset Token" to generate a bot token — save this
5. Under "Privileged Gateway Intents", enable:
   - **Message Content Intent** (required to read message text)
6. Go to "OAuth2" → "URL Generator"
7. Select scopes: `bot`
8. Select permissions: `Send Messages`, `Read Message History`
9. Copy the generated URL and open it to invite the bot to your server

## Build

```sh
cargo build --release
```

## Install as Brig Skill

```sh
# Add the skill to brig
brig skill add ./

# Set the Discord token
brig secret set discord-gateway.discord_token
# (paste your bot token when prompted)

# Enable the persistent skill
brig skill enable discord-gateway

# Start the service
service brig_discord start

# Enable at boot
sysrc brig_discord_enable=YES
```

## Manual Run

```sh
export BRIG_DISCORD_TOKEN="your-bot-token-here"
export BRIG_SOCKET="/var/brig/sock/brig.sock"  # optional, this is the default
./target/release/brig-discord
```

## How It Works

1. Connects to Brig's unix socket, sends hello, receives welcome
2. Fetches Discord gateway URL from `/api/v10/gateway`
3. Connects to Discord's websocket gateway
4. Sends Identify with bot token and required intents
5. Listens for MESSAGE_CREATE events
6. For each non-bot message:
   - Formats session key as `discord-{guild_id}-{channel_id}-{user_id}`
   - Sends task to Brig socket
   - Reads status updates until final response
   - POSTs response back to Discord channel
7. Handles heartbeats to keep the websocket alive
8. Reconnects automatically on connection loss

## Session Keys

Each conversation is identified by a session key:

```
discord-{guild_id}-{channel_id}-{user_id}
```

For DMs, `guild_id` is `dm`. This means:
- Different users in the same channel have separate sessions
- The same user in different channels has separate sessions
- Brig's memory system tracks context per session

## Dependencies

- `ureq` — synchronous HTTP client (Discord REST API)
- `tungstenite` — synchronous websocket client (Discord Gateway)
- `serde`, `serde_json` — JSON serialization

Total: 4 crates. No async. No framework.

## License

BSD-2-Clause
