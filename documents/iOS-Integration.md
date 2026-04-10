# iOS/tvOS 集成指南

本文档说明如何在iOS/tvOS应用中集成vnt-Redir。

## 概述

在iOS和tvOS上，应用无法直接创建TUN设备。必须：
1. 使用`NEPacketTunnelProvider`建立VPN隧道
2. 从packet flow获取文件描述符
3. 通过FFI将文件描述符传递给Rust代码
4. 使用`tun_rs::SyncDevice::from_fd()`管理隧道

## 架构说明

```
Swift (NEPacketTunnelProvider)
    ↓ 获取文件描述符
    ↓ FFI调用
Rust (vnt-Redir)
    ↓ 使用SyncDevice::from_fd()
tun-rs (底层TUN设备管理)
```

## Swift端实现

### 1. 获取文件描述符（iOS 16+推荐方法）

```swift
import Foundation
import NetworkExtension
import os.log

class PacketTunnelProvider: NEPacketTunnelProvider {
    
    /// 通过搜索可用的文件描述符来查找隧道文件描述符
    /// 此方法改编自WireGuard的实现，适用于iOS 16+
    private func getTunnelFileDescriptor() -> Int32? {
        var ctlInfo = ctl_info()
        withUnsafeMutablePointer(to: &ctlInfo.ctl_name) {
            $0.withMemoryRebound(to: CChar.self, capacity: MemoryLayout.size(ofValue: $0.pointee)) {
                _ = strcpy($0, "com.apple.net.utun_control")
            }
        }
        
        // 搜索文件描述符以找到utun套接字
        // 注意：范围0...1024确保我们能找到fd。实际上，utun套接字通常在低范围（<100）并很快找到
        // 这个方法来自WireGuard的生产实现，只在隧道启动时运行一次
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
                return fd
            }
        }
        
        return nil
    }
    
    override func startTunnel(options: [String : NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        os_log(.info, "正在启动VNT隧道...")
        
        // 1. 创建隧道网络设置
        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "10.0.0.1")
        settings.mtu = 1400
        
        // 2. 配置IPv4设置
        let ipv4Settings = NEIPv4Settings(addresses: ["10.0.0.2"], subnetMasks: ["255.255.255.0"])
        ipv4Settings.includedRoutes = [NEIPv4Route.default()]
        settings.ipv4Settings = ipv4Settings
        
        // 3. 应用设置
        setTunnelNetworkSettings(settings) { [weak self] error in
            guard let self = self else {
                completionHandler(NSError(domain: "TunnelError", code: 1, 
                    userInfo: [NSLocalizedDescriptionKey: "Self已释放"]))
                return
            }
            
            if let error = error {
                os_log(.error, "设置隧道网络失败: %{public}@", error.localizedDescription)
                completionHandler(error)
                return
            }
            
            // 4. 获取文件描述符
            guard let tunFd = self.getTunnelFileDescriptor() else {
                let error = NSError(domain: "TunnelError", code: 2, 
                    userInfo: [NSLocalizedDescriptionKey: "无法定位隧道文件描述符"])
                os_log(.error, "获取文件描述符失败")
                completionHandler(error)
                return
            }
            
            os_log(.default, "使用文件描述符 %{public}d 启动隧道", tunFd)
            
            // 5. 在后台线程启动Rust隧道实现
            DispatchQueue.global(qos: .userInitiated).async {
                // 调用Rust FFI函数
                vnt_start_tunnel(tunFd)
            }
            
            completionHandler(nil)
        }
    }
    
    override func stopTunnel(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        os_log(.default, "停止隧道，原因: %{public}@", String(describing: reason))
        
        // 调用Rust FFI函数停止隧道
        vnt_stop_tunnel()
        
        completionHandler()
    }
}
```

### 2. 旧版KVO方法（不推荐，iOS 16+可能返回nil）

```swift
// ⚠️ 警告：此方法在iOS 16+上已弃用，可能不工作
let tunFd = self.packetFlow.value(forKeyPath: "socket.fileDescriptor") as? Int32
guard let unwrappedFd = tunFd else {
    os_log(.error, "无法启动隧道：文件描述符为nil")
    return
}
```

