use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crossbeam_utils::atomic::AtomicCell;

use crate::channel::context::ChannelContext;
use crate::channel::idle::{Idle, IdleType};
use crate::channel::sender::{AcceptSocketSender, ConnectUtil};
use crate::channel::socket::LocalInterface;
use crate::channel::{ConnectProtocol, RouteKey};
use crate::handle::callback::{ConnectInfo, ErrorType};
use crate::handle::handshaker::Handshake;
use crate::handle::{BaseConfigInfo, ConnectStatus, CurrentDeviceInfo};
use crate::util::{address_choose, dns_query_all, run_hook, HookInfo, Scheduler};
use crate::{ErrorInfo, VntCallback};

const DEFAULT_RECONNECT_REBIND_INTERVAL: usize = 20;
const RECONNECT_REBIND_INTERVAL_ENV: &str = "VNT_REBIND";

fn reconnect_rebind_interval() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var(RECONNECT_REBIND_INTERVAL_ENV)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(DEFAULT_RECONNECT_REBIND_INTERVAL)
    })
}

pub fn idle_route<Call: VntCallback>(
    scheduler: &Scheduler,
    idle: Idle,
    context: ChannelContext,
    current_device_info: Arc<AtomicCell<CurrentDeviceInfo>>,
    config: BaseConfigInfo,
    call: Call,
) {
    let delay = idle_route0(&idle, &context, &current_device_info, &config, &call);
    let rs = scheduler.timeout(delay, move |s| {
        idle_route(s, idle, context, current_device_info, config, call)
    });
    if !rs {
        log::info!("定时任务停止");
    }
}

pub fn idle_gateway<Call: VntCallback>(
    scheduler: &Scheduler,
    context: ChannelContext,
    current_device_info: Arc<AtomicCell<CurrentDeviceInfo>>,
    config: BaseConfigInfo,
    connect_util: ConnectUtil,
    udp_socket_sender: AcceptSocketSender<Option<Vec<mio::net::UdpSocket>>>,
    call: Call,
    mut connect_count: usize,
    handshake: Handshake,
) {
    idle_gateway0(
        &context,
        &current_device_info,
        &config,
        &connect_util,
        &udp_socket_sender,
        &call,
        &mut connect_count,
        &handshake,
    );
    let rs = scheduler.timeout(Duration::from_secs(8), move |s| {
        idle_gateway(
            s,
            context,
            current_device_info,
            config,
            connect_util,
            udp_socket_sender,
            call,
            connect_count,
            handshake,
        )
    });
    if !rs {
        log::info!("定时任务停止");
    }
}

fn idle_gateway0<Call: VntCallback>(
    context: &ChannelContext,
    current_device: &AtomicCell<CurrentDeviceInfo>,
    config: &BaseConfigInfo,
    connect_util: &ConnectUtil,
    udp_socket_sender: &AcceptSocketSender<Option<Vec<mio::net::UdpSocket>>>,
    call: &Call,
    connect_count: &mut usize,
    handshake: &Handshake,
) {
    if let Err(e) = check_gateway_channel(
        context,
        current_device,
        config,
        connect_util,
        udp_socket_sender,
        call,
        connect_count,
        handshake,
    ) {
        let cur = current_device.load();
        call.error(ErrorInfo::new_msg(
            ErrorType::Disconnect,
            format!("connect:{},error:{:?}", cur.connect_server, e),
        ));
    }
}

fn idle_route0<Call: VntCallback>(
    idle: &Idle,
    context: &ChannelContext,
    current_device: &AtomicCell<CurrentDeviceInfo>,
    config: &BaseConfigInfo,
    call: &Call,
) -> Duration {
    let cur = current_device.load();
    match idle.next_idle() {
        IdleType::Timeout(ip, route) => {
            log::info!("route Timeout {:?},{:?}", ip, route);
            context.remove_route(&ip, route.route_key());
            if cur.is_gateway(&ip) {
                //网关路由过期，则需要改变状态
                crate::handle::change_status(current_device, ConnectStatus::Connecting);
                run_state_hook(
                    config,
                    "down",
                    Some(route.route_key()),
                    context
                        .local_port_by_key(&route.route_key())
                        .or_else(|| context.default_local_port()),
                    None,
                    Some(cur),
                    None,
                    "route_timeout",
                );
                call.error(ErrorInfo::new(ErrorType::Disconnect));
            }
            Duration::from_millis(100)
        }
        IdleType::Sleep(duration) => duration,
        IdleType::None => Duration::from_millis(3000),
    }
}

