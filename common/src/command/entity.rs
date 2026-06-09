use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};

#[derive(Serialize, Deserialize, Debug)]
pub struct Info {
    pub name: String,
    pub virtual_ip: String,
    pub virtual_gateway: String,
    pub virtual_netmask: String,
    pub connect_status: String,
    pub relay_server: String,
    pub nat_type: String,
    pub public_ips: String,
    pub local_addr: String,
    pub ipv6_addr: String,
    pub port_mapping_list: Vec<(bool, SocketAddr, String)>,
    pub in_ips: Vec<(u32, u32, Ipv4Addr)>,
    pub out_ips: Vec<(u32, u32)>,
    pub udp_listen_addr: Vec<String>,
    pub tcp_listen_addr: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RouteItem {
    pub destination: String,
    pub next_hop: String,
    pub metric: String,
    pub rt: String,
    pub interface: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeviceItem {
    pub name: String,
    pub virtual_ip: String,
    pub nat_type: String,
    pub public_ips: String,
    pub local_ip: String,
    pub ipv6: String,
    pub nat_traversal_type: String,
    pub rt: String,
    pub status: String,
    pub client_secret: bool,
    pub client_secret_hash: Vec<u8>,
    pub current_client_secret: bool,
    pub current_client_secret_hash: Vec<u8>,
    pub wire_guard: bool,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ChartA {
    pub disable_stats: bool,
    pub up_total: u64,
    pub down_total: u64,
    pub up_map: HashMap<Ipv4Addr, u64>,
    pub down_map: HashMap<Ipv4Addr, u64>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ChartB {
    pub disable_stats: bool,
    pub ip: Option<Ipv4Addr>,
    pub up_total: u64,
    pub up_list: Vec<usize>,
    pub down_total: u64,
    pub down_list: Vec<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HubInfo {
    pub hub_mode: bool,
    pub console_url: String,
    pub room_id: String,
    pub device_id: Option<String>,
    pub device_name: String,
    pub console_version: Option<String>,
    pub connection_status: String,
    pub config_name: Option<String>,
    pub group_id: Option<String>,
    pub config_version: Option<u32>,
    pub config_received: bool,
    pub config_running: bool,
    pub server_address: Option<String>,
    pub last_error: Option<String>,
    pub updated_at: u64,
}

impl HubInfo {
    pub fn not_hub() -> Self {
        Self {
            hub_mode: false,
            console_url: String::new(),
            room_id: String::new(),
            device_id: None,
            device_name: String::new(),
            console_version: None,
            connection_status: "not_hub".into(),
            config_name: None,
            group_id: None,
            config_version: None,
            config_received: false,
            config_running: false,
            server_address: None,
            last_error: None,
            updated_at: 0,
        }
    }
}
