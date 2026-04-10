//
//  PacketTunnelProvider.swift
//  VNT iOS/tvOS 完整实现示例
//
//  此文件展示如何在iOS/tvOS上集成VNT
//

import NetworkExtension
import os.log

/// VNT隧道提供者
/// 继承自NEPacketTunnelProvider，实现VPN隧道功能
class PacketTunnelProvider: NEPacketTunnelProvider {
    
    // MARK: - 属性
    
    /// 日志子系统
    private let logger = OSLog(subsystem: "com.yourcompany.vnt", category: "Tunnel")
    
    /// 隧道文件描述符
    private var tunnelFd: Int32?
    
    /// VNT配置
    private struct VNTConfig {
        let serverAddress: String
        let token: String
        let virtualIP: String
        let virtualNetmask: String
        let mtu: Int
        
        static let `default` = VNTConfig(
            serverAddress: "your-server.com:29872",
            token: "your-token",
            virtualIP: "10.26.0.2",
            virtualNetmask: "255.255.255.0",
            mtu: 1400
        )
    }
    
    // MARK: - 文件描述符获取
    
    /// 获取隧道文件描述符（iOS 16+推荐方法）
    /// 此方法改编自WireGuard的实现
    private func getTunnelFileDescriptor() -> Int32? {
        var ctlInfo = ctl_info()
        withUnsafeMutablePointer(to: &ctlInfo.ctl_name) {
            $0.withMemoryRebound(to: CChar.self, capacity: MemoryLayout.size(ofValue: $0.pointee)) {
                _ = strcpy($0, "com.apple.net.utun_control")
            }
        }
        
        // 搜索文件描述符以找到utun套接字
        // 范围0...1024确保我们能找到fd
        // 实际上utun套接字通常在低范围（<100）并很快找到
        for fd: Int32 in 0...1024 {
            var addr = sockaddr_ctl()
            var ret: Int32 = -1
            var len = socklen_t(MemoryLayout.size(ofValue: addr))
            
            withUnsafeMutablePointer(to: &addr) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    ret = getpeername(fd, $0, &len)
                }
            }
            
            if ret != 0 || addr.sc_family != AF_SYSTEM {
                continue
            }
            
            if ctlInfo.ctl_id == 0 {
                ret = ioctl(fd, CTLIOCGINFO, &ctlInfo)
                if ret != 0 {
                    continue
                }
            }
            
