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

## Install

```sh
make                     # build release binary
sudo make install        # install binary + skill manifest
```

This installs:
- `/usr/local/bin/brig-discord`
- `/usr/local/share/brig/skills/discord-gateway/manifest.toml`

Then enable via brig (jailed, recommended):

```sh
brig secret set discord-gateway.discord_token
brig skill enable discord-gateway
```

Or as a host service (no jail):

```sh
sudo make install-service
sudo sysrc brig_discord_enable=YES
sudo sysrc brig_discord_token="your-bot-token"
sudo sysrc brig_discord_user="jim"
sudo service brig_discord start
```

## Manual Run

```sh
export BRIG_DISCORD_TOKEN="your-bot-token-here"
export BRIG_SOCKET="/var/brig/sock/brig.sock"  # optional, this is the default
export BRIG_GATEWAY_NAME="discord-gateway"      # optional, identity for brig audit/logging
export BRIG_SESSION_PREFIX="discord"             # optional, session key prefix
./target/release/brig-discord
```

### Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `BRIG_DISCORD_TOKEN` | Yes | -- | Bot token from Developer Portal |
| `BRIG_TOKEN` | Yes | -- | Brig IPC authentication token (generate with `brig token create discord-gateway`) |
| `BRIG_SOCKET` | No | `~/.brig/sock/brig.sock` | Path to Brig's unix socket |
| `BRIG_GATEWAY_NAME` | No | `discord-gateway` | Gateway identity for brig (audit/log) |
| `BRIG_SESSION_PREFIX` | No | `discord` | Session key prefix |
| `BRIG_DISCORD_ALLOWED_CHANNELS` | No | -- | Comma-separated channel IDs to restrict listening (all channels if unset) |

To run multiple bot instances simultaneously, give each a unique gateway name and
session prefix:

```sh
# Instance 1: ops bot
BRIG_DISCORD_TOKEN="ops-token" BRIG_GATEWAY_NAME="discord-ops" BRIG_SESSION_PREFIX="disc-ops" ./target/release/brig-discord

# Instance 2: community bot
BRIG_DISCORD_TOKEN="community-token" BRIG_GATEWAY_NAME="discord-community" BRIG_SESSION_PREFIX="disc-community" ./target/release/brig-discord
```

## Running Multiple Bots

A single binary supports multiple independent bot instances, each with its own
Discord token, session prefix, and rc.d service.  Example manifests are in
`contrib/`.  See [docs/GUIDE.md](docs/GUIDE.md#running-multiple-bots) for the
full walkthrough.

## How It Works

1. Connects to Brig's unix socket, sends hello, receives welcome
2. Fetches Discord gateway URL from `/api/v10/gateway`
3. Connects to Discord's websocket gateway
4. Sends Identify with bot token and required intents
5. Listens for MESSAGE_CREATE events
6. For each non-bot message:
   - Formats session key as `{session_prefix}-{guild_id}-{channel_id}-{user_id}`
   - Sends task to Brig socket
   - Reads status updates until final response
   - POSTs response back to Discord channel
7. Handles heartbeats to keep the websocket alive
8. Reconnects automatically on connection loss

## Session Keys

Each conversation is identified by a session key:

```
{session_prefix}-{guild_id}-{channel_id}-{user_id}
```

The default prefix is `discord`, producing keys like `discord-123-456-789`.
For DMs, `guild_id` is `dm`. This means:
- Different users in the same channel have separate sessions
- The same user in different channels has separate sessions
- Brig's memory system tracks context per session

Brig derives per-user memory scope from session key structure: any key with
3+ hyphen-delimited segments gets scoped as `{first_segment}-{last_segment}`
(i.e., `{prefix}-{user_id}`). The prefix value itself does not need to be
registered with brig — any prefix works as long as the key has enough segments.

## Dependencies

- `ureq` — synchronous HTTP client (Discord REST API)
- `tungstenite` — synchronous websocket client (Discord Gateway)
- `serde`, `serde_json` — JSON serialization

Total: 4 crates. No async. No framework.

## License

BSD-2-Clause
