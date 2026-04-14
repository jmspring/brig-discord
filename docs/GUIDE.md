# brig-discord: Installation, Configuration, and Usage Guide

## Overview

brig-discord is a gateway that bridges Discord messages to Brig. When a user
sends a message in a Discord channel where the bot is present, brig-discord
forwards it to Brig's unix domain socket, waits for the LLM-driven response,
and posts the result back to Discord.

No async runtime, no Discord bot framework. Synchronous websocket
(tungstenite) and HTTP (ureq) calls in ~600 lines.

## Prerequisites

- FreeBSD with Brig installed and running in daemon mode (`brig -d`)
- A working internet connection (the gateway needs to reach discord.com
  and gateway.discord.gg)
- Rust toolchain (for building from source)

## Step 1: Register a Discord Bot

You need a bot account on Discord before brig-discord can connect.

### Create the Application

1. Open the Discord Developer Portal: https://discord.com/developers/applications
2. Click **New Application**
3. Enter a name (e.g., "Brig") and click **Create**

### Generate a Bot Token

1. In the left sidebar, click **Bot**
2. Click **Reset Token** to generate a new bot token
3. **Copy the token immediately** -- Discord only shows it once. If you lose
   it, you must reset it again.
4. Keep this token secret. Anyone with the token can control the bot.

### Enable Required Intents

Under **Privileged Gateway Intents** on the Bot page, enable:

- **Message Content Intent** -- required for the bot to read message text.
  Without this, the bot receives message events but the `content` field is
  empty.

The bot also uses **Guild Messages** intent (non-privileged, enabled by
default).

### Invite the Bot to Your Server

1. In the left sidebar, click **OAuth2** then **URL Generator**
2. Under **Scopes**, check `bot`
3. Under **Bot Permissions**, check:
   - **Send Messages** -- so the bot can reply
   - **Read Message History** -- so the bot can see channel messages
4. Copy the generated URL at the bottom of the page
5. Open the URL in your browser
6. Select the Discord server you want to add the bot to
7. Click **Authorize**

The bot now appears in your server's member list (offline until you start the
gateway).

### Verify Bot Settings

On the Bot page, confirm these settings:

- **Public Bot**: disable this if you don't want others to invite the bot
  to their servers
- **Requires OAuth2 Code Grant**: leave disabled (not needed)

## Step 2: Build brig-discord

```sh
cd /path/to/brig-discord
cargo build --release
```

The binary is at `target/release/brig-discord`.

## Step 3: Install

### Option A: Install as a Brig Persistent Skill (Recommended)

This runs brig-discord inside a FreeBSD jail with network access restricted
to discord.com and gateway.discord.gg.

```sh
# Install the binary where the jail can reach it
sudo cp target/release/brig-discord /usr/local/bin/

# Register the skill manifest with brig
brig skill add /path/to/brig-discord/

# Store the bot token (encrypted in ~/.brig/secrets.db)
brig secret set discord-gateway.discord_token
# Paste your bot token when prompted

# Enable the skill (creates ZFS dataset, jail, rc.d script)
sudo brig skill enable discord-gateway

# Start the service
sudo sysrc brig_discord_enable=YES
sudo service brig_discord start
```

Check that it's running:

```sh
sudo service brig_discord status
```

View logs via syslog:

```sh
grep brig_discord /var/log/messages
```

### Option B: Run Manually (For Testing)

```sh
export BRIG_DISCORD_TOKEN="your-bot-token"
./target/release/brig-discord
```

The gateway prints status to stderr:

```
brig-discord starting
  socket: /var/brig/sock/brig.sock
connected to brig socket
discord gateway: wss://gateway.discord.gg
connected to discord gateway
heartbeat interval: 41250ms
sent Identify
received READY - bot is online
```

## Step 4: Configure

### Environment Variables

| Variable             | Required | Default                       | Description                     |
|----------------------|----------|-------------------------------|---------------------------------|
| `BRIG_DISCORD_TOKEN` | Yes      | --                            | Bot token from Developer Portal |
| `BRIG_SOCKET`        | No       | `/var/brig/sock/brig.sock`    | Path to Brig's unix socket      |

When installed as a persistent skill, `BRIG_DISCORD_TOKEN` is injected
automatically from brig's secret store. Override `BRIG_SOCKET` only if
your brig daemon uses a non-default socket path.

