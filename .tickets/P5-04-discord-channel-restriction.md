# P5-04: Add channel/role restriction to Discord gateway

**Phase:** 5 — Gateway Hardening
**Severity:** HIGH
**Effort:** S (~30min)
**Component:** brig-discord
**Personas:** 1/7 (security)
**Depends on:** P2-01 (gateway token auth)
**Blocks:** none

## Problem

`brig-discord/src/main.rs`: Any user in any channel where the bot is present can submit tasks. No channel ID or role filtering exists.

## Files to change

- `brig-discord/src/main.rs` — add `BRIG_DISCORD_ALLOWED_CHANNELS` env var and filter
- `brig-discord/README.md` — document the env var

## Fix

```rust
let allowed_channels: Option<Vec<String>> = std::env::var("BRIG_DISCORD_ALLOWED_CHANNELS")
    .ok()
    .map(|s| s.split(',').map(|id| id.trim().to_string()).collect());

// In message processing:
if let Some(ref allowed) = allowed_channels {
    if !allowed.contains(&message.channel_id) {
        continue;
    }
}
```

## Verification

- With `BRIG_DISCORD_ALLOWED_CHANNELS=123,456`, only messages from those channels are processed
- Without the env var, all channels work (backwards compatible)