fn check_gateway_channel<Call: VntCallback>(
    context: &ChannelContext,
    current_device_info: &AtomicCell<CurrentDeviceInfo>,
    config: &BaseConfigInfo,
    connect_util: &ConnectUtil,
    udp_socket_sender: &AcceptSocketSender<Option<Vec<mio::net::UdpSocket>>>,
    call: &Call,
    count: &mut usize,
    handshake: &Handshake,
) -> io::Result<()> {
    let mut current_device = current_device_info.load();
    if current_device.status.offline() {
        *count += 1;
        let connect_protocol = context.main_protocol();
        let mut dns_ok = true;
        if connect_protocol.is_transport() {
            // 传输层的协议需要探测服务器地址
            let (next_device, resolved) =
                domain_request0(current_device_info, config, context.default_interface());
            current_device = next_device;
            dns_ok = resolved;
        }
        //需要重连
        call.connect(ConnectInfo::new(*count, current_device.connect_server));
        log::info!("发送握手请求,{:?}", config);
        let reconnect_rebind_interval = reconnect_rebind_interval();
        let force_new_route = dns_ok && *count % reconnect_rebind_interval == 0;
        if force_new_route {
            let request_packet = handshake.handshake_request_packet(config.server_secret)?;
            let old_local_port = context.default_local_port();
            context.clear_default_route_key();
            match connect_protocol {
                ConnectProtocol::UDP => {
                    match context.reset_reconnect_udp_socket(
                        udp_socket_sender,
                        current_device.connect_server,
                    ) {
                        Ok((index, port)) => {
                            context.set_default_local_port(Some(port));
                            let route_key = RouteKey::new(
                                ConnectProtocol::UDP,
                                index,
                                current_device.connect_server,
                            );
                            log::info!(
                                "重连失败达到{}次，阈值{}，使用新的UDP本地端口重连:{:?}",
                                *count,
                                reconnect_rebind_interval,
                                route_key
                            );
                            run_state_hook(
                                config,
                                "reconnect",
                                Some(route_key),
                                Some(port),
                                old_local_port,
                                Some(current_device),
                                Some(*count),
                                "rebind",
                            );
                            context.send_by_key(&request_packet, route_key)?;
                        }
                        Err(e) => {
                            log::warn!("创建UDP重连端口失败:{:?}", e);
                        }
                    }
                    return Ok(());
                }
                ConnectProtocol::TCP => {
                    log::info!(
                        "重连失败达到{}次，阈值{}，使用随机TCP源端口重连",
                        *count,
                        reconnect_rebind_interval
                    );
                    let hook = state_hook(
                        config,
                        "reconnect",
                        Some(ConnectProtocol::TCP),
                        None,
                        old_local_port,
                        Some(current_device.connect_server),
                        Some(current_device),
                        Some(*count),
                        "rebind",
                    );
                    connect_util.try_connect_tcp_punch_with_hook(
                        request_packet.into_buffer(),
                        current_device.connect_server,
                        hook,
                    );
                    return Ok(());
                }
                ConnectProtocol::WS | ConnectProtocol::WSS => {
                    log::info!(
                        "重连失败达到{}次，阈值{}，重新建立{}连接",
                        *count,
                        reconnect_rebind_interval,
                        config.server_addr
                    );
                    let protocol = if connect_protocol == ConnectProtocol::WSS {
                        "wss"
                    } else {
                        "ws"
                    };
                    let hook = state_hook(
                        config,
                        "reconnect",
                        None,
                        None,
                        old_local_port,
                        Some(current_device.connect_server),
                        Some(current_device),
                        Some(*count),
                        "rebind",
                    )
                    .map(|hook| hook.protocol(protocol));
                    connect_util.try_connect_ws_with_hook(
                        request_packet.into_buffer(),
                        config.server_addr.clone(),
                        hook,
                    );
                    return Ok(());
                }
            }
        }
        if let Err(e) = handshake.send(context, config.server_secret, current_device.connect_server)
        {
            log::warn!("{:?}", e);
            let request_packet = handshake.handshake_request_packet(config.server_secret)?;
            match connect_protocol {
                ConnectProtocol::UDP => {}
                ConnectProtocol::TCP => {
                    connect_util.try_connect_tcp(
                        request_packet.into_buffer(),
                        current_device.connect_server,
                    );
                }
                ConnectProtocol::WS | ConnectProtocol::WSS => {
                    connect_util
                        .try_connect_ws(request_packet.into_buffer(), config.server_addr.clone());
                }
            }
        }
    }
    Ok(())
}

