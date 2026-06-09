use anyhow::{anyhow, Context};
use base64::{engine::general_purpose, Engine as _};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as RustlsError, SignatureScheme};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::{Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{client_tls_with_config, connect, Connector, Error as WsError, Message};
use url::Url;
use vnt::channel::punch::PunchModel;
use vnt::channel::UseChannelType;
use vnt::cipher::CipherModel;
use vnt::compression::Compressor;
use vnt::core::{redact_sensitive_value, Config as VntConfig, Vnt};
use vnt::{ConnectInfo, ErrorInfo, HandshakeInfo, RegisterInfo, VntCallback};

type SharedManagedVnt = Arc<Mutex<Option<ManagedVnt>>>;
type SharedHubState = Arc<RwLock<HubRuntimeState>>;
type SharedVntsStatus = Arc<Mutex<VntsStatus>>;

enum ConsoleRead {
    Text(String),
    Timeout,
    Closed,
}

enum ConsoleConnection {
    Ws(tungstenite::WebSocket<MaybeTlsStream<TcpStream>>),
    Tcp(LineTcpConnection),
    Udp(UdpConnection),
}

struct LineTcpConnection {
    stream: TcpStream,
    read_buf: Vec<u8>,
}

struct UdpConnection {
    socket: UdpSocket,
}

#[derive(Debug, Clone, PartialEq)]
struct VntsStatus {
    status: String,
    error: Option<String>,
    updated_at: u64,
}

impl VntsStatus {
    fn new(status: &str) -> Self {
        Self {
            status: status.into(),
            error: None,
            updated_at: now(),
        }
    }
}

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
    last_config_push: Option<CachedConfigPush>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedConfigPush {
    version: u32,
    encrypted_config: String,
    nonce: String,
    received_at: u64,
}

#[derive(Debug, Clone)]
struct HubRuntimeState {
    console_url: String,
    room_id: String,
    device_id: Option<String>,
    device_name: String,
    console_version: Option<String>,
    connection_status: String,
    config_name: Option<String>,
    group_id: Option<String>,
    config_version: Option<u32>,
    config_received: bool,
    config_running: bool,
    server_address: Option<String>,
    last_error: Option<String>,
    updated_at: u64,
}

impl HubRuntimeState {
    fn new(config: &ConsoleConfig) -> Self {
        Self {
            console_url: config.console_url.clone(),
            room_id: config.room_id.clone(),
            device_id: None,
            device_name: config.device_name.clone(),
            console_version: None,
            connection_status: "starting".into(),
            config_name: None,
            group_id: None,
            config_version: None,
            config_received: false,
            config_running: false,
            server_address: None,
            last_error: None,
            updated_at: now(),
        }
    }

    fn to_hub_info(&self) -> common::command::entity::HubInfo {
        common::command::entity::HubInfo {
            hub_mode: true,
            console_url: self.console_url.clone(),
            room_id: self.room_id.clone(),
            device_id: self.device_id.clone(),
            device_name: self.device_name.clone(),
            console_version: self.console_version.clone(),
            connection_status: self.connection_status.clone(),
            config_name: self.config_name.clone(),
            group_id: self.group_id.clone(),
            config_version: self.config_version,
            config_received: self.config_received,
            config_running: self.config_running,
            server_address: self.server_address.clone(),
            last_error: self.last_error.clone(),
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ConfigPushPayload {
    device_id: String,
    group_id: Option<String>,
    config_name: Option<String>,
    config_version: u32,
    config: HubVntClientConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct HubVntClientConfig {
    pub tap: bool,
    pub token: String,
    pub device_id: String,
    pub name: String,
    pub server_address: String,
    pub stun_server: Vec<String>,
    pub dns: Vec<String>,
    pub in_ips: Vec<String>,
    pub out_ips: Vec<String>,
    pub password: Option<String>,
    pub mtu: Option<u32>,
    pub tcp: bool,
    pub ip: Option<String>,
    pub use_channel: String,
    pub no_proxy: bool,
    pub server_encrypt: bool,
    pub cipher_model: Option<String>,
    pub finger: bool,
    pub punch_model: String,
    pub ports: Option<Vec<u16>>,
    pub cmd: bool,
    pub first_latency: bool,
    pub device_name: Option<String>,
    pub packet_loss: Option<f64>,
    pub packet_delay: u32,
    pub mapping: Vec<String>,
    pub compressor: Option<String>,
    pub vnt_mapping: Vec<String>,
    pub disable_stats: bool,
    pub allow_wire_guard: bool,
    pub local_dev: Option<String>,
    pub disable_relay: bool,
    pub hook: Option<String>,
}

impl Default for HubVntClientConfig {
    fn default() -> Self {
        Self {
            tap: false,
            token: String::new(),
            device_id: String::new(),
            name: String::new(),
            server_address: "vnt.wherewego.top:29872".into(),
            stun_server: common::config::PUB_STUN
                .iter()
                .map(|v| v.to_string())
                .collect(),
            dns: vec![],
            in_ips: vec![],
            out_ips: vec![],
            password: None,
            mtu: None,
            tcp: false,
            ip: None,
            use_channel: "all".into(),
            no_proxy: false,
            server_encrypt: false,
            cipher_model: None,
            finger: false,
            punch_model: "all".into(),
            ports: None,
            cmd: false,
            first_latency: false,
            device_name: None,
            packet_loss: None,
            packet_delay: 0,
            mapping: vec![],
            compressor: None,
            vnt_mapping: vec![],
            disable_stats: false,
            allow_wire_guard: false,
            local_dev: None,
            disable_relay: false,
            hook: None,
        }
    }
}

impl HubVntClientConfig {
    fn into_vnt_config(self) -> anyhow::Result<VntConfig> {
        let token = non_empty_or(self.token, "token")?;
        let device_id = if self.device_id.trim().is_empty() {
            common::config::get_device_id()
        } else {
            self.device_id
        };
        let device_id = non_empty_or(device_id, "device_id")?;
        let name = if self.name.trim().is_empty() {
            default_device_name()
        } else {
            self.name
        };
        let name = non_empty_or(name, "name")?;
        let mut server_address = if self.server_address.trim().is_empty() {
            HubVntClientConfig::default().server_address
        } else {
            self.server_address
        };
        if self.tcp && !has_transport_scheme(&server_address) {
            server_address = format!("tcp://{}", server_address);
        }
        let stun_server = if self.stun_server.is_empty() {
            common::config::PUB_STUN
                .iter()
                .map(|v| v.to_string())
                .collect()
        } else {
            self.stun_server
        };
        let in_ips = common::args_parse::ips_parse(&self.in_ips)
            .map_err(|e| anyhow!("invalid in_ips: {}", e))?;
        let out_ips = common::args_parse::out_ips_parse(&self.out_ips)
            .map_err(|e| anyhow!("invalid out_ips: {}", e))?;
        let ip = parse_optional_ip(self.ip)?;
        let cipher_model = match self.cipher_model.filter(|v| !v.trim().is_empty()) {
            Some(model) => CipherModel::from_str(&model)
                .map_err(|e| anyhow!("invalid cipher_model {}: {}", model, e))?,
            None => default_cipher_model(),
        };
        let punch_model = PunchModel::from_str(&self.punch_model)
            .map_err(|e| anyhow!("invalid punch_model {}: {}", self.punch_model, e))?;
        let use_channel_type = UseChannelType::from_str(&self.use_channel)
            .map_err(|e| anyhow!("invalid use_channel {}: {}", self.use_channel, e))?;
        let compressor = match self.compressor.filter(|v| !v.trim().is_empty()) {
            Some(v) if v.trim().eq_ignore_ascii_case("none") => Compressor::None,
            Some(v) => {
                Compressor::from_str(&v).map_err(|e| anyhow!("invalid compressor {}: {}", v, e))?
            }
            None => Compressor::None,
        };
        let password = self.password.filter(|v| !v.is_empty());
        let _ = self.cmd;
        let _ = self.vnt_mapping;

        VntConfig::new(
            #[cfg(target_os = "windows")]
            self.tap,
            token,
            device_id,
            name,
            server_address,
            self.dns,
            stun_server,
            in_ips,
            out_ips,
            password,
            self.mtu,
            ip,
            #[cfg(feature = "ip_proxy")]
            self.no_proxy,
            self.server_encrypt,
            cipher_model,
            self.finger,
            punch_model,
            self.ports,
            self.first_latency,
            #[cfg(not(target_os = "android"))]
            self.device_name,
            use_channel_type,
            self.packet_loss,
            self.packet_delay,
            #[cfg(feature = "port_mapping")]
            self.mapping,
            compressor,
            !self.disable_stats,
            self.allow_wire_guard,
            self.local_dev,
            self.disable_relay,
            self.hook,
        )
    }
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
        client_version: Option<String>,
    },
    ConfigState {
        config_version: u32,
        group_id: Option<String>,
        config_name: Option<String>,
        config: HubVntClientConfig,
        running: bool,
    },
    EventReport {
        event_type: String,
        payload: serde_json::Value,
        timestamp: u64,
    },
    Heartbeat {
        timestamp: u64,
    },
    Disconnect {
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
        #[serde(default)]
        console_version: Option<String>,
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
    std::env::set_var("VNT_HUB_CONSOLE_REDACT", "1");
    println!("vnt hub console mode");
    println!("console: {}", config.console_url);
    println!("room_id: {}", config.room_id);
    println!("device_name: {}", config.device_name);
    println!("hub connection: connecting");

    let mut cache = load_cache().unwrap_or_default();
    if cache.room_id != config.room_id {
        cache = ConsoleCache {
            room_id: config.room_id.clone(),
            console_url: config.console_url.clone(),
            ..Default::default()
        };
    } else {
        cache.console_url = config.console_url.clone();
        if cache.room_id.is_empty() {
            cache.room_id = config.room_id.clone();
        }
    }
    let hub_state = Arc::new(RwLock::new(HubRuntimeState::new(&config)));
    let cached_vnt = match start_cached_config(&cache) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("cached hub config ignored: {:?}", e);
            println!("cached hub config ignored: {:?}", e);
            None
        }
    };
    if let Some(vnt) = cached_vnt.as_ref() {
        let cached_payload = cache
            .last_config_push
            .as_ref()
            .and_then(|push| decrypt_config_push(&cache, push).ok());
        if let Some(payload) = cached_payload.as_ref() {
            println!(
                "hub cached config: name={}, version={}, server={}",
                payload.config_name.as_deref().unwrap_or("None"),
                payload.config_version,
                redact_sensitive_value(&payload.config.server_address)
            );
            println!("hub config running: yes");
        }
        update_hub_state(&hub_state, |state| {
            state.device_id = cache.device_id.clone();
            state.connection_status = "disconnected_using_cached_config".into();
            state.config_received = true;
            state.config_running = true;
            state.config_version = Some(vnt.version);
            state.server_address = Some(redact_sensitive_value(
                &vnt.vnt.config().server_address_str,
            ));
            if let Some(payload) = cached_payload {
                state.config_name = payload.config_name;
                state.group_id = payload.group_id;
            }
        });
    }
    let managed_vnt = Arc::new(Mutex::new(cached_vnt));
    let shutdown = Arc::new(AtomicBool::new(false));
    start_hub_command_server(hub_state.clone(), managed_vnt.clone(), shutdown.clone());

    while !shutdown.load(Ordering::Relaxed) {
        update_hub_state(&hub_state, |state| {
            state.connection_status = "connecting".into();
            state.last_error = None;
        });
        if let Err(e) = connect_once(
            &config,
            &mut cache,
            managed_vnt.clone(),
            hub_state.clone(),
            shutdown.clone(),
        ) {
            log::warn!("hub console disconnected: {:?}", e);
            println!("hub connection: disconnected");
            println!("hub console disconnected: {:?}, retry in 5s", e);
            update_hub_state(&hub_state, |state| {
                state.connection_status = if state.config_running {
                    "disconnected_using_cached_config".into()
                } else {
                    "disconnected".into()
                };
                state.last_error = Some(e.to_string());
            });
            std::thread::sleep(Duration::from_secs(5));
        }
    }
    if let Some(vnt) = managed_vnt.lock().unwrap().take() {
        vnt.stop();
    }
    Ok(())
}

impl ConsoleConnection {
    fn send_json<T: Serialize>(&mut self, message: &T) -> anyhow::Result<()> {
        let text = serde_json::to_string(message)?;
        match self {
            ConsoleConnection::Ws(socket) => socket.send(Message::Text(text))?,
            ConsoleConnection::Tcp(conn) => {
                conn.stream.write_all(text.as_bytes())?;
                conn.stream.write_all(b"\n")?;
                conn.stream.flush()?;
            }
            ConsoleConnection::Udp(conn) => {
                conn.socket.send(text.as_bytes())?;
            }
        }
        Ok(())
    }

    fn read_event(&mut self) -> anyhow::Result<ConsoleRead> {
        match self {
            ConsoleConnection::Ws(socket) => loop {
                match socket.read() {
                    Ok(Message::Text(text)) => return Ok(ConsoleRead::Text(text)),
                    Ok(Message::Ping(payload)) => socket.send(Message::Pong(payload))?,
                    Ok(Message::Close(_)) => return Ok(ConsoleRead::Closed),
                    Ok(_) => {}
                    Err(WsError::Io(e))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        return Ok(ConsoleRead::Timeout);
                    }
                    Err(e) => return Err(e.into()),
                }
            },
            ConsoleConnection::Tcp(conn) => conn.read_event(),
            ConsoleConnection::Udp(conn) => conn.read_event(),
        }
    }

    fn set_read_timeout(&mut self, duration: Duration) -> anyhow::Result<()> {
        match self {
            ConsoleConnection::Ws(socket) => set_ws_read_timeout(socket, duration),
            ConsoleConnection::Tcp(conn) => {
                conn.stream.set_read_timeout(Some(duration))?;
                Ok(())
            }
            ConsoleConnection::Udp(conn) => {
                conn.socket.set_read_timeout(Some(duration))?;
                Ok(())
            }
        }
    }
}

impl LineTcpConnection {
    fn read_event(&mut self) -> anyhow::Result<ConsoleRead> {
        loop {
            if let Some(pos) = self.read_buf.iter().position(|b| *b == b'\n') {
                let mut line = self.read_buf.drain(..=pos).collect::<Vec<_>>();
                if line.last() == Some(&b'\n') {
                    line.pop();
                }
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(ConsoleRead::Text(String::from_utf8(line)?));
            }

            let mut buf = [0u8; 4096];
            match self.stream.read(&mut buf) {
                Ok(0) => return Ok(ConsoleRead::Closed),
                Ok(n) => self.read_buf.extend_from_slice(&buf[..n]),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(ConsoleRead::Timeout);
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

impl UdpConnection {
    fn read_event(&mut self) -> anyhow::Result<ConsoleRead> {
        let mut buf = [0u8; 65_535];
        match self.socket.recv(&mut buf) {
            Ok(0) => Ok(ConsoleRead::Timeout),
            Ok(n) => Ok(ConsoleRead::Text(
                std::str::from_utf8(&buf[..n])?.to_string(),
            )),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(ConsoleRead::Timeout)
            }
            Err(e) => Err(e.into()),
        }
    }
}

fn connect_once(
    config: &ConsoleConfig,
    cache: &mut ConsoleCache,
    managed_vnt: SharedManagedVnt,
    hub_state: SharedHubState,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let url = normalize_console_url(&config.console_url)?;
    let mut socket = connect_console(&url)?;
    let hello = ClientMessage::Hello {
        room_id: config.room_id.clone(),
        device_id: cache.device_id.clone(),
        device_token: cache.device_token.clone(),
        device_name: config.device_name.clone(),
        protocol_version: 1,
        client_version: Some(vnt::VNT_VERSION.into()),
    };
    socket.send_json(&hello)?;

    let ack_text = read_required_text(&mut socket, Duration::from_secs(10))?;
    let server_message = serde_json::from_str::<ServerMessage>(&ack_text)?;
    match server_message {
        ServerMessage::HelloAck {
            device_id,
            device_token,
            status,
            console_version,
        } => {
            println!("hub device_id: {}, status: {}", device_id, status);
            println!(
                "hub console version: {}",
                console_version.as_deref().unwrap_or("Unknown")
            );
            cache.device_id = Some(device_id);
            if device_token.is_some() {
                cache.device_token = device_token;
            }
            save_cache(cache)?;
            let config_running = hub_state.read().unwrap().config_running;
            if config_running {
                println!("hub connection: connected, config running");
            } else {
                println!("hub connection: connected, waiting for config push");
            }
            update_hub_state(&hub_state, |state| {
                state.device_id = cache.device_id.clone();
                state.console_version = console_version;
                state.connection_status = if state.config_running {
                    "connected_config_running".into()
                } else {
                    "connected_waiting_config".into()
                };
                state.last_error = None;
            });
            if status == "kicked" {
                return Err(anyhow!("device kicked by hub"));
            }
        }
        _ => return Err(anyhow!("unexpected hello ack")),
    }
    socket.set_read_timeout(Duration::from_secs(1))?;

    send_event(
        &mut socket,
        "connected_to_console",
        serde_json::json!({
            "client_version": vnt::VNT_VERSION,
            "protocol_version": 1,
        }),
    )?;
    report_cached_config_state(&mut socket, cache, &managed_vnt)?;
    let mut last_report = std::time::Instant::now();
    let mut last_vnts_status_report: Option<VntsStatus> = None;
    while !shutdown.load(Ordering::Relaxed) {
        if last_report.elapsed() >= Duration::from_secs(30) {
            let now = now();
            socket.send_json(&ClientMessage::Heartbeat { timestamp: now })?;
            let stats = managed_vnt
                .lock()
                .unwrap()
                .as_ref()
                .and_then(ManagedVnt::traffic_stats);
            send_traffic_stats(&mut socket, stats, now)?;
            last_report = std::time::Instant::now();
        }
        if let Some(status) = current_vnts_status(&managed_vnt) {
            if last_vnts_status_report.as_ref() != Some(&status) {
                send_vnts_status(&mut socket, &status)?;
                last_vnts_status_report = Some(status);
            }
        }

        match socket.read_event()? {
            ConsoleRead::Text(text) => {
                handle_server_message(&mut socket, cache, &managed_vnt, &hub_state, &text)?
            }
            ConsoleRead::Timeout => {}
            ConsoleRead::Closed => return Err(anyhow!("hub console connection closed")),
        }
    }
    let _ = socket.send_json(&ClientMessage::Disconnect { timestamp: now() });
    Ok(())
}

fn read_required_text(socket: &mut ConsoleConnection, timeout: Duration) -> anyhow::Result<String> {
    socket.set_read_timeout(timeout)?;
    loop {
        match socket.read_event()? {
            ConsoleRead::Text(text) => return Ok(text),
            ConsoleRead::Timeout => return Err(anyhow!("hub console read timeout")),
            ConsoleRead::Closed => return Err(anyhow!("hub console connection closed")),
        }
    }
}

fn handle_server_message(
    socket: &mut ConsoleConnection,
    cache: &mut ConsoleCache,
    managed_vnt: &SharedManagedVnt,
    hub_state: &SharedHubState,
    text: &str,
) -> anyhow::Result<()> {
    match serde_json::from_str::<ServerMessage>(text)? {
        ServerMessage::ConfigPush {
            version,
            encrypted_config,
            nonce,
        } => {
            let push = CachedConfigPush {
                version,
                encrypted_config,
                nonce,
                received_at: now(),
            };
            let (applied_version, payload) = apply_config_push(cache, &push, managed_vnt)?;
            println!(
                "hub config pushed: name={}, version={}, server={}",
                payload.config_name.as_deref().unwrap_or("None"),
                applied_version,
                redact_sensitive_value(&payload.config.server_address)
            );
            update_hub_state(hub_state, |state| {
                state.connection_status = "connected_config_running".into();
                state.config_received = true;
                state.config_running = true;
                state.config_name = payload.config_name.clone();
                state.group_id = payload.group_id.clone();
                state.config_version = Some(applied_version);
                state.server_address = Some(redact_sensitive_value(&payload.config.server_address));
                state.last_error = None;
            });
            cache.last_config_push = Some(push);
            save_cache(cache)?;
            socket.send_json(&ClientMessage::ConfigAck {
                config_version: applied_version,
            })?;
            report_config_state(socket, &payload, true)?;
            send_event(
                socket,
                "config_applied",
                serde_json::json!({ "config_version": applied_version }),
            )?;
            println!("applied hub config version {}", applied_version);
            println!("hub config running: yes");
        }
        ServerMessage::Kick { reason } => {
            if let Some(old) = managed_vnt.lock().unwrap().take() {
                old.stop();
            }
            update_hub_state(hub_state, |state| {
                state.connection_status = "kicked".into();
                state.config_running = false;
                state.last_error = Some(reason.clone());
            });
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

fn report_cached_config_state(
    socket: &mut ConsoleConnection,
    cache: &ConsoleCache,
    managed_vnt: &SharedManagedVnt,
) -> anyhow::Result<()> {
    if managed_vnt.lock().unwrap().is_none() {
        return Ok(());
    }
    let Some(push) = cache.last_config_push.as_ref() else {
        return Ok(());
    };
    let payload = decrypt_config_push(cache, push)?;
    report_config_state(socket, &payload, true)
}

fn report_config_state(
    socket: &mut ConsoleConnection,
    payload: &ConfigPushPayload,
    running: bool,
) -> anyhow::Result<()> {
    socket.send_json(&ClientMessage::ConfigState {
        config_version: payload.config_version,
        group_id: payload.group_id.clone(),
        config_name: payload.config_name.clone(),
        config: payload.config.clone(),
        running,
    })?;
    Ok(())
}

fn send_event(
    socket: &mut ConsoleConnection,
    event_type: &str,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    socket.send_json(&ClientMessage::EventReport {
        event_type: event_type.into(),
        payload,
        timestamp: now(),
    })?;
    Ok(())
}

fn send_vnts_status(socket: &mut ConsoleConnection, status: &VntsStatus) -> anyhow::Result<()> {
    send_event(
        socket,
        "vnts_status",
        serde_json::json!({
            "status": status.status,
            "error": status.error,
            "updated_at": status.updated_at,
        }),
    )
}

fn current_vnts_status(managed_vnt: &SharedManagedVnt) -> Option<VntsStatus> {
    let managed = managed_vnt.lock().unwrap();
    managed
        .as_ref()
        .map(|managed| managed.status.lock().unwrap().clone())
}

fn start_hub_command_server(
    hub_state: SharedHubState,
    managed_vnt: SharedManagedVnt,
    shutdown: Arc<AtomicBool>,
) {
    #[cfg(feature = "command")]
    {
        std::thread::Builder::new()
            .name("HubCommandServer".into())
            .spawn(move || {
                let state = hub_state.clone();
                let vnt = managed_vnt.clone();
                let shutdown_flag = shutdown.clone();
                let result =
                    common::command::server::CommandServer::new().start_with_handler(move |cmd| {
                        handle_hub_command(cmd, &state, &vnt, &shutdown_flag)
                    });
                if let Err(e) = result {
                    log::warn!("hub cmd:{:?}", e);
                }
            })
            .expect("HubCommandServer");
    }
    #[cfg(not(feature = "command"))]
    {
        let _ = hub_state;
        let _ = managed_vnt;
        let _ = shutdown;
    }
}

fn handle_hub_command(
    cmd: &str,
    hub_state: &SharedHubState,
    managed_vnt: &SharedManagedVnt,
    shutdown: &Arc<AtomicBool>,
) -> std::io::Result<String> {
    let cmd = cmd.trim();
    if cmd == "hub" {
        return Ok(
            serde_yaml::to_string(&hub_state.read().unwrap().to_hub_info())
                .unwrap_or_else(|e| format!("error {:?}", e)),
        );
    }
    if cmd == "stop" {
        shutdown.store(true, Ordering::Relaxed);
        if let Some(vnt) = managed_vnt.lock().unwrap().take() {
            vnt.stop();
        }
        return Ok("stopped".into());
    }
    if let Some(vnt) = managed_vnt.lock().unwrap().as_ref() {
        common::command::server::command_vnt(cmd, &vnt.vnt)
    } else {
        Ok(
            serde_yaml::to_string(&hub_state.read().unwrap().to_hub_info())
                .unwrap_or_else(|e| format!("error {:?}", e)),
        )
    }
}

fn update_hub_state<F>(hub_state: &SharedHubState, update: F)
where
    F: FnOnce(&mut HubRuntimeState),
{
    let mut state = hub_state.write().unwrap();
    update(&mut state);
    state.updated_at = now();
}

struct ManagedVnt {
    version: u32,
    vnt: Vnt,
    status: SharedVntsStatus,
}

impl ManagedVnt {
    fn stop(self) {
        self.vnt.stop();
        let _ = self.vnt.wait_timeout(Duration::from_secs(5));
    }

    fn traffic_stats(&self) -> Option<(u64, u64)> {
        if !self.vnt.config().enable_traffic {
            return None;
        }
        Some((self.vnt.up_stream(), self.vnt.down_stream()))
    }
}

fn start_cached_config(cache: &ConsoleCache) -> anyhow::Result<Option<ManagedVnt>> {
    let Some(push) = cache.last_config_push.as_ref() else {
        return Ok(None);
    };
    let payload = decrypt_config_push(cache, push)?;
    if payload.device_id != cache.device_id.as_deref().unwrap_or_default() {
        return Err(anyhow!("cached config device mismatch"));
    }
    let version = payload.config_version;
    let status = Arc::new(Mutex::new(VntsStatus::new("starting")));
    let vnt = Vnt::new(
        payload.config.into_vnt_config()?,
        HubVntHandler {
            status: status.clone(),
        },
    )?;
    println!("using cached hub config version {}", version);
    Ok(Some(ManagedVnt {
        version,
        vnt,
        status,
    }))
}

fn apply_config_push(
    cache: &ConsoleCache,
    push: &CachedConfigPush,
    managed_vnt: &SharedManagedVnt,
) -> anyhow::Result<(u32, ConfigPushPayload)> {
    let payload = decrypt_config_push(cache, push)?;
    let cache_device_id = cache
        .device_id
        .as_deref()
        .ok_or_else(|| anyhow!("device_id missing before config push"))?;
    if payload.device_id != cache_device_id {
        return Err(anyhow!("config push device mismatch"));
    }
    let _ = &payload.group_id;
    let version = payload.config_version;
    if managed_vnt
        .lock()
        .unwrap()
        .as_ref()
        .map(|managed| managed.version == version)
        .unwrap_or(false)
    {
        return Ok((version, payload));
    }
    if let Some(old) = managed_vnt.lock().unwrap().take() {
        old.stop();
    }
    let status = Arc::new(Mutex::new(VntsStatus::new("starting")));
    let vnt = Vnt::new(
        payload.config.clone().into_vnt_config()?,
        HubVntHandler {
            status: status.clone(),
        },
    )?;
    *managed_vnt.lock().unwrap() = Some(ManagedVnt {
        version,
        vnt,
        status,
    });
    Ok((version, payload))
}

fn decrypt_config_push(
    cache: &ConsoleCache,
    push: &CachedConfigPush,
) -> anyhow::Result<ConfigPushPayload> {
    let device_id = cache
        .device_id
        .as_deref()
        .ok_or_else(|| anyhow!("device_id missing before config push"))?;
    let device_token = cache
        .device_token
        .as_deref()
        .ok_or_else(|| anyhow!("device_token missing before config push"))?;
    let key_material = device_push_key_material(device_id, device_token);
    let context = device_push_context(device_id, push.version);
    let plaintext = decrypt_parts(&key_material, &context, &push.encrypted_config, &push.nonce)?;
    Ok(serde_json::from_str(&plaintext)?)
}

fn send_traffic_stats(
    socket: &mut ConsoleConnection,
    stats: Option<(u64, u64)>,
    timestamp: u64,
) -> anyhow::Result<()> {
    let Some((up_stream, down_stream)) = stats else {
        return Ok(());
    };
    socket.send_json(&ClientMessage::TrafficStats {
        up_stream,
        down_stream,
        timestamp,
    })?;
    Ok(())
}

fn connect_console(url: &Url) -> anyhow::Result<ConsoleConnection> {
    match url.scheme() {
        "ws" => {
            let (socket, _) = connect(url.clone()).context("connect hub websocket")?;
            Ok(ConsoleConnection::Ws(socket))
        }
        "wss" => {
            let host = url
                .host_str()
                .ok_or_else(|| anyhow!("hub console url host is required"))?;
            let port = url
                .port_or_known_default()
                .ok_or_else(|| anyhow!("hub console url port is required"))?;
            let stream = TcpStream::connect((host, port)).context("connect hub tcp socket")?;
            stream.set_nodelay(true).ok();
            let connector = Connector::Rustls(Arc::new(insecure_tls_config()?));
            let (socket, _) = client_tls_with_config(url.as_str(), stream, None, Some(connector))
                .map_err(|e| anyhow!("connect hub websocket: {:?}", e))?;
            Ok(ConsoleConnection::Ws(socket))
        }
        "tcp" => {
            let addr = resolve_console_addr(url)?;
            let stream = TcpStream::connect(addr).context("connect hub raw tcp socket")?;
            stream.set_nodelay(true).ok();
            Ok(ConsoleConnection::Tcp(LineTcpConnection {
                stream,
                read_buf: Vec::new(),
            }))
        }
        "udp" => {
            let addr = resolve_console_addr(url)?;
            let bind_addr = if addr.is_ipv6() {
                "[::]:0"
            } else {
                "0.0.0.0:0"
            };
            let socket = UdpSocket::bind(bind_addr).context("bind hub udp socket")?;
            socket.connect(addr).context("connect hub udp socket")?;
            Ok(ConsoleConnection::Udp(UdpConnection { socket }))
        }
        _ => Err(anyhow!(
            "hub mode supports ws://, wss://, tcp:// or udp:// console url"
        )),
    }
}

fn insecure_tls_config() -> anyhow::Result<ClientConfig> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Ok(ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
        .with_no_client_auth())
}

#[derive(Debug)]
struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

fn set_ws_read_timeout(
    socket: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>,
    duration: Duration,
) -> anyhow::Result<()> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(duration))?,
        MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(Some(duration))?,
        _ => {}
    }
    Ok(())
}

fn normalize_console_url(url: &str) -> anyhow::Result<Url> {
    let url = if url.starts_with("ws://")
        || url.starts_with("wss://")
        || url.starts_with("tcp://")
        || url.starts_with("udp://")
    {
        url.to_string()
    } else {
        return Err(anyhow!(
            "hub mode supports ws://, wss://, tcp:// or udp:// console url"
        ));
    };
    let mut parsed = Url::parse(&url)?;
    if matches!(parsed.scheme(), "ws" | "wss") && (parsed.path().is_empty() || parsed.path() == "/")
    {
        parsed = Url::parse(&format!("{}/client/ws", url.trim_end_matches('/')))?;
    }
    if parsed.host_str().is_none() {
        return Err(anyhow!("hub console url host is required"));
    }
    if parsed.port_or_known_default().is_none() {
        return Err(anyhow!("hub console url port is required"));
    }
    Ok(parsed)
}

fn resolve_console_addr(url: &Url) -> anyhow::Result<SocketAddr> {
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("hub console url host is required"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("hub console url port is required"))?;
    (host, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow!("hub console address cannot be resolved"))
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
    cache_dir().join("vnt-console_cache.enc")
}

fn legacy_cache_path() -> PathBuf {
    cache_dir().join("vnt-console_cache.json")
}

fn cache_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|v| v.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vnt-config")
}

fn load_cache() -> anyhow::Result<ConsoleCache> {
    if let Ok(text) = fs::read_to_string(cache_path()) {
        return decrypt_cache(&text).or_else(|_| Ok(serde_json::from_str(&text)?));
    }
    let text = fs::read_to_string(legacy_cache_path())?;
    Ok(serde_json::from_str(&text)?)
}

fn save_cache(cache: &ConsoleCache) -> anyhow::Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let plaintext = serde_json::to_vec(cache)?;
    fs::write(path, encrypt_cache(&plaintext)?)?;
    Ok(())
}

fn encrypt_cache(plaintext: &[u8]) -> anyhow::Result<String> {
    let key = cache_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|_| anyhow!("cache encrypt failed"))?;
    Ok(format!(
        "v1:{}:{}",
        general_purpose::STANDARD.encode(nonce),
        general_purpose::STANDARD.encode(ciphertext)
    ))
}

fn decrypt_cache(text: &str) -> anyhow::Result<ConsoleCache> {
    let mut parts = text.trim().split(':');
    if parts.next() != Some("v1") {
        return Err(anyhow!("unsupported cache format"));
    }
    let nonce = parts
        .next()
        .ok_or_else(|| anyhow!("cache nonce missing"))
        .and_then(|v| general_purpose::STANDARD.decode(v).map_err(Into::into))?;
    let ciphertext = parts
        .next()
        .ok_or_else(|| anyhow!("cache ciphertext missing"))
        .and_then(|v| general_purpose::STANDARD.decode(v).map_err(Into::into))?;
    if parts.next().is_some() || nonce.len() != 12 {
        return Err(anyhow!("invalid cache format"));
    }
    let key = cache_key();
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow!("cache decrypt failed"))?;
    Ok(serde_json::from_slice(&plaintext)?)
}

