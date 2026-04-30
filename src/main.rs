//! brig-discord: Discord gateway for Brig
//!
//! Bridges Discord messages to Brig's unix domain socket.
//! No async, no framework - just synchronous websocket + HTTP.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tungstenite::{connect, Message, WebSocket};
use tungstenite::stream::MaybeTlsStream;

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
const USER_AGENT: &str = concat!(
    "DiscordBot (https://github.com/jmspring/brig-discord, ",
    env!("CARGO_PKG_VERSION"),
    ")"
);

// Gateway intents: GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT
const INTENTS: u64 = (1 << 9) | (1 << 12) | (1 << 15);

// sysexits.h exit codes
const EX_USAGE: i32 = 64;          // command line usage error or config error
#[allow(dead_code)]
const EX_UNAVAILABLE: i32 = 69;    // service unavailable (connection failure)
#[allow(dead_code)]
const EX_PROTOCOL: i32 = 76;       // remote protocol error

// --- Discord Gateway Protocol Types ---

#[derive(Debug, Deserialize)]
struct GatewayPayload {
    op: u8,
    d: Option<Value>,
    s: Option<u64>,
    t: Option<String>,
}

#[derive(Debug, Serialize)]
struct GatewayIdentify {
    token: String,
    intents: u64,
    properties: IdentifyProperties,
}

#[derive(Debug, Serialize)]
struct IdentifyProperties {
    os: String,
    browser: String,
    device: String,
}

#[derive(Debug, Deserialize)]
struct HelloPayload {
    heartbeat_interval: u64,
}

#[derive(Debug, Deserialize)]
struct MessageCreate {
    #[allow(dead_code)]
    id: String,
    channel_id: String,
    guild_id: Option<String>,
    author: Author,
    content: String,
}

#[derive(Debug, Deserialize)]
struct Author {
    id: String,
    username: String,
    bot: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct GatewayUrl {
    url: String,
}

// --- Brig Socket Protocol Types ---

#[derive(Debug, Serialize)]
struct BrigHello {
    #[serde(rename = "type")]
    msg_type: String,
    name: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrigMessage {
    #[serde(rename = "type")]
    msg_type: String,
    content: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Serialize)]
struct BrigTask {
    #[serde(rename = "type")]
    msg_type: String,
    content: String,
    session: String,
}

// --- Main ---

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("brig-discord — Discord gateway for Brig");
        println!();
        println!("Usage: brig-discord");
        println!();
        println!("Environment variables:");
        println!("  BRIG_DISCORD_TOKEN    Discord bot token (required)");
        println!("  BRIG_TOKEN            Brig IPC authentication token (required)");
        println!("  BRIG_SOCKET           Socket path (default: ~/.brig/sock/brig.sock)");
        println!("  BRIG_GATEWAY_NAME     Gateway name (default: discord-gateway)");
        println!("  BRIG_SESSION_PREFIX   Session prefix (default: discord)");
        println!("  BRIG_DISCORD_ALLOWED_CHANNELS  Comma-separated channel IDs to listen in");
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("brig-discord {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let token = env::var("BRIG_DISCORD_TOKEN").unwrap_or_else(|_| {
        eprintln!("brig-discord: config: BRIG_DISCORD_TOKEN not set — get a bot token from https://discord.com/developers/applications");
        std::process::exit(EX_USAGE);
    });

    let brig_token = match env::var("BRIG_TOKEN") {
        Ok(t) => Some(t),
        Err(_) => {
            eprintln!("brig-discord: config: BRIG_TOKEN not set — generate one with: brig token create discord-gateway");
            std::process::exit(EX_USAGE);
        }
    };

    let socket_path = std::env::var("BRIG_SOCKET").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let user_path = format!("{}/.brig/sock/brig.sock", home);
        if std::path::Path::new(&user_path).exists() {
            user_path
        } else {
            "/var/brig/sock/brig.sock".into()
        }
    });

    let gateway_name = env::var("BRIG_GATEWAY_NAME")
        .unwrap_or_else(|_| "discord-gateway".to_string());

    let session_prefix = env::var("BRIG_SESSION_PREFIX")
        .unwrap_or_else(|_| "discord".to_string());

    let allowed_channels: Option<Vec<String>> = env::var("BRIG_DISCORD_ALLOWED_CHANNELS")
        .ok()
        .map(|s| s.split(',').map(|id| id.trim().to_string()).collect());

    eprintln!("{} starting", gateway_name);
    eprintln!("  socket: {}", socket_path);
    eprintln!("  session prefix: {}", session_prefix);
    if let Some(ref channels) = allowed_channels {
        eprintln!("  allowed channels: {}", channels.join(", "));
    }

    loop {
        if let Err(e) = run_gateway(&token, &brig_token, &socket_path, &gateway_name, &session_prefix, &allowed_channels) {
            eprintln!("brig-discord: gateway: {}", e);
            eprintln!("brig-discord: gateway: reconnecting in 5 seconds");
            thread::sleep(Duration::from_secs(5));
        }
    }
}