            if addr.sc_id == ctlInfo.ctl_id {
                os_log(.debug, log: logger, "找到隧道文件描述符: %{public}d", fd)
                return fd
            }
        }
        
        os_log(.error, log: logger, "未找到隧道文件描述符")
        return nil
    }
    
    // MARK: - 隧道生命周期
    
    /// 启动隧道
    override func startTunnel(options: [String : NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        os_log(.info, log: logger, "正在启动VNT隧道...")
        
        // 1. 从选项或默认配置获取VNT配置
        let config = loadConfiguration(from: options)
        
        // 2. 创建隧道网络设置
        let tunnelNetworkSettings = createTunnelNetworkSettings(config: config)
        
        // 3. 应用网络设置
        setTunnelNetworkSettings(tunnelNetworkSettings) { [weak self] error in
            guard let self = self else {
                completionHandler(self?.createError(code: 1, message: "Self已释放"))
                return
            }
            
            if let error = error {
                os_log(.error, log: self.logger, "设置隧道网络失败: %{public}@", error.localizedDescription)
                completionHandler(error)
                return
            }
            
            os_log(.info, log: self.logger, "隧道网络设置已应用")
            
            // 4. 获取文件描述符
            guard let tunFd = self.getTunnelFileDescriptor() else {
                let error = self.createError(code: 2, message: "无法定位隧道文件描述符")
                completionHandler(error)
                return
            }
            
            self.tunnelFd = tunFd
            os_log(.default, log: self.logger, "使用文件描述符 %{public}d 启动隧道", tunFd)
            
            // 5. 设置日志级别
            vnt_set_log_level(2) // Info级别
            
            // 6. 在后台线程启动VNT
            DispatchQueue.global(qos: .userInitiated).async {
                let result = config.serverAddress.withCString { serverPtr in
                    config.token.withCString { tokenPtr in
                        vnt_start_tunnel(tunFd, serverPtr, tokenPtr)
                    }
                }
                
                DispatchQueue.main.async {
                    if result == 0 {
                        os_log(.info, log: self.logger, "VNT隧道启动成功")
                        completionHandler(nil)
                    } else {
                        let error = self.createError(code: Int(result), message: "VNT启动失败")
                        os_log(.error, log: self.logger, "VNT启动失败，错误码: %{public}d", result)
                        completionHandler(error)
                    }
                }
            }
        }
    }
    
    /// 停止隧道
    override func stopTunnel(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        os_log(.default, log: logger, "停止隧道，原因: %{public}@", String(describing: reason))
        
        // 调用Rust FFI停止VNT
        vnt_stop_tunnel()
        
        tunnelFd = nil
        
        completionHandler()
    }
    
    /// 处理应用消息
    override func handleAppMessage(_ messageData: Data, completionHandler: ((Data?) -> Void)?) {
        os_log(.debug, log: logger, "收到应用消息: %{public}d 字节", messageData.count)
        
        // 可以在这里处理来自主应用的消息
        // 例如：查询状态、更新配置等
        
        if let handler = completionHandler {
            // 返回状态信息
            let status = vnt_get_status()
            let response = ["status": status]
            if let responseData = try? JSONSerialization.data(withJSONObject: response) {
                handler(responseData)
            } else {
                handler(nil)
            }
        }
    }
    
    /// 处理网络变化
    override func handleNetworkChange(_ newPath: NWPath) {
        os_log(.info, log: logger, "网络状态变化: %{public}@", String(describing: newPath.status))
        
        // 可以在这里处理网络切换
        // 例如：WiFi ↔ 蜂窝网络
    }
    
    // MARK: - 配置管理
    
    /// 从选项加载配置
    private func loadConfiguration(from options: [String: NSObject]?) -> VNTConfig {
        guard let options = options else {
            os_log(.info, log: logger, "使用默认配置")
            return .default
        }
        
        let serverAddress = options["serverAddress"] as? String ?? VNTConfig.default.serverAddress
        let token = options["token"] as? String ?? VNTConfig.default.token
        let virtualIP = options["virtualIP"] as? String ?? VNTConfig.default.virtualIP
        let virtualNetmask = options["virtualNetmask"] as? String ?? VNTConfig.default.virtualNetmask
        let mtu = options["mtu"] as? Int ?? VNTConfig.default.mtu
        
        os_log(.info, log: logger, "配置: 服务器=%{public}@, IP=%{public}@", serverAddress, virtualIP)
        
        return VNTConfig(
            serverAddress: serverAddress,
            token: token,
            virtualIP: virtualIP,
            virtualNetmask: virtualNetmask,
            mtu: mtu
        )
    }
    
    /// 创建隧道网络设置
    private func createTunnelNetworkSettings(config: VNTConfig) -> NEPacketTunnelNetworkSettings {
        // 使用虚拟网关地址作为远程地址
        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "10.26.0.1")
        
        // 设置MTU
        settings.mtu = NSNumber(value: config.mtu)
        
        // 配置IPv4
        let ipv4Settings = NEIPv4Settings(
            addresses: [config.virtualIP],
            subnetMasks: [config.virtualNetmask]
        )
        
        // 配置路由
        // 选项1：所有流量通过VPN
        ipv4Settings.includedRoutes = [NEIPv4Route.default()]
        
        // 选项2：仅特定流量通过VPN（取消注释以使用）
        // ipv4Settings.includedRoutes = [
        //     NEIPv4Route(destinationAddress: "10.26.0.0", subnetMask: "255.255.255.0")
        // ]
        
        // 排除本地网络
        ipv4Settings.excludedRoutes = [
            NEIPv4Route(destinationAddress: "192.168.0.0", subnetMask: "255.255.0.0"),
            NEIPv4Route(destinationAddress: "172.16.0.0", subnetMask: "255.240.0.0"),
            NEIPv4Route(destinationAddress: "10.0.0.0", subnetMask: "255.0.0.0")
        ]
        
        settings.ipv4Settings = ipv4Settings
        
        // ⚠️ 重要：配置IPv6以保留本机IPv6网络
        // 如果不配置IPv6，iOS会禁用IPv6网络
        let ipv6Settings = NEIPv6Settings(
            addresses: ["fd00::1"],  // 虚拟IPv6地址
            networkPrefixLengths: [64]
        )
        
        // 不路由任何IPv6流量到VPN，保留本机IPv6网络
        // 通过设置空的includedRoutes来实现
        ipv6Settings.includedRoutes = []
        
        settings.ipv6Settings = ipv6Settings
        
        // 配置DNS（可选）
        // let dnsSettings = NEDNSSettings(servers: ["8.8.8.8", "8.8.4.4"])
        // settings.dnsSettings = dnsSettings
        
        return settings
    }
    
    // MARK: - 工具方法
    
    /// 创建错误对象
    private func createError(code: Int, message: String) -> NSError {
        return NSError(
            domain: "com.yourcompany.vnt.tunnel",
            code: code,
            userInfo: [NSLocalizedDescriptionKey: message]
        )
    }
}