### Brig Daemon Configuration

Ensure brig is running in daemon mode and the socket is accessible:

```sh
# Start brig daemon
brig -d

# Verify the socket exists
ls -la /var/brig/sock/brig.sock
```

The gateway connects with `submit_task` and `read_status` capabilities.
No additional brig configuration is needed.

### Skill Manifest

The `manifest.toml` declares what the gateway needs:

```toml
[skill]
name = "discord-gateway"
description = "Bridge Discord messages to Brig"
kind = "persistent"

[requires]
network = ["discord.com", "gateway.discord.gg"]
max_runtime = "forever"

[persistent]
rc_name = "brig_discord"
entrypoint = "/usr/local/bin/brig-discord"
restart_on_failure = true
depends_on = ["brig"]

[persistent.socket]
capabilities = ["submit_task", "read_status"]

[secrets]
discord_token = { env = "BRIG_DISCORD_TOKEN" }
```

You shouldn't need to modify this unless your installation paths differ.

## Usage

### Basic Interaction

Once the bot is online in your Discord server, any non-bot message in a
channel where the bot has read/send permissions is forwarded to Brig:

```
User:  What's the disk usage on the system?
Bot:   All ZFS pools healthy. zroot is at 42% capacity (126G used of 300G).
```

```
User:  Create a new jail template with nginx installed
Bot:   Created template "nginx-base" with nginx-1.24 installed.
       Dataset: zroot/brig/templates/nginx-base
```

```
User:  Search memory for anything about the last deployment
Bot:   Found 2 facts:
       1. Production deployment completed 2026-04-10, all services healthy
       2. Rolled back nginx config change due to TLS cert mismatch
```

### Session Isolation

Each conversation is tracked by a session key:

```
discord-{guild_id}-{channel_id}-{user_id}
```

This means:

- **Different users in the same channel** have separate sessions and
  separate memory contexts
- **The same user in different channels** has separate sessions
- **DMs** use `dm` as the guild_id: `discord-dm-{channel_id}-{user_id}`

Brig's memory system associates facts and context with these session keys,
so conversations maintain continuity across messages.

### Long Responses

Discord enforces a 2000-character limit per message. brig-discord
automatically splits longer responses into multiple messages, breaking at
newlines or spaces. A 500ms delay between chunks prevents rate limiting.

### Error Handling

If Brig is unavailable or returns an error, the bot posts the error message
to the Discord channel so the user sees what went wrong:

```
Bot:   Error: task_failed - skill shell timed out after 120s
```

If the Discord websocket disconnects (network issues, Discord maintenance),
the gateway reconnects automatically after 5 seconds. If the Brig socket
drops, the gateway also reconnects.

## Troubleshooting

### Bot appears offline in Discord

- Check that the gateway process is running: `service brig_discord status`
- Check logs: `grep brig_discord /var/log/messages`
- Verify the token is correct: `brig secret list` should show
  `discord-gateway.discord_token`

### Bot is online but doesn't respond to messages

- Confirm **Message Content Intent** is enabled in the Developer Portal
- Check that the bot has **Send Messages** and **Read Message History**
  permissions in the channel
- Look for errors in the gateway's stderr output

### "cannot connect to brig socket"

- Verify brig is running in daemon mode: `brig status`
- Check the socket path: `ls -la /var/brig/sock/brig.sock`
- If using a custom socket path, set `BRIG_SOCKET` accordingly

### "brig rejected connection"

- The brig daemon may not be configured to accept gateway connections
- Check that the gateway's capabilities match what brig allows

## Running Multiple Bots

A single brig-discord binary can power any number of independent Discord bots.
Each bot runs as a separate brig persistent skill with its own manifest, token,
session prefix, and rc.d service name.  No code changes or rebuilds are needed.

### Why Run Multiple Bots

Common reasons:

- **Team separation** -- an ops bot in a private staff server and a community
  bot in a public server, each with different permissions.
- **Environment isolation** -- a staging bot and a production bot pointed at
  separate brig daemons.
- **Capability scoping** -- one bot with `submit_task` only, another with
  broader capabilities.

### Overview

Each bot instance requires:

