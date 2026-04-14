use anyhow::{anyhow, Context};
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use socket2::Protocol;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub trait VntSocketTrait {
    fn set_ip_unicast_if(&self, _interface: &LocalInterface) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct LocalInterface {
    index: u32,
    #[cfg(unix)]
    name: Option<String>,
}

pub async fn connect_tcp(
    addr: SocketAddr,
    bind_port: u16,
    default_interface: &LocalInterface,
) -> anyhow::Result<tokio::net::TcpStream> {
    let socket = create_tcp0(addr.is_ipv4(), bind_port, default_interface)?;
    Ok(socket.connect(addr).await?)
}
pub fn create_tcp(
    v4: bool,
    default_interface: &LocalInterface,
) -> anyhow::Result<tokio::net::TcpSocket> {
    create_tcp0(v4, 0, default_interface)
}
pub fn create_tcp0(
    v4: bool,
    bind_port: u16,
    default_interface: &LocalInterface,
) -> anyhow::Result<tokio::net::TcpSocket> {
    let socket = if v4 {
        socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::STREAM,
            Some(Protocol::TCP),
        )?
    } else {
        socket2::Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::STREAM,
            Some(Protocol::TCP),
        )?
    };
    if v4 {
        #[cfg(not(any(target_os = "ios", target_os = "tvos")))]
        if let Err(e) = socket.set_ip_unicast_if(default_interface) {
            log::warn!("set_ip_unicast_if {:?}", e)
        }
    }
    if bind_port != 0 {
        socket
            .set_reuse_address(true)
            .context("set_reuse_address")?;
        #[cfg(unix)]
        if let Err(e) = socket.set_reuse_port(true) {
            log::warn!("set_reuse_port {:?}", e)
        }
        if v4 {
            let addr: SocketAddr = format!("0.0.0.0:{}", bind_port).parse().unwrap();
            socket.bind(&addr.into())?;
        } else {
            socket.set_only_v6(true)?;
            let addr: SocketAddr = format!("[::]:{}", bind_port).parse().unwrap();
            socket.bind(&addr.into())?;
        }
    }
    socket.set_nonblocking(true)?;
    socket.set_nodelay(true)?;
    Ok(tokio::net::TcpSocket::from_std_stream(socket.into()))
}
pub fn bind_udp_ops(
    addr: SocketAddr,
    only_v6: bool,
    default_interface: &LocalInterface,
) -> anyhow::Result<socket2::Socket> {
    let socket = if addr.is_ipv4() {
        let socket = socket2::Socket::new(
            socket2::Domain::IPV4,
            socket2::Type::DGRAM,
            Some(Protocol::UDP),
        )?;
        #[cfg(not(any(target_os = "ios", target_os = "tvos")))]
        if let Err(e) = socket.set_ip_unicast_if(default_interface) {
            log::warn!("set_ip_unicast_if {:?}", e)
        }
        socket
    } else {
        let socket = socket2::Socket::new(
            socket2::Domain::IPV6,
            socket2::Type::DGRAM,
            Some(Protocol::UDP),
        )?;
        socket
            .set_only_v6(only_v6)
            .with_context(|| format!("set_only_v6 failed: {}", &addr))?;
        socket
    };
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    Ok(socket)
}
pub fn bind_udp(
    addr: SocketAddr,
    default_interface: &LocalInterface,
) -> anyhow::Result<socket2::Socket> {
    bind_udp_ops(addr, true, default_interface).with_context(|| format!("{}", addr))
}

