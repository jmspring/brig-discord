# P10-02: Unify Discord gateway to single TLS stack

**Phase:** 10 — Dependency Optimization
**Severity:** HIGH
**Effort:** S (<30min)
**Component:** brig-discord
**Personas:** 1/7 (rust-minimal-deps)
**Depends on:** none
**Blocks:** none

## Problem

`ureq` uses `rustls` (pure Rust TLS). `tungstenite` with `features = ["native-tls"]` uses `native-tls`/`openssl`. The binary links both OpenSSL and ring, doubling TLS compile cost and binary size. 145 transitive deps for 4 direct deps.

## Files to change

- `brig-discord/Cargo.toml:12` — change tungstenite features

## Fix

```toml
# Before:
tungstenite = { version = "0.21", features = ["native-tls"] }
# After:
tungstenite = { version = "0.21", default-features = false, features = ["rustls-tls-webpki-roots"] }
```

## Verification

- `cargo build` succeeds
- Discord WebSocket connection works with rustls
- `cargo tree | grep -c "^"` shows ~12 fewer crates
- No openssl/native-tls in dependency tree
