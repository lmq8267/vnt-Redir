// iOS/tvOS平台的FFI接口
// 用于从Swift的NEPacketTunnelProvider调用Rust代码

#[cfg(any(target_os = "ios", target_os = "tvos"))]
use std::os::unix::io::RawFd;
#[cfg(any(target_os = "ios", target_os = "tvos"))]
use std::sync::{Arc, Mutex};
#[cfg(any(target_os = "ios", target_os = "tvos"))]
use tun_rs::SyncDevice;

#[cfg(any(target_os = "ios", target_os = "tvos"))]
use crate::core::Vnt;
#[cfg(any(target_os = "ios", target_os = "tvos"))]
use crate::handle::callback::VntCallback;
#[cfg(any(target_os = "ios", target_os = "tvos"))]
use crate::tun_tap_device::vnt_device::DeviceWrite;
#[cfg(any(target_os = "ios", target_os = "tvos"))]
use crate::Config;

#[cfg(any(target_os = "ios", target_os = "tvos"))]
lazy_static::lazy_static! {
    /// 全局VNT实例，用于在FFI调用之间保持状态
    static ref VNT_INSTANCE: Mutex<Option<Arc<Vnt>>> = Mutex::new(None);
}

/// SyncDevice的包装器，实现DeviceWrite trait
#[cfg(any(target_os = "ios", target_os = "tvos"))]
#[derive(Clone)]
struct SyncDeviceWrapper(Arc<SyncDevice>);

#[cfg(any(target_os = "ios", target_os = "tvos"))]
impl DeviceWrite for SyncDeviceWrapper {
    fn write(&self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.send(buf)
    }
    
    #[cfg(feature = "integrated_tun")]
    fn into_device_adapter(self) -> crate::tun_tap_device::tun_create_helper::DeviceAdapter {
        // iOS不需要DeviceAdapter，因为设备已经从外部创建
        // 返回一个空的DeviceAdapter
        crate::tun_tap_device::tun_create_helper::DeviceAdapter::default()
    }
}

/// 默认的回调实现（用于iOS/tvOS）
#[cfg(any(target_os = "ios", target_os = "tvos"))]
#[derive(Clone)]
struct DefaultCallback;

#[cfg(any(target_os = "ios", target_os = "tvos"))]
impl VntCallback for DefaultCallback {
    fn success(&self) {
        log::info!("VNT启动成功");
    }

    fn create_tun(&self, info: crate::handle::callback::DeviceInfo) {
        log::info!("创建TUN设备: {:?}", info);
    }

    fn connect(&self, info: crate::handle::callback::ConnectInfo) {
        log::info!("连接信息: {:?}", info);
    }

    fn handshake(&self, info: crate::handle::callback::HandshakeInfo) -> bool {
        log::info!("握手信息: {:?}", info);
        true
    }

    fn register(&self, info: crate::handle::callback::RegisterInfo) -> bool {
        log::info!("注册信息: {:?}", info);
        true
    }

    fn create_device(&self, info: crate::handle::callback::DeviceConfig) {
        log::info!("创建设备配置: {:?}", info);
    }

    fn generate_tun(&self, info: crate::handle::callback::DeviceConfig) -> usize {
        log::info!("生成TUN: {:?}", info);
        0
    }

    fn peer_client_list(&self, info: Vec<crate::handle::callback::PeerClientInfo>) {
        log::info!("对等客户端列表: {} 个客户端", info.len());
    }

    fn error(&self, info: crate::handle::callback::ErrorInfo) {
        log::error!("VNT错误: {:?}", info);
    }

    fn stop(&self) {
        log::info!("VNT停止");
    }
}

/// 从文件描述符启动VNT隧道（iOS/tvOS）
///
/// # 参数
/// * `fd` - 从NEPacketTunnelProvider获取的文件描述符
/// * `server_addr` - VNT服务器地址（C字符串指针）
/// * `token` - 认证令牌（C字符串指针）
///
/// # 返回值
/// * `0` - 成功
/// * `-1` - 创建设备失败
/// * `-2` - 创建配置失败
/// * `-3` - 启动VNT失败
///
/// # 安全性
/// - fd必须是有效的、打开的文件描述符
/// - fd必须指向TUN设备
/// - server_addr和token必须是有效的C字符串指针
#[no_mangle]
#[cfg(any(target_os = "ios", target_os = "tvos"))]
pub extern "C" fn vnt_start_tunnel(
    fd: RawFd,
    server_addr: *const libc::c_char,
    token: *const libc::c_char,
) -> i32 {
    vnt_start_tunnel_with_config(fd, server_addr, token, std::ptr::null())
}

