use std::io;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tun_rs::SyncDevice;

use crate::{DeviceConfig, ErrorInfo, ErrorType, VntCallback};

#[cfg(any(target_os = "windows", target_os = "linux"))]
const DEFAULT_TUN_NAME: &str = "vnt-tun";

pub fn create_device<Call: VntCallback>(
    config: DeviceConfig,
    call: &Call,
) -> Result<Arc<SyncDevice>, ErrorInfo> {
    let device = match create_device0(&config) {
        Ok(device) => device,
        Err(e) => {
            return Err(ErrorInfo::new_msg(
                ErrorType::FailedToCrateDevice,
                format!("create device {:?}", e),
            ));
        }
    };
    
    // iOS/tvOS平台的路由由NEPacketTunnelProvider管理，无需手动配置
    #[cfg(any(target_os = "ios", target_os = "tvos"))]
    {
        log::info!("iOS/tvOS平台检测到，路由配置由系统VPN框架管理");
        return Ok(device);
    }
    
    #[cfg(windows)]
    let index = device.if_index().unwrap();
    #[cfg(all(unix, not(any(target_os = "ios", target_os = "tvos"))))]
    let index = &device.name().unwrap();
    
    #[cfg(not(any(target_os = "ios", target_os = "tvos")))]
    {
        if let Err(e) = add_route(index, Ipv4Addr::BROADCAST, Ipv4Addr::BROADCAST) {
            log::warn!("添加广播路由失败 ={:?}", e);
        }

        if let Err(e) = add_route(
            index,
            Ipv4Addr::from([224, 0, 0, 0]),
            Ipv4Addr::from([240, 0, 0, 0]),
        ) {
            log::warn!("添加组播路由失败 ={:?}", e);
        }

        for (dest, mask) in config.external_route {
            if let Err(e) = add_route(index, dest, mask) {
                log::warn!("添加路由失败,请检查-i参数是否和现有路由冲突 ={:?}", e);
                call.error(ErrorInfo::new_msg(
                    ErrorType::Warn,
                    format!(
                        "警告！ 添加路由失败,请检查-i参数是否和现有路由冲突 ={:?}",
                        e
                    ),
                ))
            }
        }
    }
    Ok(device)
}

fn create_device0(config: &DeviceConfig) -> io::Result<Arc<SyncDevice>> {
    // iOS/tvOS平台：从文件描述符创建设备
    // 文件描述符应该通过FFI从Swift的NEPacketTunnelProvider传入
    #[cfg(any(target_os = "ios", target_os = "tvos"))]
    {
        // 注意：在iOS/tvOS上，设备必须从外部提供的文件描述符创建
        // 这个文件描述符来自NEPacketTunnelProvider.packetFlow
        // 实际使用时，应该通过FFI接口传入fd，这里返回错误提示
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "iOS/tvOS平台不支持直接创建TUN设备。\
             请使用SyncDevice::from_fd()从NEPacketTunnelProvider获取的文件描述符创建设备。\
             参考文档: tun-rs/docs/iOS-Integration.md"
        ));
    }
    
    #[cfg(not(any(target_os = "ios", target_os = "tvos")))]
    {
        let mut tun_builder = tun_rs::DeviceBuilder::new();
        tun_builder = tun_builder.ipv4(config.virtual_ip, config.virtual_netmask, None);

        match &config.device_name {
            None => {
                #[cfg(any(target_os = "windows", target_os = "linux"))]
                {
                    tun_builder = tun_builder.name(DEFAULT_TUN_NAME);
                }
            }
            Some(name) => {
                tun_builder = tun_builder.name(name);
            }
        }

        #[cfg(target_os = "windows")]
        {
            let name = config
                .device_name
                .clone()
                .unwrap_or_else(|| DEFAULT_TUN_NAME.to_string());
            _ = delete_adapter_info_from_reg(&name);
            tun_builder = tun_builder.metric(0).ring_capacity(4 * 1024 * 1024);
        }

        #[cfg(target_os = "linux")]
        {
            let device_name = config
                .device_name
                .clone()
                .unwrap_or(DEFAULT_TUN_NAME.to_string());
            if &device_name == DEFAULT_TUN_NAME {
                delete_device(DEFAULT_TUN_NAME);
            }
        }

        let device = tun_builder.mtu(config.mtu as u16).build_sync()?;
        Ok(Arc::new(device))
    }
}

