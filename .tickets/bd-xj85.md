---
id: bd-xj85
status: open
deps: []
links: []
created: 2026-04-14T02:56:11Z
type: feature
priority: 1
assignee: Jim Spring
tags: [multi-bot, config]
---
# Configurable gateway name and scope

## Goal

Make the gateway name and session key prefix configurable via environment variables, so
multiple instances of brig-discord can run simultaneously with distinct identities.

Memory isolation is per-user, not per-gateway — brig derives scope from the user_id in
the session key (e.g., `discord-{user_id}`). But each gateway instance still needs a
unique name for brig's audit logging and a unique session prefix to avoid session key
collisions when the same user talks to multiple bots.

## Context

Currently in `src/main.rs`, the gateway identity is hardcoded in three places:

1. **Hello handshake name** (line ~221): The `BrigHello` struct sends
   `name: "discord-gateway"` to brig in the IPC hello message. Used for audit logging
   and display (not memory scoping — memory scope is derived per-user from the session key).

2. **Session key prefix** (line ~468): `format!("discord-{}-{}-{}", guild_id, channel_id, user_id)`
   — used to track conversation continuity in brig's session database.

3. **Startup log** (line ~117): `eprintln!("brig-discord starting")` — identifies the
   instance in logs.

To run two Discord bots (e.g., an ops bot and a community bot), each instance needs a
unique name for audit/logging and a unique session prefix so the same Discord user
talking to both bots gets separate conversation histories (though their memory facts
are shared — per-user isolation, not per-gateway).

## Required Changes

### 1. Add `BRIG_GATEWAY_NAME` environment variable

Read at startup alongside the existing env vars:

```rust
let gateway_name = env::var("BRIG_GATEWAY_NAME")
    .unwrap_or_else(|_| "discord-gateway".to_string());
```

Default preserves backward compatibility.

### 2. Use gateway_name in hello handshake

In `connect_brig()` (line ~219):

```rust
// Before:
let hello = BrigHello {
    msg_type: "hello".to_string(),
    name: "discord-gateway".to_string(),
    version: "0.1.0".to_string(),
};

// After:
let hello = BrigHello {
    msg_type: "hello".to_string(),
    name: gateway_name.to_string(),
    version: "0.1.0".to_string(),
};
```

Note: `connect_brig()` currently takes only `socket_path: &str`. It will need the
gateway name passed in as well.

### 3. Add `BRIG_SESSION_PREFIX` environment variable

```rust
let session_prefix = env::var("BRIG_SESSION_PREFIX")
    .unwrap_or_else(|_| "discord".to_string());
```

Then update session key generation in `handle_message_create()` (line ~468):

```rust
// Before:
let session = format!(
    "discord-{}-{}-{}",
    msg.guild_id.as_deref().unwrap_or("dm"),
    msg.channel_id,
    msg.author.id
);

// After:
let session = format!(
    "{}-{}-{}-{}",
    session_prefix,
    msg.guild_id.as_deref().unwrap_or("dm"),
    msg.channel_id,
    msg.author.id
);
```

This ensures different bot instances produce different session keys.

### 4. Thread gateway_name and session_prefix through call chain

The current call chain is:
```
main() → run_gateway(token, socket_path)
  → connect_brig(socket_path)
  → message_loop(..., token)
    → handle_message_create(data, brig, token)
```

Both `gateway_name` and `session_prefix` need to reach their use sites. Options:
- Pass them as additional parameters down the chain
- Store them in a config struct that's passed around

Since the codebase is minimal and uses free functions, adding parameters is simplest.
Update `run_gateway()`, `connect_brig()`, `message_loop()`, and
`handle_message_create()` signatures.

### 5. Update startup logging

```rust
eprintln!("{} starting", gateway_name);
eprintln!("  socket: {}", socket_path);
eprintln!("  session prefix: {}", session_prefix);
```

### 6. Update environment variable documentation

Update the README.md to include the new variables:

| Variable              | Required | Default              | Description                           |
|-----------------------|----------|----------------------|---------------------------------------|
| `BRIG_DISCORD_TOKEN`  | Yes      | --                   | Bot token from Developer Portal       |
| `BRIG_SOCKET`         | No       | `/var/brig/sock/...` | Path to Brig's unix socket            |
| `BRIG_GATEWAY_NAME`   | No       | `discord-gateway`    | Gateway identity for brig (audit/log) |
| `BRIG_SESSION_PREFIX`  | No       | `discord`            | Session key prefix                    |

## Files to Modify

- `src/main.rs` — env var reading, connect_brig(), run_gateway(), handle_message_create()
  signatures, session key format, logging
- `README.md` — environment variable table

## Acceptance Criteria

- Default behavior unchanged: `BRIG_GATEWAY_NAME` absent → uses `"discord-gateway"`
- `BRIG_GATEWAY_NAME=discord-ops-bot` → hello message sends `name: "discord-ops-bot"`
- `BRIG_SESSION_PREFIX=disc-ops` → session keys are `disc-ops-{guild}-{channel}-{user}`
- Two instances with different names and tokens can run simultaneously
- `cargo build` succeeds
- README updated with new environment variables


## Notes

**2026-04-14T02:58:22Z**

Cross-project dependency: brig tickets bri-6yz7 (scoped facts) and bri-2ks0 (thread user identity) should be completed first for per-user memory isolation to take effect. However, this ticket can be implemented independently — the gateway will send its configured name in the hello message regardless. Memory scope is derived from user_id in the session key, not from the gateway name.