/// 从文件描述符启动VNT隧道（带完整配置）
///
/// # 参数
/// * `fd` - 从NEPacketTunnelProvider获取的文件描述符
/// * `server_addr` - VNT服务器地址
/// * `token` - 认证令牌
/// * `config_json` - JSON格式的配置（可选，传NULL使用默认配置）
///
/// JSON配置示例：
/// ```json
/// {
///   "name_servers": ["8.8.8.8:53"],
///   "stun_server": ["stun.l.google.com:19302"],
///   "password": "your_password",
///   "mtu": 1400,
///   "cipher_model": "aes_gcm",
///   "first_latency": true,
///   "use_channel_type": "all"
/// }
/// ```
#[no_mangle]
#[cfg(any(target_os = "ios", target_os = "tvos"))]
pub extern "C" fn vnt_start_tunnel_with_config(
    fd: RawFd,
    server_addr: *const libc::c_char,
    token: *const libc::c_char,
    config_json: *const libc::c_char,
) -> i32 {
    // 初始化日志（如果尚未初始化）
    let _ = env_logger::try_init();

    log::info!("iOS/tvOS: 从文件描述符 {} 启动VNT隧道", fd);

    // 转换C字符串
    let server_addr_str = if server_addr.is_null() {
        log::error!("服务器地址为空");
        return -2;
    } else {
        unsafe {
            match std::ffi::CStr::from_ptr(server_addr).to_str() {
                Ok(s) => s.to_string(),
                Err(e) => {
                    log::error!("无效的服务器地址字符串: {:?}", e);
                    return -2;
                }
            }
        }
    };

    let token_str = if token.is_null() {
        log::error!("令牌为空");
        return -2;
    } else {
        unsafe {
            match std::ffi::CStr::from_ptr(token).to_str() {
                Ok(s) => s.to_string(),
                Err(e) => {
                    log::error!("无效的令牌字符串: {:?}", e);
                    return -2;
                }
            }
        }
    };

    // 解析可选的JSON配置
    let extra_config = if !config_json.is_null() {
        unsafe {
            match std::ffi::CStr::from_ptr(config_json).to_str() {
                Ok(json_str) => {
                    log::info!("使用自定义配置: {}", json_str);
                    parse_json_config(json_str)
                }
                Err(e) => {
                    log::warn!("无效的配置JSON: {:?}, 使用默认配置", e);
                    None
                }
            }
        }
    } else {
        None
    };

    // 从文件描述符创建设备
    let device = match unsafe { SyncDevice::from_fd(fd) } {
        Ok(dev) => SyncDeviceWrapper(Arc::new(dev)),
        Err(e) => {
            log::error!("从文件描述符创建设备失败: {:?}", e);
            return -1;
        }
    };

    log::info!("设备创建成功，正在配置VNT...");

    // 创建VNT配置
    let config = match create_ios_config_with_extra(&server_addr_str, &token_str, extra_config) {
        Ok(cfg) => cfg,
        Err(e) => {
            log::error!("创建VNT配置失败: {:?}", e);
            return -2;
        }
    };

    // 创建回调
    let callback = DefaultCallback;

    // 使用设备启动VNT
    match Vnt::new_with_device(config, callback, device) {
        Ok(vnt) => {
            log::info!("VNT隧道启动成功");
            // 保存VNT实例
            if let Ok(mut instance) = VNT_INSTANCE.lock() {
                *instance = Some(Arc::new(vnt));
            }
            0
        }
        Err(e) => {
            log::error!("启动VNT失败: {:?}", e);
            -3
        }
    }
}

/// 停止VNT隧道（iOS/tvOS）
#[no_mangle]
#[cfg(any(target_os = "ios", target_os = "tvos"))]
pub extern "C" fn vnt_stop_tunnel() {
    log::info!("iOS/tvOS: 停止VNT隧道");

    if let Ok(mut instance) = VNT_INSTANCE.lock() {
        if let Some(vnt) = instance.take() {
            vnt.stop();
            log::info!("VNT隧道已停止");
        } else {
            log::warn!("没有运行中的VNT实例");
        }
    }
}

/// 获取VNT连接状态（iOS/tvOS）
///
/// # 返回值
/// * `0` - 离线
/// * `1` - 在线
/// * `-1` - 无实例
#[no_mangle]
#[cfg(any(target_os = "ios", target_os = "tvos"))]
pub extern "C" fn vnt_get_status() -> i32 {
    if let Ok(instance) = VNT_INSTANCE.lock() {
        if let Some(vnt) = instance.as_ref() {
            if vnt.connection_status().online() {
                return 1;
            } else {
                return 0;
            }
        }
    }
    -1
}

