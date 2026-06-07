use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tungstenite::{connect, Message};
use url::Url;
use vnt::core::Vnt;

#[derive(Debug, Clone)]
pub struct ConsoleConfig {
    pub console_url: String,
    pub room_id: String,
    pub device_name: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct ConsoleCache {
    room_id: String,
    console_url: String,
    device_id: Option<String>,
    device_token: Option<String>,
    last_config_push: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    Hello {
        room_id: String,
        device_id: Option<String>,
        device_token: Option<String>,
        device_name: String,
        protocol_version: u32,
    },
    EventReport {
        event_type: String,
        payload: serde_json::Value,
        timestamp: u64,
    },
    Heartbeat {
        timestamp: u64,
    },
    TrafficStats {
        up_stream: u64,
        down_stream: u64,
        timestamp: u64,
    },
    ConfigAck {
        config_version: u32,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ServerMessage {
    HelloAck {
        device_id: String,
        device_token: Option<String>,
        status: String,
    },
    ConfigPush {
        version: u32,
        encrypted_config: String,
        nonce: String,
    },
    Kick {
        reason: String,
    },
    Heartbeat {
        timestamp: u64,
    },
}

pub fn parse_console_args() -> anyhow::Result<Option<ConsoleConfig>> {
    let args = std::env::args().collect::<Vec<_>>();
    let console_url = opt_value(&args, "-C", "--console");
    let room_id = opt_value(&args, "-r", "--room-id");
    if console_url.is_none() && room_id.is_none() {
        return Ok(None);
    }
    let console_url = console_url.ok_or_else(|| anyhow!("--console is required in hub mode"))?;
    let room_id = room_id.ok_or_else(|| anyhow!("--room-id is required in hub mode"))?;
    validate_room_id(&room_id)?;
    let device_name = opt_value(&args, "-n", "")
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(default_device_name);
    Ok(Some(ConsoleConfig {
        console_url,
        room_id,
        device_name,
    }))
}

pub fn run(config: ConsoleConfig) -> anyhow::Result<()> {
    println!("vnt hub console mode");
    println!("console: {}", config.console_url);
    println!("room_id: {}", config.room_id);
    println!("device_name: {}", config.device_name);

    loop {
        if let Err(e) = connect_once(&config) {
            log::warn!("hub console disconnected: {:?}", e);
            println!("hub console disconnected: {:?}, retry in 5s", e);
            std::thread::sleep(Duration::from_secs(5));
        }
    }
}

fn connect_once(config: &ConsoleConfig) -> anyhow::Result<()> {
    let mut cache = load_cache().unwrap_or_default();
    if cache.room_id != config.room_id || cache.console_url != config.console_url {
        cache = ConsoleCache {
            room_id: config.room_id.clone(),
            console_url: config.console_url.clone(),
            ..Default::default()
        };
    }

    let url = normalize_console_url(&config.console_url)?;
    let (mut socket, _) = connect(url).context("connect hub websocket")?;
    let hello = ClientMessage::Hello {
        room_id: config.room_id.clone(),
        device_id: cache.device_id.clone(),
        device_token: cache.device_token.clone(),
        device_name: config.device_name.clone(),
        protocol_version: 1,
    };
    socket.send(Message::Text(serde_json::to_string(&hello)?))?;

    let ack = socket.read()?;
    let Message::Text(ack_text) = ack else {
        return Err(anyhow!("invalid hello ack"));
    };
    let server_message = serde_json::from_str::<ServerMessage>(&ack_text)?;
    match server_message {
        ServerMessage::HelloAck {
            device_id,
            device_token,
            status,
        } => {
            println!("hub device_id: {}, status: {}", device_id, status);
            cache.device_id = Some(device_id);
            if device_token.is_some() {
                cache.device_token = device_token;
            }
            save_cache(&cache)?;
            if status == "kicked" {
                return Err(anyhow!("device kicked by hub"));
            }
        }
        _ => return Err(anyhow!("unexpected hello ack")),
    }

    send_event(
        &mut socket,
        "connected_to_console",
        serde_json::json!({
            "version": vnt::VNT_VERSION,
            "protocol_version": 1,
        }),
    )?;
    let managed_vnt: Option<ManagedVnt> = None;
    let mut last_report = std::time::Instant::now();
    loop {
        if last_report.elapsed() >= Duration::from_secs(30) {
            let now = now();
            socket.send(Message::Text(serde_json::to_string(
                &ClientMessage::Heartbeat { timestamp: now },
            )?))?;
            send_traffic_stats(&mut socket, managed_vnt.as_ref(), now)?;
            last_report = std::time::Instant::now();
        }

        match socket.read() {
            Ok(Message::Text(text)) => handle_server_message(&mut cache, &text)?,
            Ok(Message::Ping(payload)) => socket.send(Message::Pong(payload))?,
            Ok(Message::Close(_)) => return Err(anyhow!("hub websocket closed")),
            Ok(_) => {}
            Err(e) => return Err(e.into()),
        }
    }
}

fn handle_server_message(cache: &mut ConsoleCache, text: &str) -> anyhow::Result<()> {
    match serde_json::from_str::<ServerMessage>(text)? {
        ServerMessage::ConfigPush {
            version,
            encrypted_config,
            nonce,
        } => {
            cache.last_config_push = Some(serde_json::json!({
                "version": version,
                "encrypted_config": encrypted_config,
                "nonce": nonce,
                "received_at": now(),
            }));
            save_cache(cache)?;
            println!(
                "received config push version {}, encrypted snapshot cached",
                version
            );
            println!("config apply requires hub session decrypt support, pending next step");
        }
        ServerMessage::Kick { reason } => {
            let _ = fs::remove_file(cache_path());
            return Err(anyhow!("kicked by hub: {}", reason));
        }
        ServerMessage::Heartbeat { timestamp } => {
            let _ = timestamp;
        }
        ServerMessage::HelloAck { .. } => {}
    }
    Ok(())
}

fn send_event<S: Read + Write>(
    socket: &mut tungstenite::WebSocket<S>,
    event_type: &str,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    socket.send(Message::Text(serde_json::to_string(
        &ClientMessage::EventReport {
            event_type: event_type.into(),
            payload,
            timestamp: now(),
        },
    )?))?;
    Ok(())
}

struct ManagedVnt {
    vnt: Vnt,
}

impl ManagedVnt {
    fn traffic_stats(&self) -> Option<(u64, u64)> {
        if !self.vnt.config().enable_traffic {
            return None;
        }
        Some((self.vnt.up_stream(), self.vnt.down_stream()))
    }
}

fn send_traffic_stats<S: Read + Write>(
    socket: &mut tungstenite::WebSocket<S>,
    managed_vnt: Option<&ManagedVnt>,
    timestamp: u64,
) -> anyhow::Result<()> {
    let Some((up_stream, down_stream)) = managed_vnt.and_then(ManagedVnt::traffic_stats) else {
        return Ok(());
    };
    socket.send(Message::Text(serde_json::to_string(
        &ClientMessage::TrafficStats {
            up_stream,
            down_stream,
            timestamp,
        },
    )?))?;
    Ok(())
}

fn normalize_console_url(url: &str) -> anyhow::Result<Url> {
    let mut url = if url.starts_with("ws://") || url.starts_with("wss://") {
        url.to_string()
    } else {
        return Err(anyhow!(
            "hub mode currently supports ws:// or wss:// console url"
        ));
    };
    let mut parsed = Url::parse(&url)?;
    if parsed.path() == "/" {
        url = format!("{}client/ws", url.trim_end_matches('/'));
        parsed = Url::parse(&url)?;
    }
    if parsed.host_str().is_none() {
        return Err(anyhow!("hub console url host is required"));
    }
    Ok(parsed)
}

fn opt_value(args: &[String], short: &str, long: &str) -> Option<String> {
    let mut iter = args.iter().enumerate();
    while let Some((idx, arg)) = iter.next() {
        if !short.is_empty() && arg == short {
            return args.get(idx + 1).cloned();
        }
        if !long.is_empty() && arg == long {
            return args.get(idx + 1).cloned();
        }
        if !long.is_empty() {
            if let Some(value) = arg.strip_prefix(&format!("{}=", long)) {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn validate_room_id(room_id: &str) -> anyhow::Result<()> {
    if room_id.len() == 16
        && room_id
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f'))
    {
        Ok(())
    } else {
        Err(anyhow!("--room-id must be 16 lowercase hex chars"))
    }
}

fn default_device_name() -> String {
    gethostname::gethostname()
        .to_str()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("UnknownName")
        .to_string()
}

fn cache_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|v| v.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vnt-config")
        .join("vnt-console_cache.json")
}

fn load_cache() -> anyhow::Result<ConsoleCache> {
    let path = cache_path();
    let text = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn save_cache(cache: &ConsoleCache) -> anyhow::Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(cache)?)?;
    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
