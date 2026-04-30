# P5-01: Fix Discord @everyone/@here mention injection

**Phase:** 5 — Gateway Hardening
**Severity:** HIGH
**Effort:** S (<15min)
**Component:** brig-discord
**Personas:** 2/7 (adversarial-llm, security)
**Depends on:** P2-01 (gateway token auth)
**Blocks:** none

## Problem

`brig-discord/src/main.rs:551-553`: LLM text response is passed directly to Discord's `content` field with no `allowed_mentions` restriction. A compromised or prompt-injected LLM can produce `@everyone` or `@here`, pinging all server members, or `<@USER_ID>` to target specific users.

## Files to change

- `brig-discord/src/main.rs:551-553` — add `allowed_mentions` to the message body

## Fix

One line change:
```rust
let body = json!({
    "content": chunk,
    "allowed_mentions": {"parse": []}
});
```

## Verification

- LLM response containing `@everyone` does NOT ping anyone
- LLM response containing `<@12345>` does NOT ping the user
- Normal text messages still display correctly