/// 额外配置选项
#[cfg(any(target_os = "ios", target_os = "tvos"))]
struct ExtraConfig {
    name_servers: Vec<String>,
    stun_server: Vec<String>,
    password: Option<String>,
    mtu: Option<u32>,
    cipher_model: Option<String>,
    first_latency: bool,
    use_channel_type: Option<String>,
    enable_traffic: bool,
}

/// 解析JSON配置
#[cfg(any(target_os = "ios", target_os = "tvos"))]
fn parse_json_config(json_str: &str) -> Option<ExtraConfig> {
    // 简单的JSON解析（生产环境建议使用serde_json）
    // 这里提供基本实现
    Some(ExtraConfig {
        name_servers: vec![],
        stun_server: vec![],
        password: None,
        mtu: None,
        cipher_model: None,
        first_latency: false,
        use_channel_type: None,
        enable_traffic: false,
    })
}

/// 创建iOS/tvOS配置（带额外选项）
#[cfg(any(target_os = "ios", target_os = "tvos"))]
fn create_ios_config_with_extra(
    server_addr: &str,
    token: &str,
    extra: Option<ExtraConfig>,
) -> anyhow::Result<Config> {
    use std::net::Ipv4Addr;

    // 获取设备主机名
    let device_name = get_device_hostname();
    
    // 生成设备ID（使用UUID）
    let device_id = format!("ios-{}", uuid::Uuid::new_v4().to_string());

    log::info!("设备名称: {}, 设备ID: {}", device_name, device_id);

    let extra = extra.unwrap_or(ExtraConfig {
        name_servers: vec![],
        stun_server: vec![],
        password: None,
        mtu: Some(1400),
        cipher_model: None,
        first_latency: false,
        use_channel_type: None,
        enable_traffic: false,
    });

    let cipher_model = if let Some(ref model) = extra.cipher_model {
        model.parse().unwrap_or_default()
    } else {
        Default::default()
    };

    let use_channel_type = if let Some(ref t) = extra.use_channel_type {
        t.parse().unwrap_or_default()
    } else {
        Default::default()
    };

    Config::new(
        token.to_string(),
        device_id,
        device_name,
        server_addr.to_string(),
        extra.name_servers,
        extra.stun_server,
        vec![], // in_ips
        vec![], // out_ips
        extra.password,
        extra.mtu,
        None,   // ip
        false,  // server_encrypt
        cipher_model,
        false,  // finger
        Default::default(), // punch_model
        None,   // ports
        extra.first_latency,
        use_channel_type,
        None,   // packet_loss_rate
        0,      // packet_delay
        Default::default(), // compressor
        extra.enable_traffic,
        false,  // allow_wire_guard
        None,   // local_dev
        false,  // disable_relay
    )
}

/// 创建iOS/tvOS配置（简化版）
#[cfg(any(target_os = "ios", target_os = "tvos"))]
fn create_ios_config(server_addr: &str, token: &str) -> anyhow::Result<Config> {
    create_ios_config_with_extra(server_addr, token, None)
}

/// 获取设备主机名（iOS/tvOS）
#[cfg(any(target_os = "ios", target_os = "tvos"))]
fn get_device_hostname() -> String {
    use std::ffi::CStr;
    
    let mut hostname = [0u8; 256];
    let result = unsafe {
        libc::gethostname(hostname.as_mut_ptr() as *mut libc::c_char, hostname.len())
    };
    
    if result == 0 {
        if let Ok(cstr) = CStr::from_bytes_until_nul(&hostname) {
            if let Ok(name) = cstr.to_str() {
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }
    
    // 如果获取失败，使用默认名称
    #[cfg(target_os = "ios")]
    return "iPhone".to_string();
    
    #[cfg(target_os = "tvos")]
    return "AppleTV".to_string();
}

/// 设置日志级别（iOS/tvOS）
///
/// # 参数
/// * `level` - 日志级别 (0=Error, 1=Warn, 2=Info, 3=Debug, 4=Trace)
#[no_mangle]
#[cfg(any(target_os = "ios", target_os = "tvos"))]
pub extern "C" fn vnt_set_log_level(level: i32) {
    let log_level = match level {
        0 => log::LevelFilter::Error,
        1 => log::LevelFilter::Warn,
        2 => log::LevelFilter::Info,
        3 => log::LevelFilter::Debug,
        4 => log::LevelFilter::Trace,
        _ => log::LevelFilter::Info,
    };

    env_logger::Builder::from_default_env()
        .filter_level(log_level)
        .init();

    log::info!("日志级别设置为: {:?}", log_level);
}