## Rust端实现

### 1. FFI接口定义

在`vnt/src/lib.rs`或单独的FFI模块中添加：

```rust
use std::os::unix::io::RawFd;
use std::sync::Arc;
use tun_rs::SyncDevice;

/// iOS/tvOS平台：从文件描述符启动VNT隧道
/// 
/// # 参数
/// * `fd` - 从NEPacketTunnelProvider获取的文件描述符
/// 
/// # 安全性
/// - fd必须是有效的、打开的文件描述符
/// - fd必须指向TUN设备
/// - 调用此函数后，Rust代码拥有fd的所有权
#[no_mangle]
#[cfg(any(target_os = "ios", target_os = "tvos"))]
pub extern "C" fn vnt_start_tunnel(fd: RawFd) -> i32 {
    // 初始化日志
    env_logger::init();
    
    log::info!("iOS/tvOS: 从文件描述符 {} 启动VNT隧道", fd);
    
    // 从文件描述符创建设备
    let device = match unsafe { SyncDevice::from_fd(fd) } {
        Ok(dev) => Arc::new(dev),
        Err(e) => {
            log::error!("从文件描述符创建设备失败: {:?}", e);
            return -1;
        }
    };
    
    // 创建VNT配置
    let config = match create_vnt_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            log::error!("创建VNT配置失败: {:?}", e);
            return -2;
        }
    };
    
    // 启动VNT
    match start_vnt_with_device(config, device) {
        Ok(_) => {
            log::info!("VNT隧道启动成功");
            0
        }
        Err(e) => {
            log::error!("启动VNT失败: {:?}", e);
            -3
        }
    }
}

/// 停止VNT隧道
#[no_mangle]
#[cfg(any(target_os = "ios", target_os = "tvos"))]
pub extern "C" fn vnt_stop_tunnel() {
    log::info!("iOS/tvOS: 停止VNT隧道");
    // 实现停止逻辑
    // 例如：调用全局VNT实例的stop方法
}

/// 创建VNT配置（示例）
#[cfg(any(target_os = "ios", target_os = "tvos"))]
fn create_vnt_config() -> anyhow::Result<Config> {
    // 从UserDefaults或配置文件读取配置
    // 这里是示例配置
    Ok(Config {
        // ... 配置参数
        ..Default::default()
    })
}

/// 使用提供的设备启动VNT
#[cfg(any(target_os = "ios", target_os = "tvos"))]
fn start_vnt_with_device(config: Config, device: Arc<SyncDevice>) -> anyhow::Result<()> {
    // 使用设备启动VNT核心逻辑
    // 注意：需要修改VntInner::new_device以接受外部设备
    todo!("实现VNT启动逻辑")
}
```

### 2. 修改设备创建逻辑

在`vnt/src/tun_tap_device/create_device.rs`中已经添加了iOS支持：

```rust
// iOS/tvOS平台不支持直接创建设备
#[cfg(any(target_os = "ios", target_os = "tvos"))]
{
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "iOS/tvOS平台不支持直接创建TUN设备。\
         请使用SyncDevice::from_fd()从NEPacketTunnelProvider获取的文件描述符创建设备。"
    ));
}
```

### 3. 修改VNT核心以支持外部设备

需要在`vnt/src/core/conn.rs`中添加一个新方法：

```rust
impl VntInner {
    /// 使用外部提供的设备创建VNT实例（用于iOS/tvOS）
    #[cfg(any(target_os = "ios", target_os = "tvos"))]
    pub fn new_with_device<Call: VntCallback>(
        config: Config,
        callback: Call,
        device: Arc<SyncDevice>,
    ) -> anyhow::Result<Self> {
        // 使用提供的设备而不是创建新设备
        Self::new_device0(config, callback, device)
    }
}
```

## 编译配置

### Cargo.toml

确保启用iOS支持：

