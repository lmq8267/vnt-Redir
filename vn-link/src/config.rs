use anyhow::Context;
use std::net::SocketAddr;
use std::str::FromStr;

#[derive(Clone, Debug)]
pub struct VnLinkConfig {
    pub mapping: Vec<LinkItem>,
}

impl VnLinkConfig {
    pub fn new(mapping: Vec<LinkItem>) -> Self {
        Self { mapping }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum LinkProtocol {
    Tcp,
    Udp,
}

impl LinkProtocol {
    pub fn is_tcp(&self) -> bool {
        self == &LinkProtocol::Tcp
    }
}

#[derive(Copy, Clone, Debug)]
pub struct LinkItem {
    pub protocol: LinkProtocol,
    pub src_addr: SocketAddr,
    pub dest: SocketAddr,
}

impl LinkItem {
    pub fn new(protocol: LinkProtocol, src_addr: SocketAddr, dest: SocketAddr) -> Self {
        Self {
            protocol,
            src_addr,
            dest,
        }
    }
}

pub fn convert(vec: Vec<String>) -> anyhow::Result<Vec<LinkItem>> {
    let mut rs = Vec::with_capacity(vec.len());
    for x in vec {
        let string = x.trim().to_lowercase();
        if let Some(udp_mapping) = string.strip_prefix("udp:") {
            let mut split = udp_mapping.split("-");
            let bind_part = split
                .next()
                .with_context(|| format!("vnt-mapping error {:?},eg: udp:80-10.26.0.10:8080", x))?;
            let src_addr = parse_bind_addr(bind_part)
                .with_context(|| format!("udp_mapping bind error {}", bind_part))?;
            if !src_addr.ip().is_loopback() {
                Err(anyhow::anyhow!(
                    "udp vnt-mapping bind address must be 127.0.0.1 (loopback only), got {}",
                    src_addr.ip()
                ))?;
            }
            let dest = split
                .next()
                .with_context(|| format!("vnt-mapping error {:?},eg: udp:80-10.26.0.10:8080", x))?;
            let dest_addr = SocketAddr::from_str(dest)
                .with_context(|| format!("udp_mapping dest error {}", dest))?;
            rs.push(LinkItem::new(LinkProtocol::Udp, src_addr, dest_addr));
            continue;
        }
        if let Some(tcp_mapping) = string.strip_prefix("tcp:") {
            let mut split = tcp_mapping.split("-");
            let bind_part = split
                .next()
                .with_context(|| format!("vnt-mapping error {:?},eg: tcp:80-10.26.0.10:8080 or tcp:0.0.0.0:80-10.26.0.10:8080", x))?;
            let src_addr = parse_bind_addr(bind_part)
                .with_context(|| format!("tcp_mapping bind error {}", bind_part))?;
            let dest = split
                .next()
                .with_context(|| format!("vnt-mapping error {:?},eg: tcp:80-10.26.0.10:8080", x))?;
            let dest_addr = SocketAddr::from_str(dest)
                .with_context(|| format!("tcp_mapping dest error {}", dest))?;
            rs.push(LinkItem::new(LinkProtocol::Tcp, src_addr, dest_addr));
            continue;
        }
        Err(anyhow::anyhow!(
            "vnt-mapping error {:?},eg: tcp:80-10.26.0.10:8080",
            x
        ))?;
    }
    Ok(rs)
}

/// Parse bind address: bare port number defaults to 127.0.0.1, full addr:port is used as-is.
fn parse_bind_addr(s: &str) -> anyhow::Result<SocketAddr> {
    if let Ok(port) = u16::from_str(s) {
        return Ok(SocketAddr::from(([127, 0, 0, 1], port)));
    }
    SocketAddr::from_str(s).with_context(|| format!("invalid bind address {}", s))
}
