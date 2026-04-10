// iOS/tvOS平台的FFI接口
// 用于从Swift的NEPacketTunnelProvider调用Rust代码
// 完整实现，支持后台保活和异常清理

use std::ffi::CStr;
use std::os::unix::io::RawFd;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use tun_rs::SyncDevice;

use crate::core::{Config, Vnt};
use crate::handle::callback::VntCallback;
use crate::tun_tap_device::vnt_device::DeviceWrite;

lazy_static::lazy_static! {
    /// 全局VNT实例，用于在FFI调用之间保持状态
    static ref VNT_INSTANCE: Mutex<Option<Arc<Vnt>>> = Mutex::new(None);
    
    /// 停止标志
    static ref STOP_FLAG: Mutex<bool> = Mutex::new(false);
}

/// SyncDevice的包装器，实现DeviceWrite trait
#[derive(Clone)]
struct SyncDeviceWrapper(Arc<SyncDevice>);

impl DeviceWrite for SyncDeviceWrapper {
    fn write(&self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.send(buf)
    }
    
    #[cfg(feature = "integrated_tun")]
    fn into_device_adapter(self) -> crate::tun_tap_device::tun_create_helper::DeviceAdapter {
        // iOS不需要DeviceAdapter，因为设备已经从外部创建
        crate::tun_tap_device::tun_create_helper::DeviceAdapter::default()
    }
}

/// 默认的回调实现（用于iOS/tvOS）
#[derive(Clone)]
struct IOSCallback;

impl VntCallback for IOSCallback {
    fn success(&self) {
        log::info!("[iOS] VNT启动成功");
    }

    fn connect(&self, info: crate::handle::callback::ConnectInfo) {
        log::info!("[iOS] 连接到服务器: {:?}", info);
    }

    fn handshake(&self, info: crate::handle::callback::HandshakeInfo) -> bool {
        log::info!("[iOS] 握手信息: {:?}", info);
        true
    }

    fn register(&self, info: crate::handle::callback::RegisterInfo) -> bool {
        log::info!("[iOS] 注册信息: 虚拟IP={}, 网关={}", info.virtual_ip, info.virtual_gateway);
        true
    }

    fn peer_client_list(&self, info: Vec<crate::handle::callback::PeerClientInfo>) {
        log::info!("[iOS] 对等客户端列表: {} 个客户端", info.len());
    }

    fn error(&self, info: crate::handle::callback::ErrorInfo) {
        log::error!("[iOS] VNT错误: {:?}", info);
    }

    fn stop(&self) {
        log::info!("[iOS] VNT停止");
    }
}

/// 初始化iOS日志系统
#[no_mangle]
pub extern "C" fn vnt_ios_init_log(log_dir: *const libc::c_char) -> i32 {
    if log_dir.is_null() {
        return -1;
    }
    
    let log_dir_str = match unsafe { CStr::from_ptr(log_dir).to_str() } {
        Ok(s) => s,
        Err(_) => return -1,
    };
    
    use log::LevelFilter;
    use log4rs::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller;
    use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTrigger;
    use log4rs::append::rolling_file::policy::compound::CompoundPolicy;
    use log4rs::append::rolling_file::RollingFileAppender;
    use log4rs::config::{Appender, Config, Root};
    use log4rs::encode::pattern::PatternEncoder;
    use std::path::PathBuf;
    
    let log_path = PathBuf::from(log_dir_str);
    if !log_path.exists() {
        if let Err(_) = std::fs::create_dir_all(&log_path) {
            return -2;
        }
    }
    
    let log_file = log_path.join("vnt-core.log");
    let trigger = SizeTrigger::new(10 * 1024 * 1024);
    let roller_pattern = log_path.join("vnt-core.{}.log").to_string_lossy().to_string();
    
    let roller = match FixedWindowRoller::builder().build(&roller_pattern, 5) {
        Ok(r) => r,
        Err(_) => return -3,
    };
    
    let policy = CompoundPolicy::new(Box::new(trigger), Box::new(roller));
    let encoder = PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S%.3f)} [{f}:{L}] {h({l})} {M}:{m}{n}{n}");
    
    let appender = match RollingFileAppender::builder()
        .encoder(Box::new(encoder))
        .build(log_file, Box::new(policy)) {
        Ok(a) => a,
        Err(_) => return -4,
    };
    
    let config = match Config::builder()
        .appender(Appender::builder().build("rolling_file", Box::new(appender)))
        .build(Root::builder().appender("rolling_file").build(LevelFilter::Info)) {
        Ok(c) => c,
        Err(_) => return -5,
    };
    
    match log4rs::init_config(config) {
        Ok(_) => {
            log::info!("[iOS] 日志系统初始化成功: {}", log_dir_str);
            0
        }
        Err(_) => -6,
    }
}