// MARK: - 主应用通信示例

/// 主应用中与VPN扩展通信的示例代码
class VPNManager {
    
    private let manager = NETunnelProviderManager()
    
    /// 启动VPN
    func startVPN(serverAddress: String, token: String) {
        // 加载配置
        NETunnelProviderManager.loadAllFromPreferences { [weak self] managers, error in
            guard let self = self else { return }
            
            if let error = error {
                print("加载VPN配置失败: \(error)")
                return
            }
            
            let manager = managers?.first ?? NETunnelProviderManager()
            
            // 配置VPN
            let protocolConfig = NETunnelProviderProtocol()
            protocolConfig.providerBundleIdentifier = "com.yourcompany.vnt.tunnel"
            protocolConfig.serverAddress = serverAddress
            
            // 传递配置选项
            protocolConfig.providerConfiguration = [
                "serverAddress": serverAddress,
                "token": token,
                "virtualIP": "10.26.0.2",
                "virtualNetmask": "255.255.255.0",
                "mtu": 1400
            ]
            
            manager.protocolConfiguration = protocolConfig
            manager.localizedDescription = "VNT"
            manager.isEnabled = true
            
            // 保存配置
            manager.saveToPreferences { error in
                if let error = error {
                    print("保存VPN配置失败: \(error)")
                    return
                }
                
                // 启动VPN
                do {
                    try manager.connection.startVPNTunnel()
                    print("VPN启动成功")
                } catch {
                    print("启动VPN失败: \(error)")
                }
            }
        }
    }
    
    /// 停止VPN
    func stopVPN() {
        manager.connection.stopVPNTunnel()
    }
    
    /// 查询VPN状态
    func queryStatus(completion: @escaping (Int32) -> Void) {
        guard let session = manager.connection as? NETunnelProviderSession else {
            completion(-1)
            return
        }
        
        // 发送消息到VPN扩展
        let message = Data()
        do {
            try session.sendProviderMessage(message) { response in
                if let response = response,
                   let json = try? JSONSerialization.jsonObject(with: response) as? [String: Any],
                   let status = json["status"] as? Int32 {
                    completion(status)
                } else {
                    completion(-1)
                }
            }
        } catch {
            print("发送消息失败: \(error)")
            completion(-1)
        }
    }
}