struct BrigConn {
    socket_path: String,
    gateway_name: String,
    brig_token: Option<String>,
}

fn run_gateway(token: &str, brig_token: &Option<String>, socket_path: &str, gateway_name: &str, session_prefix: &str, allowed_channels: &Option<Vec<String>>) -> Result<(), Box<dyn std::error::Error>> {
    // Connect to brig socket
    let mut brig = connect_brig(socket_path, gateway_name, brig_token)?;
    eprintln!("connected to brig socket");

    // Get Discord gateway URL
    let gateway_url = get_gateway_url(token)?;
    eprintln!("discord gateway: {}", gateway_url);

    // Connect to Discord gateway websocket
    let ws_url = format!("{}/?v=10&encoding=json", gateway_url);
    let (mut ws, _response) = connect(&ws_url)?;
    eprintln!("connected to discord gateway");

    // Receive Hello (opcode 10)
    let hello = read_gateway_message(&mut ws)?;
    if hello.op != 10 {
        return Err(format!("brig-discord: protocol: expected Hello (op 10), got op {}", hello.op).into());
    }
    let hello_data: HelloPayload = serde_json::from_value(
        hello.d.ok_or("missing Hello payload")?
    )?;
    let heartbeat_interval = hello_data.heartbeat_interval;
    eprintln!("heartbeat interval: {}ms", heartbeat_interval);

    // Send Identify (opcode 2)
    let identify = json!({
        "op": 2,
        "d": GatewayIdentify {
            token: token.to_string(),
            intents: INTENTS,
            properties: IdentifyProperties {
                os: "freebsd".to_string(),
                browser: "brig-discord".to_string(),
                device: "brig-discord".to_string(),
            },
        }
    });
    ws.send(Message::Text(serde_json::to_string(&identify)?))?;
    eprintln!("sent Identify");

    // Wait for Ready (opcode 0, type READY)
    let ready = read_gateway_message(&mut ws)?;
    if ready.op != 0 || ready.t.as_deref() != Some("READY") {
        return Err(format!("brig-discord: protocol: expected READY, got op {} type {:?}", ready.op, ready.t).into());
    }
    eprintln!("received READY - bot is online");

    // Shared state for heartbeat thread
    let sequence = Arc::new(AtomicU64::new(0));
    let running = Arc::new(AtomicBool::new(true));
    let last_ack = Arc::new(AtomicBool::new(true));

    // Spawn heartbeat thread
    let heartbeat_sequence = Arc::clone(&sequence);
    let heartbeat_running = Arc::clone(&running);
    let heartbeat_ack = Arc::clone(&last_ack);

    // Spawn heartbeat monitoring thread
    // Note: tungstenite doesn't support concurrent access, so we handle
    // heartbeats in the main loop. This thread just tracks timing.
    let heartbeat_handle = thread::spawn(move || {
        heartbeat_loop(
            heartbeat_interval,
            heartbeat_sequence,
            heartbeat_running,
            heartbeat_ack,
        );
    });

    let brig_conn = BrigConn {
        socket_path: socket_path.to_string(),
        gateway_name: gateway_name.to_string(),
        brig_token: brig_token.clone(),
    };

    // Main message loop
    let result = message_loop(&mut ws, &mut brig, &sequence, &last_ack, token, session_prefix, allowed_channels, heartbeat_interval, &brig_conn);

    // Clean shutdown
    running.store(false, Ordering::SeqCst);
    let _ = heartbeat_handle.join();

    result
}