fn cache_key() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"vnt-console-cache:v1");
    hasher.update(common::config::get_device_id().as_bytes());
    hasher.update(b":");
    hasher.update(default_device_name().as_bytes());
    hasher.finalize().into()
}

fn device_push_key_material(device_id: &str, device_token: &str) -> String {
    format!("{}:{}", device_id, device_token)
}

fn device_push_context(device_id: &str, version: u32) -> String {
    format!("push:{}:{}", device_id, version)
}

fn derive_key(master: &str, context: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(master.as_bytes());
    hasher.update(b":");
    hasher.update(context.as_bytes());
    hasher.finalize().into()
}

fn decrypt_parts(
    master: &str,
    context: &str,
    ciphertext: &str,
    nonce: &str,
) -> anyhow::Result<String> {
    let ciphertext = general_purpose::STANDARD
        .decode(ciphertext)
        .context("invalid config ciphertext base64")?;
    let nonce = general_purpose::STANDARD
        .decode(nonce)
        .context("invalid config nonce base64")?;
    if nonce.len() != 12 {
        return Err(anyhow!("invalid config nonce length"));
    }
    let key = derive_key(master, context);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| anyhow!("config decrypt failed"))?;
    Ok(String::from_utf8(plaintext)?)
}

fn non_empty_or(value: String, field: &str) -> anyhow::Result<String> {
    if value.trim().is_empty() {
        Err(anyhow!("hub config {} is required", field))
    } else {
        Ok(value)
    }
}