```toml
[lib]
crate-type = ["staticlib", "cdylib", "lib"]

[dependencies]
tun-rs = { version = "2", features = ["experimental"] }

[target.'cfg(any(target_os = "ios", target_os = "tvos"))'.dependencies]
# iOS特定依赖
```

### 编译命令

```bash
# iOS (ARM64)
cargo build --release --target aarch64-apple-ios

# iOS模拟器 (x86_64)
cargo build --release --target x86_64-apple-ios

# iOS模拟器 (ARM64)
cargo build --release --target aarch64-apple-ios-sim

# tvOS
cargo build --release --target aarch64-apple-tvos
```

## Xcode集成

### 1. 添加静态库

将编译生成的`.a`文件添加到Xcode项目：
- `libvnt.a` (从`target/aarch64-apple-ios/release/`)

### 2. 桥接头文件

创建`VNT-Bridging-Header.h`：

```c
#ifndef VNT_Bridging_Header_h
#define VNT_Bridging_Header_h

#include <stdint.h>

// 启动VNT隧道
int32_t vnt_start_tunnel(int32_t fd);

// 停止VNT隧道
void vnt_stop_tunnel(void);

#endif
```

### 3. 项目设置

在Xcode项目设置中：
- **Build Settings** → **Swift Compiler - General** → **Objective-C Bridging Header**: 设置为桥接头文件路径
- **Build Settings** → **Linking** → **Other Linker Flags**: 添加 `-lvnt`
- **Build Settings** → **Search Paths** → **Library Search Paths**: 添加静态库路径

## 注意事项

### 1. 路由配置

iOS/tvOS上的路由通过`NEPacketTunnelNetworkSettings`配置，不需要手动添加：

```swift
let ipv4Settings = NEIPv4Settings(addresses: ["10.0.0.2"], subnetMasks: ["255.255.255.0"])

// 包含的路由（通过VPN的流量）
ipv4Settings.includedRoutes = [
    NEIPv4Route.default(),  // 所有流量
    // 或指定特定路由
    // NEIPv4Route(destinationAddress: "192.168.1.0", subnetMask: "255.255.255.0")
]

// 排除的路由（不通过VPN的流量）
ipv4Settings.excludedRoutes = [
    NEIPv4Route(destinationAddress: "192.168.0.0", subnetMask: "255.255.0.0")
]
```

### 2. 权限要求

- 需要Network Extension权限
- 在Xcode中启用**Network Extensions** capability
- 配置App ID和Provisioning Profile

### 3. 后台运行

VPN扩展在后台运行，但需要注意：
- 系统可能在内存压力下终止扩展
- 实现适当的状态保存和恢复
- 处理网络切换（WiFi ↔ 蜂窝网络）

### 4. 调试

使用Console.app查看日志：
```
log stream --predicate 'subsystem == "com.yourcompany.yourapp.tunnel"' --level debug
```

## 示例项目结构

```
YourVPNApp/
├── YourVPNApp/              # 主应用
│   ├── AppDelegate.swift
│   └── ViewController.swift
├── TunnelExtension/         # VPN扩展
│   ├── PacketTunnelProvider.swift
│   ├── VNT-Bridging-Header.h
│   └── libvnt.a
└── Shared/
    └── Config.swift
```

## 参考资料

- [tun-rs iOS集成文档](../tun-rs/docs/iOS-Integration.md)
- [Apple NEPacketTunnelProvider文档](https://developer.apple.com/documentation/networkextension/nepackettunnelprovider)
- [WireGuard iOS实现](https://github.com/WireGuard/wireguard-apple)

## 故障排除

### 问题：文件描述符为nil

**解决方案**：使用推荐的`getTunnelFileDescriptor()`方法而不是KVO方法。

### 问题：隧道无法启动

**检查**：
1. 确认文件描述符有效
2. 检查网络设置是否正确应用
3. 查看Console.app中的日志
4. 确认权限配置正确

### 问题：编译错误

**检查**：
1. 确认目标架构正确
2. 检查依赖版本兼容性
3. 清理并重新编译：`cargo clean && cargo build`