#[cfg(target_os = "linux")]
fn delete_device(name: &str) {
    // 删除默认网卡，此操作有风险，后续可能去除
    use std::process::Command;
    let cmd = format!("ip link delete {}", name);
    let delete_tun = Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .output()
        .expect("sh exec error!");
    if !delete_tun.status.success() {
        log::warn!("删除网卡失败:{:?}", delete_tun);
    }
}
#[cfg(windows)]
fn delete_adapter_info_from_reg(dev_name: &str) -> std::io::Result<()> {
    use std::collections::HashSet;
    use winreg::{enums::HKEY_LOCAL_MACHINE, enums::KEY_ALL_ACCESS, RegKey};
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let profiles_key = hklm.open_subkey_with_flags(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\NetworkList\\Profiles",
        KEY_ALL_ACCESS,
    )?;
    let mut profile_guid_set = HashSet::new();
    for sub_key_name in profiles_key.enum_keys().filter_map(Result::ok) {
        let sub_key = profiles_key.open_subkey(&sub_key_name)?;
        match sub_key.get_value::<String, _>("Description") {
            Ok(profile_name) => {
                if dev_name == profile_name {
                    match profiles_key.delete_subkey_all(&sub_key_name) {
                        Ok(_) => {
                            log::info!("deleted Profiles sub_key: {}", sub_key_name);
                            profile_guid_set.insert(sub_key_name);
                        }
                        Err(e) => {
                            log::warn!("Failed to delete Profiles sub_key {}: {}", sub_key_name, e)
                        }
                    }
                }
            }
            Err(e) => log::warn!(
                "Failed to read Description for sub_key {}: {}",
                sub_key_name,
                e
            ),
        }
    }
    let unmanaged_key = hklm.open_subkey_with_flags(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\NetworkList\\Signatures\\Unmanaged",
        KEY_ALL_ACCESS,
    )?;
    for sub_key_name in unmanaged_key.enum_keys().filter_map(Result::ok) {
        let sub_key = unmanaged_key.open_subkey(&sub_key_name)?;
        match sub_key.get_value::<String, _>("ProfileGuid") {
            Ok(profile_guid) => {
                if profile_guid_set.contains(&profile_guid) {
                    match unmanaged_key.delete_subkey_all(&sub_key_name) {
                        Ok(_) => log::info!("deleted Unmanaged sub_key: {}", sub_key_name),
                        Err(e) => {
                            log::warn!("Failed to delete Unmanaged sub_key {}: {}", sub_key_name, e)
                        }
                    }
                }
            }
            Err(e) => log::warn!(
                "Failed to read Description for sub_key {}: {}",
                sub_key_name,
                e
            ),
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn add_route(index: u32, dest: Ipv4Addr, netmask: Ipv4Addr) -> io::Result<()> {
    let cmd = format!(
        "route add {:?} mask {:?} {:?} metric {} if {}",
        dest,
        netmask,
        Ipv4Addr::UNSPECIFIED,
        1,
        index
    );
    exe_cmd(&cmd)
}
#[cfg(target_os = "windows")]
pub fn exe_cmd(cmd: &str) -> io::Result<()> {
    use std::os::windows::process::CommandExt;

    println!("exe cmd: {}", cmd);
    let out = std::process::Command::new("cmd")
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .arg("/C")
        .arg(&cmd)
        .output()?;
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("cmd={},out={:?}", cmd, String::from_utf8(out.stderr)),
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn add_route(name: &str, address: Ipv4Addr, netmask: Ipv4Addr) -> io::Result<()> {
    let cmd = format!(
        "route -n add {} -netmask {} -interface {}",
        address, netmask, name
    );
    exe_cmd(&cmd)?;
    Ok(())
}

// iOS/tvOS平台：路由由NEPacketTunnelProvider的NEPacketTunnelNetworkSettings管理
// 不需要手动添加路由，这个函数仅用于兼容性
#[cfg(any(target_os = "ios", target_os = "tvos"))]
pub fn add_route(_name: &str, _address: Ipv4Addr, _netmask: Ipv4Addr) -> io::Result<()> {
    // iOS/tvOS上路由通过NEPacketTunnelNetworkSettings配置
    // 在Swift代码中通过以下方式设置：
    // let ipv4Settings = NEIPv4Settings(addresses: [...], subnetMasks: [...])
    // ipv4Settings.includedRoutes = [NEIPv4Route(...)]
    // tunnelNetworkSettings.ipv4Settings = ipv4Settings
    log::debug!("iOS/tvOS平台：路由配置由NEPacketTunnelProvider管理");
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn add_route(name: &str, address: Ipv4Addr, netmask: Ipv4Addr) -> io::Result<()> {
    let cmd = if netmask.is_broadcast() {
        format!("route add -host {:?} {}", address, name)
    } else {
        format!(
            "route add -net {}/{} {}",
            address,
            u32::from(netmask).count_ones(),
            name
        )
    };
    exe_cmd(&cmd)?;
    Ok(())
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn exe_cmd(cmd: &str) -> io::Result<std::process::Output> {
    use std::process::Command;
    println!("exe cmd: {}", cmd);
    let out = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .output()
        .expect("sh exec error!");
    if !out.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("cmd={},out={:?}", cmd, out),
        ));
    }
    Ok(out)
}