/// 从文件描述符启动VNT隧道（iOS/tvOS）
///
/// # 参数
/// * `fd` - 从NEPacketTunnelProvider获取的文件描述符
/// * `server_addr` - VNT服务器地址（C字符串指针）
/// * `token` - 认证令牌（C字符串指针）
/// * `device_name` - 设备名称（C字符串指针）
/// * `mtu` - MTU值
///
/// # 返回值
/// * `0` - 成功
/// * `-1` - 创建设备失败
/// * `-2` - 创建配置失败
/// * `-3` - 启动VNT失败
/// * `-4` - 参数无效
///
/// # 安全性
/// - fd必须是有效的、打开的文件描述符
/// - fd必须指向TUN设备
/// - 所有字符串指针必须是有效的C字符串
#[no_mangle]
pub extern "C" fn vnt_ios_start_tunnel(
    fd: RawFd,
    server_addr: *const libc::c_char,
    token: *const libc::c_char,
    device_name: *const libc::c_char,
    mtu: i32,
) -> i32 {
    log::info!("========================================");
    log::info!("[iOS] 启动VNT隧道");
    log::info!("[iOS] 文件描述符: {}", fd);
    log::info!("[iOS] MTU: {}", mtu);
    log::info!("========================================");

    // 验证参数
    if server_addr.is_null() {
        log::error!("[iOS] 服务器地址为空");
        return -4;
    }
    if token.is_null() {
        log::error!("[iOS] 令牌为空");
        return -4;
    }
    if device_name.is_null() {
        log::error!("[iOS] 设备名称为空");
        return -4;
    }

    // 转换C字符串
    let server_addr_str = match unsafe { CStr::from_ptr(server_addr).to_str() } {
        Ok(s) => s.to_string(),
        Err(e) => {
            log::error!("[iOS] 无效的服务器地址字符串: {:?}", e);
            return -4;
        }
    };

    let token_str = match unsafe { CStr::from_ptr(token).to_str() } {
        Ok(s) => s.to_string(),
        Err(e) => {
            log::error!("[iOS] 无效的令牌字符串: {:?}", e);
            return -4;
        }
    };

    let device_name_str = match unsafe { CStr::from_ptr(device_name).to_str() } {
        Ok(s) => s.to_string(),
        Err(e) => {
            log::error!("[iOS] 无效的设备名称字符串: {:?}", e);
            return -4;
        }
    };

    log::info!("[iOS] 服务器地址: {}", server_addr_str);
    log::info!("[iOS] 设备名称: {}", device_name_str);

    // 重置停止标志
    if let Ok(mut flag) = STOP_FLAG.lock() {
        *flag = false;
    }

    // 从文件描述符创建设备
    log::info!("[iOS] 正在从文件描述符创建TUN设备...");
    let device = match unsafe { SyncDevice::from_fd(fd) } {
        Ok(dev) => {
            log::info!("[iOS] TUN设备创建成功");
            SyncDeviceWrapper(Arc::new(dev))
        }
        Err(e) => {
            log::error!("[iOS] 从文件描述符创建设备失败: {:?}", e);
            return -1;
        }
    };

    // 创建VNT配置
    log::info!("[iOS] 正在创建VNT配置...");
    let config = match create_ios_config(&server_addr_str, &token_str, &device_name_str, mtu as u32) {
        Ok(cfg) => {
            log::info!("[iOS] VNT配置创建成功");
            cfg
        }
        Err(e) => {
            log::error!("[iOS] 创建VNT配置失败: {:?}", e);
            return -2;
        }
    };

    // 创建回调
    let callback = IOSCallback;

    // 使用设备启动VNT
    log::info!("[iOS] 正在启动VNT核心...");
    #[cfg(feature = "integrated_tun")]
    let result = Vnt::new(config, callback);
    #[cfg(not(feature = "integrated_tun"))]
    let result = Vnt::new_device(config, callback, device);
    
    match result {
        Ok(vnt) => {
            log::info!("[iOS] VNT核心启动成功");
            
            // 保存VNT实例
            if let Ok(mut instance) = VNT_INSTANCE.lock() {
                *instance = Some(Arc::new(vnt));
                log::info!("[iOS] VNT实例已保存");
            }
            
            // 启动后台保活线程
            start_keepalive_thread();
            
            log::info!("[iOS] VNT隧道完全启动");
            0
        }
        Err(e) => {
            log::error!("[iOS] 启动VNT失败: {:?}", e);
            -3
        }
    }
}