1. Its own Discord application, bot token, and OAuth2 invite URL
2. A manifest file with a unique skill name, `rc_name`, and session prefix
3. Its own secret stored in brig's secret store
4. A separate `brig skill add` / `enable` / `start` cycle

The binary is installed once.  Every instance uses the same
`/usr/local/bin/brig-discord` binary.

### Step 1: Register Each Bot in Discord

Go to https://discord.com/developers/applications and create a **separate
application** for every bot you want to run.  Each application has its own:

- Bot token (Bot -> Reset Token)
- OAuth2 invite URL (OAuth2 -> URL Generator)
- Privileged intent settings (enable **Message Content Intent** on each)

You must invite each bot individually to the Discord servers where it should
operate.  A token for one application cannot be used by another.

### Step 2: Create a Manifest Per Bot

Example manifests are provided in `contrib/`:

| File | Bot name | rc.d service | Session prefix |
|------|----------|--------------|----------------|
| `contrib/manifest-ops.toml` | discord-ops-bot | brig_disc_ops | disc-ops |
| `contrib/manifest-community.toml` | discord-community-bot | brig_disc_community | disc-community |

Copy and adjust to fit your needs.  The important fields that must differ
between instances:

- `[skill] name` -- unique skill name for brig's registry
- `[persistent] rc_name` -- unique rc.d service name
- `[persistent] env.BRIG_GATEWAY_NAME` -- identity used in brig audit logs
- `[persistent] env.BRIG_SESSION_PREFIX` -- prefix for session keys (controls
  memory isolation)

The `entrypoint` stays the same for all instances: `/usr/local/bin/brig-discord`.

### Step 3: Install and Register Each Bot

Build and install the binary once:

```sh
cargo build --release
sudo cp target/release/brig-discord /usr/local/bin/
```

Then register each manifest as a separate skill:

```sh
# Ops bot
brig skill add /path/to/contrib/manifest-ops.toml
brig secret set discord-ops-bot.discord_token
# Paste the ops bot token when prompted
brig skill enable discord-ops-bot
sudo sysrc brig_disc_ops_enable=YES
sudo service brig_disc_ops start

# Community bot
brig skill add /path/to/contrib/manifest-community.toml
brig secret set discord-community-bot.discord_token
# Paste the community bot token when prompted
brig skill enable discord-community-bot
sudo sysrc brig_disc_community_enable=YES
sudo service brig_disc_community start
```

### Session Key Isolation

Each bot instance uses a different session prefix, which produces distinct
session keys:

```
disc-ops-{guild_id}-{channel_id}-{user_id}
disc-community-{guild_id}-{channel_id}-{user_id}
```

Because brig's memory system (facts, sessions, messages) is keyed by session,
this means:

- Conversations with the ops bot are completely separate from conversations
  with the community bot, even for the same Discord user.
- Memory facts observed by one bot are scoped to that bot's sessions and do
  not leak into the other bot's context.
- `brig recall search` and `brig memory search` can distinguish between bots
  by filtering on the session prefix.

### Per-User Memory Scoping

Within a single bot instance, each Discord user already gets an isolated
session (the session key includes the user ID).  Across multiple bot instances,
the session prefix adds another layer of separation.  The result is
per-user, per-bot memory isolation with no additional configuration.

### Managing Multiple Bots

Check status of all bots:

```sh
sudo service brig_disc_ops status
sudo service brig_disc_community status
```

View logs (each service logs under its own rc_name):

```sh
grep brig_disc_ops /var/log/messages
grep brig_disc_community /var/log/messages
```

Stop a single bot without affecting others:

```sh
sudo service brig_disc_ops stop
```

### Notes

- Each bot must be separately invited to every Discord server where it should
  respond.  Adding one bot to a server does not grant the other bot access.
- All instances share the same brig daemon.  If the daemon restarts, all bots
  reconnect automatically.
- Resource limits in each manifest apply independently.  One bot hitting its
  limit does not affect the others.

## Stopping the Service

```sh
sudo service brig_discord stop
```

To disable at boot:

```sh
sudo sysrc brig_discord_enable=NO
```

To remove the skill entirely:

```sh
sudo service brig_discord stop
sudo brig skill disable discord-gateway
brig skill remove discord-gateway
sudo rm /usr/local/bin/brig-discord
```