fn has_transport_scheme(value: &str) -> bool {
    value.starts_with("udp://")
        || value.starts_with("tcp://")
        || value.starts_with("ws://")
        || value.starts_with("wss://")
}

fn parse_optional_ip(value: Option<String>) -> anyhow::Result<Option<Ipv4Addr>> {
    let Some(value) = value.filter(|v| !v.trim().is_empty()) else {
        return Ok(None);
    };
    let ip = Ipv4Addr::from_str(&value).with_context(|| format!("invalid ip {}", value))?;
    if ip.is_unspecified() || ip.is_broadcast() || ip.is_multicast() {
        return Err(anyhow!("invalid ip {}", ip));
    }
    Ok(Some(ip))
}

fn default_cipher_model() -> CipherModel {
    #[cfg(any(feature = "aes_gcm", feature = "server_encrypt"))]
    {
        CipherModel::AesGcm
    }
    #[cfg(not(any(feature = "aes_gcm", feature = "server_encrypt")))]
    {
        CipherModel::None
    }
}

#[derive(Clone)]
struct HubVntHandler {
    status: SharedVntsStatus,
}

impl fmt::Debug for HubVntHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HubVntHandler")
    }
}

impl VntCallback for HubVntHandler {
    fn success(&self) {
        update_vnts_status(&self.status, "connected", None);
        println!("vnt server connected");
    }

    fn connect(&self, _info: ConnectInfo) {
        update_vnts_status(&self.status, "connecting", None);
        println!("vnt server connect event");
    }

    fn handshake(&self, _info: HandshakeInfo) -> bool {
        true
    }

    fn register(&self, _info: RegisterInfo) -> bool {
        true
    }

    fn error(&self, info: ErrorInfo) {
        let status = if info.code == vnt::ErrorType::Disconnect {
            "disconnected"
        } else {
            "error"
        };
        update_vnts_status(&self.status, status, Some(info.to_string()));
        log::error!("vnt error {:?}", info.code);
        println!("vnt error {:?}", info.code);
    }

    fn stop(&self) {
        update_vnts_status(&self.status, "stopped", None);
        println!("vnt stopped");
    }
}

fn update_vnts_status(status: &SharedVntsStatus, value: &str, error: Option<String>) {
    let mut status = status.lock().unwrap();
    status.status = value.into();
    status.error = error;
    status.updated_at = now();
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