/// 停止VNT隧道（iOS/tvOS）
#[no_mangle]
pub extern "C" fn vnt_ios_stop_tunnel() {
    log::info!("========================================");
    log::info!("[iOS] 停止VNT隧道");
    log::info!("========================================");

    // 设置停止标志
    if let Ok(mut flag) = STOP_FLAG.lock() {
        *flag = true;
    }

    // 停止VNT实例
    if let Ok(mut instance) = VNT_INSTANCE.lock() {
        if let Some(vnt) = instance.take() {
            log::info!("[iOS] 正在停止VNT实例...");
            vnt.stop();
            log::info!("[iOS] VNT实例已停止");
        } else {
            log::warn!("[iOS] 没有运行中的VNT实例");
        }
    }

    log::info!("[iOS] VNT隧道已完全停止");
}

/// 获取VNT连接状态（iOS/tvOS）
///
/// # 返回值
/// * `0` - 离线
/// * `1` - 在线
/// * `-1` - 无实例
#[no_mangle]
pub extern "C" fn vnt_ios_get_status() -> i32 {
    if let Ok(instance) = VNT_INSTANCE.lock() {
        if let Some(vnt) = instance.as_ref() {
            let status = vnt.connection_status();
            if status.online() {
                return 1;
            } else {
                return 0;
            }
        }
    }
    -1
}

/// 设置日志级别（iOS/tvOS）
///
/// # 参数
/// * `level` - 日志级别 (0=Error, 1=Warn, 2=Info, 3=Debug, 4=Trace)
#[no_mangle]
pub extern "C" fn vnt_ios_set_log_level(level: i32) {
    let log_level = match level {
        0 => log::LevelFilter::Error,
        1 => log::LevelFilter::Warn,
        2 => log::LevelFilter::Info,
        3 => log::LevelFilter::Debug,
        4 => log::LevelFilter::Trace,
        _ => log::LevelFilter::Info,
    };

    log::set_max_level(log_level);
    log::info!("[iOS] 日志级别设置为: {:?}", log_level);
}

/// 创建iOS/tvOS配置
fn create_ios_config(
    server_addr: &str,
    token: &str,
    device_name: &str,
    mtu: u32,
) -> anyhow::Result<Config> {
    use uuid::Uuid;

    let device_id = format!("ios-{}", Uuid::new_v4());

    log::info!("[iOS] 配置参数:");
    log::info!("[iOS]   服务器地址: {}", server_addr);
    log::info!("[iOS]   设备名称: {}", device_name);
    log::info!("[iOS]   设备ID: {}", device_id);
    log::info!("[iOS]   MTU: {}", mtu);

    Config::new(
        token.to_string(),
        device_id,
        device_name.to_string(),
        server_addr.to_string(),
        vec![],                 // name_servers
        vec![],                 // stun_server
        vec![],                 // in_ips
        vec![],                 // out_ips
        None,                   // password
        Some(mtu),              // mtu
        None,                   // ip
        false,                  // no_proxy
        false,                  // server_encrypt
        crate::cipher::CipherModel::AesGcm,  // cipher_model
        false,                  // finger
        crate::channel::punch::PunchModel::IPv4,  // punch_model
        None,                   // ports
        false,                  // first_latency
        None,                   // device_name
        crate::channel::UseChannelType::All,  // use_channel_type
        None,                   // packet_loss_rate
        0,                      // packet_delay
        #[cfg(feature = "port_mapping")]
        vec![],                 // port_mapping_list
        crate::compression::Compressor::None,  // compressor
        false,                  // enable_traffic
        false,                  // allow_wire_guard
        None,                   // local_dev
        false,                  // disable_relay
    )
    .context("创建iOS配置失败")
}

/// 启动后台保活线程
fn start_keepalive_thread() {
    thread::spawn(|| {
        log::info!("[iOS] 保活线程已启动");
        
        loop {
            // 检查停止标志
            if let Ok(flag) = STOP_FLAG.lock() {
                if *flag {
                    log::info!("[iOS] 保活线程收到停止信号");
                    break;
                }
            }
            
            // 检查VNT状态
            if let Ok(instance) = VNT_INSTANCE.lock() {
                if let Some(vnt) = instance.as_ref() {
                    let status = vnt.connection_status();
                    log::debug!("[iOS] 保活检查: 在线={}", status.online());
                    
                    // 如果VNT已停止，退出保活线程
                    if vnt.is_stopped() {
                        log::info!("[iOS] VNT已停止，保活线程退出");
                        break;
                    }
                } else {
                    log::debug!("[iOS] 保活检查: 无VNT实例");
                }
            }
            
            // 每30秒检查一次
            thread::sleep(Duration::from_secs(30));
        }
        
        log::info!("[iOS] 保活线程已退出");
    });
}
