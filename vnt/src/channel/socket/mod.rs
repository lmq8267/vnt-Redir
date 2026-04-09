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
    for iface in network_interfaces {
        if iface.name == dest_name {
            for addr in iface.addr {
                if let IpAddr::V4(ip) = addr.ip() {
                    return Ok((
                        LocalInterface {
                            index: iface.index,
                            #[cfg(unix)]
                            name: Some(iface.name),
                        },
                        ip,
                    ));
                }
            }
        }
    }
    Err(anyhow!("No network card with name {} found", dest_name))
}

pub fn get_default_interface() -> anyhow::Result<(LocalInterface, Ipv4Addr)> {
    use std::process::Command;
    
    #[cfg(target_os = "linux")]
    {
        let output = Command::new("ip").args(&["route", "show", "default"]).output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        // 解析 "default via 192.168.1.1 dev eth0 ..."
        for line in stdout.lines() {
            if let Some(dev_pos) = line.find(" dev ") {
                let after_dev = &line[dev_pos + 5..];
                if let Some(dev_name) = after_dev.split_whitespace().next() {
                    return get_interface(dev_name.to_string());
                }
            }
        }
    }
    
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("route").args(&["-n", "get", "default"]).output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        // 解析 "interface: en0"
        for line in stdout.lines() {
            if line.trim().starts_with("interface:") {
                if let Some(dev_name) = line.split(':').nth(1) {
                    return get_interface(dev_name.trim().to_string());
                }
            }
        }
    }
    
    #[cfg(target_os = "windows")]
    {
        // Windows 通过 route print 获取默认路由
        let output = Command::new("route").args(&["print", "0.0.0.0"]).output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // 解析 "0.0.0.0          0.0.0.0     192.168.1.1   192.168.1.100     25"
        // 格式：Network Destination  Netmask  Gateway  Interface  Metric
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("0.0.0.0") {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                // 需要至少 5 个字段：目标 掩码 网关 接口 跃点
                if parts.len() >= 5 && parts[0] == "0.0.0.0" && parts[1] == "0.0.0.0" {
                    // parts[3] 是接口 IP，通过它找到对应的网卡索引
                    if let Ok(interface_ip) = parts[3].parse::<Ipv4Addr>() {
                        let network_interfaces = NetworkInterface::show()?;
                        for iface in network_interfaces {
                            for addr in &iface.addr {
                                if let IpAddr::V4(ip) = addr.ip() {
                                    if ip == interface_ip {
                                        log::info!("自动检测到的 Windows 默认出口网卡: {} index={} ip={}", iface.name, iface.index, ip);
                                        return Ok((LocalInterface { index: iface.index }, ip));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        log::warn!("route print 命令输出无法解析到默认出口网卡，将不绑定至特定接口，内置IP代理功能可能无效");
    }
    
    Err(anyhow!("未能检测到默认网络出口网卡接口，内置IP代理功能可能无效"))
}