fn connect_brig(socket_path: &str, gateway_name: &str, brig_token: &Option<String>) -> Result<BufReader<UnixStream>, Box<dyn std::error::Error>> {
    let stream = UnixStream::connect(socket_path)
        .map_err(|e| format!("brig-discord: socket: cannot connect to {}: {}", socket_path, e))?;

    stream.set_read_timeout(Some(Duration::from_secs(300)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;

    let mut reader = BufReader::new(stream);

    // Send hello
    let hello = BrigHello {
        msg_type: "hello".to_string(),
        name: gateway_name.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        token: brig_token.clone(),
    };
    writeln!(reader.get_mut(), "{}", serde_json::to_string(&hello)?)?;
    reader.get_mut().flush()?;

    // Read welcome
    let line = read_line_bounded(&mut reader, BRIG_MAX_MESSAGE_BYTES)?;
    let welcome: BrigMessage = serde_json::from_str(&line)?;

    if welcome.msg_type == "error" {
        return Err(format!(
            "brig-discord: socket: brig rejected connection: {} - {}",
            welcome.code.unwrap_or_default(),
            welcome.message.unwrap_or_default()
        ).into());
    }

    if welcome.msg_type != "welcome" {
        return Err(format!("brig-discord: protocol: expected welcome, got {}", welcome.msg_type).into());
    }

    eprintln!("brig capabilities: {:?}", welcome.capabilities);
    Ok(reader)
}

/// Reconnect to brig socket with exponential backoff (max 3 attempts).
fn reconnect_brig(
    socket_path: &str,
    gateway_name: &str,
    brig_token: &Option<String>,
) -> Result<BufReader<UnixStream>, Box<dyn std::error::Error>> {
    let delays = [1, 2, 4]; // seconds
    for (attempt, delay) in delays.iter().enumerate() {
        eprintln!(
            "brig-discord: socket: reconnecting (attempt {}/{})",
            attempt + 1,
            delays.len()
        );
        thread::sleep(Duration::from_secs(*delay));
        match connect_brig(socket_path, gateway_name, brig_token) {
            Ok(reader) => {
                eprintln!("brig-discord: socket: reconnected");
                return Ok(reader);
            }
            Err(e) => {
                eprintln!("brig-discord: socket: reconnect failed: {}", e);
            }
        }
    }
    Err("brig-discord: socket: all reconnect attempts failed".into())
}

fn get_gateway_url(token: &str) -> Result<String, Box<dyn std::error::Error>> {
    let response = ureq::get(&format!("{}/gateway", DISCORD_API_BASE))
        .set("Authorization", &format!("Bot {}", token))
        .set("User-Agent", USER_AGENT)
        .call()?;

    let body = response.into_string()?;
    let gateway: GatewayUrl = serde_json::from_str(&body)?;
    Ok(gateway.url)
}

fn read_gateway_message(
    ws: &mut WebSocket<MaybeTlsStream<TcpStream>>
) -> Result<GatewayPayload, Box<dyn std::error::Error>> {
    loop {
        match ws.read()? {
            Message::Text(text) => {
                return Ok(serde_json::from_str(&text)?);
            }
            Message::Binary(data) => {
                return Ok(serde_json::from_slice(&data)?);
            }
            Message::Ping(data) => {
                ws.send(Message::Pong(data))?;
            }
            Message::Pong(_) => {}
            Message::Close(_) => {
                return Err("websocket closed by server".into());
            }
            Message::Frame(_) => {}
        }
    }
}

fn heartbeat_loop(
    interval_ms: u64,
    _sequence: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    last_ack: Arc<AtomicBool>,
) {
    // tungstenite doesn't support concurrent access, so heartbeats are
    // sent from the main loop. This thread monitors whether ACKs are
    // being received to detect zombie connections.

    let interval = Duration::from_millis(interval_ms);
    let mut last_beat = Instant::now();

    while running.load(Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(100));

        if last_beat.elapsed() >= interval {
            if !last_ack.load(Ordering::SeqCst) {
                // No ACK received for last heartbeat - connection is zombie
                eprintln!("brig-discord: heartbeat: no ACK received, connection may be dead");
            }
            // Signal that a heartbeat is due (main loop will send it)
            last_ack.store(false, Ordering::SeqCst);
            last_beat = Instant::now();
        }
    }
}

fn message_loop(
    ws: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    brig: &mut BufReader<UnixStream>,
    sequence: &Arc<AtomicU64>,
    last_ack: &Arc<AtomicBool>,
    token: &str,
    session_prefix: &str,
    allowed_channels: &Option<Vec<String>>,
    heartbeat_interval_ms: u64,
    brig_conn: &BrigConn,
) -> Result<(), Box<dyn std::error::Error>> {
    // Set non-blocking for the websocket so we can periodically send heartbeats
    match ws.get_mut() {
        MaybeTlsStream::Plain(s) => s.set_nonblocking(true)?,
        MaybeTlsStream::Rustls(s) => s.get_ref().set_nonblocking(true)?,
        _ => {} // Best effort for other stream types
    }

    let mut last_heartbeat = Instant::now();
    let heartbeat_interval = Duration::from_millis(heartbeat_interval_ms);

    loop {
        // Check if heartbeat is due
        if last_heartbeat.elapsed() >= heartbeat_interval {
            let seq = sequence.load(Ordering::SeqCst);
            let heartbeat = if seq > 0 {
                json!({"op": 1, "d": seq})
            } else {
                json!({"op": 1, "d": null})
            };
            ws.send(Message::Text(serde_json::to_string(&heartbeat)?))?;
            last_heartbeat = Instant::now();
        }

        // Try to read a message (non-blocking)
        match ws.read() {
            Ok(msg) => {
                match msg {
                    Message::Text(text) => {
                        if let Err(e) = handle_gateway_message(&text, ws, brig, sequence, last_ack, token, session_prefix, allowed_channels, brig_conn) {
                            eprintln!("brig-discord: dispatch: {}", e);
                        }
                    }
                    Message::Binary(data) => {
                        if let Ok(text) = String::from_utf8(data) {
                            if let Err(e) = handle_gateway_message(&text, ws, brig, sequence, last_ack, token, session_prefix, allowed_channels, brig_conn) {
                                eprintln!("brig-discord: dispatch: {}", e);
                            }
                        }
                    }
                    Message::Ping(data) => {
                        ws.send(Message::Pong(data))?;
                    }
                    Message::Close(frame) => {
                        eprintln!("brig-discord: gateway: websocket closed: {:?}", frame);
                        return Err("websocket closed".into());
                    }
                    _ => {}
                }
            }
            Err(tungstenite::Error::Io(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // No message available, sleep briefly
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }
}

fn handle_gateway_message(
    text: &str,
    ws: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    brig: &mut BufReader<UnixStream>,
    sequence: &Arc<AtomicU64>,
    last_ack: &Arc<AtomicBool>,
    token: &str,
    session_prefix: &str,
    allowed_channels: &Option<Vec<String>>,
    brig_conn: &BrigConn,
) -> Result<(), Box<dyn std::error::Error>> {
    let payload: GatewayPayload = serde_json::from_str(text)?;

    // Update sequence number
    if let Some(s) = payload.s {
        sequence.store(s, Ordering::SeqCst);
    }

    match payload.op {
        // Dispatch (event)
        0 => {
            if let Some(ref event_type) = payload.t {
                if event_type == "MESSAGE_CREATE" {
                    if let Some(d) = payload.d {
                        handle_message_create(d, brig, token, session_prefix, allowed_channels, brig_conn)?;
                    }
                }
            }
        }
        // Heartbeat requested
        1 => {
            let seq = sequence.load(Ordering::SeqCst);
            let heartbeat = if seq > 0 {
                json!({"op": 1, "d": seq})
            } else {
                json!({"op": 1, "d": null})
            };
            ws.send(Message::Text(serde_json::to_string(&heartbeat)?))?;
        }
        // Reconnect
        7 => {
            eprintln!("brig-discord: gateway: server requested reconnect");
            return Err("reconnect requested".into());
        }
        // Invalid session
        9 => {
            eprintln!("brig-discord: gateway: invalid session, will reconnect");
            return Err("invalid session".into());
        }
        // Hello (shouldn't happen mid-session)
        10 => {
            eprintln!("brig-discord: protocol: unexpected Hello mid-session");
        }
        // Heartbeat ACK
        11 => {
            last_ack.store(true, Ordering::SeqCst);
        }
        _ => {
            eprintln!("brig-discord: protocol: unknown opcode {}", payload.op);
        }
    }

    Ok(())
}

fn handle_message_create(
    data: Value,
    brig: &mut BufReader<UnixStream>,
    token: &str,
    session_prefix: &str,
    allowed_channels: &Option<Vec<String>>,
    brig_conn: &BrigConn,
) -> Result<(), Box<dyn std::error::Error>> {
    let msg: MessageCreate = serde_json::from_value(data)?;

    // Ignore bot messages
    if msg.author.bot.unwrap_or(false) {
        return Ok(());
    }

    // Ignore messages from non-allowed channels
    if let Some(ref allowed) = allowed_channels {
        if !allowed.contains(&msg.channel_id) {
            return Ok(());
        }
    }

    // Ignore empty messages
    if msg.content.trim().is_empty() {
        return Ok(());
    }

    let preview: String = if msg.content.chars().count() > 50 {
        let truncated: String = msg.content.chars().take(50).collect();
        format!("{}...", truncated)
    } else {
        msg.content.clone()
    };
    eprintln!(
        "message from {} in channel {}: {}",
        msg.author.username,
        msg.channel_id,
        preview,
    );

    // Format session key
    let session = format!(
        "{}-{}-{}-{}",
        session_prefix,
        msg.guild_id.as_deref().unwrap_or("dm"),
        msg.channel_id,
        msg.author.id
    );

    // Send task to brig (with reconnection on failure)
    let task = BrigTask {
        msg_type: "task".to_string(),
        content: msg.content,
        session,
    };
    let task_json = serde_json::to_string(&task)?;

    let response_content = match send_and_read_brig(brig, &task_json) {
        Ok(content) => content,
        Err(e) => {
            eprintln!("brig-discord: socket: send/read failed: {}", e);
            // Attempt reconnection
            let mut new_brig = reconnect_brig(
                &brig_conn.socket_path,
                &brig_conn.gateway_name,
                &brig_conn.brig_token,
            )?;
            let content = send_and_read_brig(&mut new_brig, &task_json)?;
            // Replace the old reader with the reconnected one
            *brig = new_brig;
            content
        }
    };

    // Send response back to Discord
    send_discord_message(token, &msg.channel_id, &response_content)?;

    Ok(())
}

fn read_line_bounded(reader: &mut BufReader<UnixStream>, max_bytes: usize) -> Result<String, String> {
    let mut line = String::new();
    loop {
        let available = reader.fill_buf().map_err(|e| format!("read error: {}", e))?;
        if available.is_empty() {
            if line.is_empty() { return Err("connection closed".into()); }
            return Ok(line);
        }
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            line.push_str(&String::from_utf8_lossy(&available[..=pos]));
            reader.consume(pos + 1);
            return Ok(line);
        }
        if line.len() + available.len() > max_bytes {
            return Err(format!("message exceeds {} byte limit", max_bytes));
        }
        line.push_str(&String::from_utf8_lossy(available));
        let len = available.len();
        reader.consume(len);
    }
}

const BRIG_MAX_MESSAGE_BYTES: usize = 1_048_576; // 1 MB

fn send_and_read_brig(
    brig: &mut BufReader<UnixStream>,
    task_json: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    writeln!(brig.get_mut(), "{}", task_json)?;
    brig.get_mut().flush()?;
    read_brig_response(brig)
}

fn read_brig_response(
    brig: &mut BufReader<UnixStream>
) -> Result<String, Box<dyn std::error::Error>> {
    loop {
        let line = read_line_bounded(brig, BRIG_MAX_MESSAGE_BYTES)?;

        let msg: BrigMessage = serde_json::from_str(&line)?;

        match msg.msg_type.as_str() {
            "response" => {
                return Ok(msg.content.unwrap_or_else(|| "(no response)".to_string()));
            }
            "status" => {
                // Intermediate status, keep reading
                continue;
            }
            "error" => {
                return Ok(format!(
                    "Error: {} - {}",
                    msg.code.unwrap_or_default(),
                    msg.message.unwrap_or_default()
                ));
            }
            other => {
                eprintln!("brig-discord: protocol: unexpected brig message type: {}", other);
                continue;
            }
        }
    }
}

fn send_discord_message(
    token: &str,
    channel_id: &str,
    content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Discord has a 2000 character limit per message
    // Split long messages if needed
    let chunks = split_message(content, 2000);

    let num_chunks = chunks.len();
    for chunk in chunks {
        let url = format!("{}/channels/{}/messages", DISCORD_API_BASE, channel_id);
        let body = json!({
            "content": chunk,
            "allowed_mentions": {"parse": []}
        });

        let json_body = serde_json::to_string(&body)?;
        let response = ureq::post(&url)
            .set("Authorization", &format!("Bot {}", token))
            .set("User-Agent", USER_AGENT)
            .set("Content-Type", "application/json")
            .send_string(&json_body);

        match response {
            Ok(_) => {}
            Err(ureq::Error::Status(code, response)) => {
                let err_body = response.into_string().unwrap_or_default();
                eprintln!("brig-discord: discord: API error {}: {}", code, err_body);
                // Don't fail completely, just log and continue
            }
            Err(e) => {
                let err: Box<dyn std::error::Error> = Box::new(e);
                return Err(err);
            }
        }

        // Rate limiting: small delay between chunks
        if num_chunks > 1 {
            thread::sleep(Duration::from_millis(500));
        }
    }

    Ok(())
}

fn split_message(content: &str, max_len: usize) -> Vec<&str> {
    if content.len() <= max_len {
        return vec![content];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < content.len() {
        if start + max_len >= content.len() {
            chunks.push(&content[start..]);
            break;
        }

        // Find the last char boundary at or before start + max_len
        let mut split_at = start + max_len;
        while split_at > start && !content.is_char_boundary(split_at) {
            split_at -= 1;
        }
        if split_at == start {
            // Single character wider than max_len — shouldn't happen in practice
            // but advance past it to avoid infinite loop
            split_at = start + content[start..].chars().next().map_or(1, |c| c.len_utf8());
        }

        let chunk = &content[start..split_at];

        // Try to split at a newline or space for readability
        if let Some(pos) = chunk.rfind('\n') {
            let end = start + pos + 1;
            chunks.push(&content[start..end]);
            start = end;
        } else if let Some(pos) = chunk.rfind(' ') {
            let end = start + pos + 1;
            chunks.push(&content[start..end]);
            start = end;
        } else {
            chunks.push(chunk);
            start = split_at;
        }
    }

    chunks
}