fn route_protocol(route_key: RouteKey) -> &'static str {
    match route_key.protocol() {
        ConnectProtocol::UDP => "udp",
        ConnectProtocol::TCP => "tcp",
        ConnectProtocol::WS => "ws",
        ConnectProtocol::WSS => "wss",
    }
}

fn hook_tun_name(config: &BaseConfigInfo) -> Option<String> {
    let _ = config;
    #[cfg(all(
        feature = "integrated_tun",
        any(target_os = "windows", target_os = "linux", target_os = "macos")
    ))]
    {
        return Some(
            config
                .device_name
                .clone()
                .unwrap_or_else(|| "vnt-tun".into()),
        );
    }
    #[allow(unreachable_code)]
    None
}

fn state_hook(
    config: &BaseConfigInfo,
    event: &'static str,
    protocol: Option<ConnectProtocol>,
    local_port: Option<u16>,
    old_local_port: Option<u16>,
    remote_addr: Option<SocketAddr>,
    current_device: Option<CurrentDeviceInfo>,
    reconnect_count: Option<usize>,
    reason: &'static str,
) -> Option<HookInfo> {
    let virtual_ip = current_device.and_then(|info| {
        if info.virtual_ip.is_unspecified() {
            None
        } else {
            Some(info.virtual_ip)
        }
    });
    let hook = HookInfo::new(config.hook.as_deref(), event)?
        .local_port(local_port)
        .old_local_port(old_local_port)
        .remote_addr(remote_addr)
        .tun_name(hook_tun_name(config))
        .device_name(Some(config.name.clone()))
        .device_id(Some(config.device_id.clone()))
        .virtual_ip(virtual_ip)
        .server_addr(Some(config.server_addr.clone()))
        .reconnect_count(reconnect_count)
        .reason(reason);
    Some(match protocol {
        Some(ConnectProtocol::UDP) => hook.protocol("udp"),
        Some(ConnectProtocol::TCP) => hook.protocol("tcp"),
        Some(ConnectProtocol::WS) => hook.protocol("ws"),
        Some(ConnectProtocol::WSS) => hook.protocol("wss"),
        None => hook,
    })
}

fn run_state_hook(
    config: &BaseConfigInfo,
    event: &'static str,
    route_key: Option<RouteKey>,
    local_port: Option<u16>,
    old_local_port: Option<u16>,
    current_device: Option<CurrentDeviceInfo>,
    reconnect_count: Option<usize>,
    reason: &'static str,
) {
    let hook = if let Some(route_key) = route_key {
        state_hook(
            config,
            event,
            Some(route_key.protocol()),
            local_port,
            old_local_port,
            Some(route_key.addr),
            current_device,
            reconnect_count,
            reason,
        )
        .map(|hook| hook.protocol(route_protocol(route_key)))
    } else {
        state_hook(
            config,
            event,
            None,
            local_port,
            old_local_port,
            None,
            current_device,
            reconnect_count,
            reason,
        )
    };
    if let Some(hook) = hook {
        run_hook(hook);
    }
}

pub fn domain_request0(
    current_device: &AtomicCell<CurrentDeviceInfo>,
    config: &BaseConfigInfo,
    default_interface: &LocalInterface,
) -> (CurrentDeviceInfo, bool) {
    let mut current_dev = current_device.load();
    let mut dns_ok = false;

    // 探测服务端地址变化
    match dns_query_all(
        &config.server_addr,
        config.name_servers.clone(),
        default_interface,
    ) {
        Ok(addrs) => {
            dns_ok = true;
            log::info!(
                "domain {} dns {:?} addr {:?}",
                config.server_addr,
                config.name_servers,
                addrs
            );

            match address_choose(addrs) {
                Ok(addr) => {
                    if addr != current_dev.connect_server {
                        let mut tmp = current_dev.clone();
                        tmp.connect_server = addr;
                        let rs = current_device.compare_exchange(current_dev, tmp);
                        log::info!(
                            "服务端地址变化,旧地址:{}，新地址:{},替换结果:{}",
                            current_dev.connect_server,
                            addr,
                            rs.is_ok()
                        );
                        if rs.is_ok() {
                            current_dev.connect_server = addr;
                        }
                    }
                }
                Err(e) => {
                    log::error!("域名地址选择失败:{:?},domain={}", e, config.server_addr);
                }
            }
        }
        Err(e) => {
            log::error!("域名解析失败:{:?},domain={}", e, config.server_addr);
        }
    }
    (current_dev, dns_ok)
}