pub fn get_interface(dest_name: String) -> anyhow::Result<(LocalInterface, Ipv4Addr)> {
    let network_interfaces = NetworkInterface::show()?;
    
    // 尝试作为接口索引解析
    if let Ok(index) = dest_name.parse::<u32>() {
        for iface in &network_interfaces {
            if iface.index == index {
                for addr in &iface.addr {
                    if let IpAddr::V4(ip) = addr.ip() {
                        log::info!("按索引找到网卡: Index={}, GUID={}, IP={}", index, iface.name, ip);
                        return Ok((
                            LocalInterface {
                                index: iface.index,
                                #[cfg(unix)]
                                name: Some(iface.name.clone()),
                            },
                            ip,
                        ));
                    }
                }
            }
        }
    }
    
    // 尝试作为网卡名称匹配（GUID 或 Unix 名称）
    for iface in &network_interfaces {
        if iface.name == dest_name {
            for addr in &iface.addr {
                if let IpAddr::V4(ip) = addr.ip() {
                    log::info!("按 GUID/名称找到网卡: {}, IP={}", iface.name, ip);
                    return Ok((
                        LocalInterface {
                            index: iface.index,
                            #[cfg(unix)]
                            name: Some(iface.name.clone()),
                        },
                        ip,
                    ));
                }
            }
        }
    }
    
    // Windows: 尝试通过友好名称查找
    #[cfg(target_os = "windows")]
    {
        if let Ok((iface_index, iface_name, ip)) = get_interface_by_friendly_name(&dest_name) {
            return Ok((LocalInterface { index: iface_index }, ip));
        }
    }
    
    Err(anyhow!("未找到指定的网卡 '{}'，请检查网卡名称、索引或友好名称是否正确", dest_name))
}

#[cfg(target_os = "windows")]
fn get_interface_by_friendly_name(friendly_name: &str) -> anyhow::Result<(u32, String, Ipv4Addr)> {
    use std::ptr;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH, GAA_FLAG_INCLUDE_PREFIX,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
    
    // 分配初始缓冲区
    let mut buffer_size: u32 = 15000;
    let mut buffer: Vec<u8> = vec![0; buffer_size as usize];
    
    // 调用 GetAdaptersAddresses
    let result = unsafe {
        GetAdaptersAddresses(
            AF_INET as u32,
            GAA_FLAG_INCLUDE_PREFIX,
            ptr::null_mut(),
            buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
            &mut buffer_size,
        )
    };
    
    if result != 0 {
        return Err(anyhow!("GetAdaptersAddresses 失败，错误码: {}", result));
    }
    
    // 遍历适配器列表
    let mut adapter_ptr = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
    while !adapter_ptr.is_null() {
        let adapter = unsafe { &*adapter_ptr };
        
        // 获取友好名称
        let friendly_name_ptr = adapter.FriendlyName;
        if !friendly_name_ptr.is_null() {
            let mut len = 0;
            while unsafe { *friendly_name_ptr.offset(len) } != 0 {
                len += 1;
            }
            let friendly_name_slice = unsafe { std::slice::from_raw_parts(friendly_name_ptr, len as usize) };
            let adapter_friendly_name = String::from_utf16_lossy(friendly_name_slice);
            
            // 匹配友好名称（支持带空格的名称，如 "WLAN 5"）
            if adapter_friendly_name == friendly_name {
                let if_index = unsafe { adapter.Anonymous1.Anonymous.IfIndex };
                
                // 获取 IPv4 地址
                let mut unicast_addr_ptr = adapter.FirstUnicastAddress;
                while !unicast_addr_ptr.is_null() {
                    let unicast_addr = unsafe { &*unicast_addr_ptr };
                    let sockaddr = unicast_addr.Address.lpSockaddr;
                    
                    if !sockaddr.is_null() {
                        let family = unsafe { (*sockaddr).sa_family };
                        if family == AF_INET as u16 {
                            let sockaddr_in = sockaddr as *const SOCKADDR_IN;
                            let ip_bytes = unsafe { (*sockaddr_in).sin_addr.S_un.S_un_b };
                            let ip = Ipv4Addr::new(ip_bytes.s_b1, ip_bytes.s_b2, ip_bytes.s_b3, ip_bytes.s_b4);
                            
                            // 获取 GUID 名称
                            let adapter_name_ptr = adapter.AdapterName;
                            let adapter_name = if !adapter_name_ptr.is_null() {
                                unsafe { std::ffi::CStr::from_ptr(adapter_name_ptr as *const i8) }
                                    .to_string_lossy()
                                    .to_string()
                            } else {
                                String::from("Unknown")
                            };
                            
                            log::info!("通过友好名称 '{}' 找到网卡: GUID={}, Index={}, IP={}", 
                                friendly_name, adapter_name, if_index, ip);
                            return Ok((if_index, adapter_name, ip));
                        }
                    }
                    
                    unicast_addr_ptr = unicast_addr.Next;
                }
            }
        }
        
        adapter_ptr = adapter.Next;
    }
    
    Err(anyhow!("未找到友好名称为 '{}' 的网卡", friendly_name))
}
