use std::net::{Ipv4Addr, SocketAddr};
use std::process::{Command, Stdio};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Clone, Debug)]
pub struct HookInfo {
    pub script: String,
    pub event: &'static str,
    pub protocol: Option<&'static str>,
    pub local_port: Option<u16>,
    pub old_local_port: Option<u16>,
    pub remote_addr: Option<SocketAddr>,
    pub tun_name: Option<String>,
    pub device_name: Option<String>,
    pub device_id: Option<String>,
    pub virtual_ip: Option<Ipv4Addr>,
    pub server_addr: Option<String>,
    pub reconnect_count: Option<usize>,
    pub reason: Option<&'static str>,
}

impl HookInfo {
    pub fn new(script: Option<&str>, event: &'static str) -> Option<Self> {
        let script = script?;
        if script.is_empty() {
            return None;
        }
        Some(Self {
            script: script.to_string(),
            event,
            protocol: None,
            local_port: None,
            old_local_port: None,
            remote_addr: None,
            tun_name: None,
            device_name: None,
            device_id: None,
            virtual_ip: None,
            server_addr: None,
            reconnect_count: None,
            reason: None,
        })
    }
    pub fn protocol(mut self, protocol: &'static str) -> Self {
        self.protocol = Some(protocol);
        self
    }
    pub fn local_port(mut self, local_port: Option<u16>) -> Self {
        self.local_port = local_port;
        self
    }
    pub fn old_local_port(mut self, old_local_port: Option<u16>) -> Self {
        self.old_local_port = old_local_port;
        self
    }
    pub fn remote_addr(mut self, remote_addr: Option<SocketAddr>) -> Self {
        self.remote_addr = remote_addr;
        self
    }
    pub fn tun_name(mut self, tun_name: Option<String>) -> Self {
        self.tun_name = tun_name;
        self
    }
    pub fn device_name(mut self, device_name: Option<String>) -> Self {
        self.device_name = device_name;
        self
    }
    pub fn device_id(mut self, device_id: Option<String>) -> Self {
        self.device_id = device_id;
        self
    }
    pub fn virtual_ip(mut self, virtual_ip: Option<Ipv4Addr>) -> Self {
        self.virtual_ip = virtual_ip;
        self
    }
    pub fn server_addr(mut self, server_addr: Option<String>) -> Self {
        self.server_addr = server_addr;
        self
    }
    pub fn reconnect_count(mut self, reconnect_count: Option<usize>) -> Self {
        self.reconnect_count = reconnect_count;
        self
    }
    pub fn reason(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }
}

pub fn run_hook(info: HookInfo) {
    std::thread::Builder::new()
        .name("vntHook".into())
        .spawn(move || {
            #[cfg(windows)]
            let mut command = {
                let mut command = Command::new("cmd");
                command
                    .arg("/C")
                    .arg(format!("start \"\" /B cmd /C {}", info.script));
                const CREATE_NO_WINDOW: u32 = 0x08000000;
                command.creation_flags(CREATE_NO_WINDOW);
                command
            };
            #[cfg(not(windows))]
            let mut command = {
                let mut command = Command::new("sh");
                command
                    .arg("-c")
                    .arg(format!("{{\n{}\n}} >/dev/null 2>&1 &", info.script));
                command
            };
            command.env("VNT_HOOK_EVENT", info.event);
            command.env("VNT_HOOK_STATUS", info.event);
            command.env("VNT_HOOK_PROTOCOL", info.protocol.unwrap_or(""));
            command.env(
                "VNT_HOOK_LOCAL_PORT",
                info.local_port.map(|v| v.to_string()).unwrap_or_default(),
            );
            command.env(
                "VNT_HOOK_OLD_LOCAL_PORT",
                info.old_local_port
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            command.env(
                "VNT_HOOK_REMOTE_ADDR",
                info.remote_addr.map(|v| v.to_string()).unwrap_or_default(),
            );
            command.env("VNT_HOOK_TUN_NAME", info.tun_name.unwrap_or_default());
            command.env("VNT_HOOK_DEVICE_NAME", info.device_name.unwrap_or_default());
            command.env("VNT_HOOK_DEVICE_ID", info.device_id.unwrap_or_default());
            command.env(
                "VNT_HOOK_VIRTUAL_IP",
                info.virtual_ip.map(|v| v.to_string()).unwrap_or_default(),
            );
            command.env("VNT_HOOK_SERVER_ADDR", info.server_addr.unwrap_or_default());
            command.env(
                "VNT_HOOK_RECONNECT_COUNT",
                info.reconnect_count
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            command.env("VNT_HOOK_REASON", info.reason.unwrap_or(""));
            command.env("VNT_HOOK_PID", std::process::id().to_string());
            command.env(
                "VNT_HOOK_TIMESTAMP",
                crate::handle::now_time()
                    .checked_div(1000)
                    .unwrap_or(0)
                    .to_string(),
            );

            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());

            match command.spawn() {
                Ok(mut child) => {
                    if let Err(e) = child.wait() {
                        log::warn!("hook {:?} wait failed {:?}", info.script, e);
                    }
                }
                Err(e) => {
                    log::warn!("hook {:?} failed {:?}", info.script, e);
                }
            }
        })
        .ok();
}
