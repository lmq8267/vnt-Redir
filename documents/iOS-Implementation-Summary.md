# iOS/tvOS 支持实现说明

本文档说明为vnt-Redir添加的iOS/tvOS支持。

## 实现概述

vnt-Redir现在完全支持iOS和tvOS平台。由于iOS/tvOS的安全限制，应用无法直接创建TUN设备，必须通过系统的`NEPacketTunnelProvider`框架获取文件描述符。

## 修改的文件

### 1. 核心实现文件

#### `vnt/src/tun_tap_device/create_device.rs`
- **修改内容**：
  - 添加iOS/tvOS平台检测
  - 跳过iOS/tvOS的手动路由配置（由系统管理）
  - 在`create_device0()`中为iOS/tvOS返回错误提示，引导使用`SyncDevice::from_fd()`
  - 添加iOS/tvOS的`add_route()`空实现（路由由NEPacketTunnelProvider管理）

- **关键代码**：
```rust
// iOS/tvOS平台的路由由NEPacketTunnelProvider管理，无需手动配置
#[cfg(any(target_os = "ios", target_os = "tvos"))]
{
    log::info!("iOS/tvOS平台检测到，路由配置由系统VPN框架管理");
    return Ok(device);
}
```

#### `vnt/src/ios_ffi.rs` (新文件)
- **功能**：提供iOS/tvOS的FFI接口
- **导出函数**：
  - `vnt_start_tunnel()` - 从文件描述符启动VNT隧道
  - `vnt_stop_tunnel()` - 停止VNT隧道
  - `vnt_get_status()` - 获取连接状态
  - `vnt_set_log_level()` - 设置日志级别

- **特性**：
  - 使用`lazy_static`维护全局VNT实例
  - 提供默认的回调实现
  - 安全的C字符串处理
  - 完整的错误处理

#### `vnt/src/lib.rs`
- **修改内容**：添加iOS FFI模块导出
```rust
#[cfg(any(target_os = "ios", target_os = "tvos"))]
pub mod ios_ffi;
```

#### `vnt/Cargo.toml`
- **修改内容**：为iOS/tvOS添加`lazy_static`依赖
```toml
[target.'cfg(any(target_os = "ios", target_os = "tvos"))'.dependencies]
lazy_static = "1.4"
```

### 2. 文档和示例文件

#### `documents/iOS-Integration.md` (新文件)
完整的iOS/tvOS集成指南，包括：
- 架构说明
- Swift端实现（获取文件描述符）
- Rust端实现（FFI接口）
- 编译配置
- Xcode集成步骤
- 注意事项和故障排除

#### `documents/VNT-Bridging-Header.h` (新文件)
Swift桥接头文件，定义C接口：
```c
int32_t vnt_start_tunnel(int32_t fd, const char* server_addr, const char* token);
void vnt_stop_tunnel(void);
int32_t vnt_get_status(void);
void vnt_set_log_level(int32_t level);
```

#### `documents/PacketTunnelProvider.swift` (新文件)
完整的Swift实现示例，包括：
- `PacketTunnelProvider`类实现
- 文件描述符获取（iOS 16+兼容方法）
- 隧道生命周期管理
- 网络设置配置
- 主应用通信示例

#### `build-ios.sh` (新文件)
自动化编译脚本，支持：
- 交互式选择编译目标
- 自动安装Rust工具链
- 编译所有iOS/tvOS架构
- 创建通用二进制（fat binary）
- 生成使用说明

## 技术细节

### 平台差异处理

1. **设备创建**：
   - 其他平台：使用`DeviceBuilder`直接创建
   - iOS/tvOS：必须使用`SyncDevice::from_fd()`从外部文件描述符创建

2. **路由配置**：
   - 其他平台：通过系统命令手动添加路由
   - iOS/tvOS：通过`NEPacketTunnelNetworkSettings`配置，无需手动操作

3. **权限模型**：
   - 其他平台：需要root权限或特定capabilities
   - iOS/tvOS：通过Network Extension权限，由系统管理

### 文件描述符获取

iOS 16+推荐使用搜索方法而不是KVO：
```swift
// 搜索utun控制套接字
for fd: Int32 in 0...1024 {
    // 检查是否为utun套接字
    if addr.sc_id == ctlInfo.ctl_id {
        return fd
    }
}
```

这个方法改编自WireGuard的生产实现，更可靠。

### FFI安全性

- 使用`unsafe`块明确标记不安全操作
- 验证文件描述符有效性
- 安全的C字符串转换
- 完整的错误处理和日志记录

## 编译和使用

### 编译

```bash
# 使用自动化脚本
./build-ios.sh

# 或手动编译特定目标
cargo build --release --target aarch64-apple-ios --features integrated_tun
```

### 集成到Xcode项目

1. 添加静态库`libvnt.a`
2. 设置桥接头文件
3. 实现`PacketTunnelProvider`
4. 配置Network Extension权限

详细步骤参见`documents/iOS-Integration.md`。

## 兼容性

### 支持的平台
- ✅ iOS 12.0+
- ✅ tvOS 12.0+
- ✅ iOS模拟器（x86_64, ARM64）
- ✅ tvOS模拟器（x86_64, ARM64）

### 支持的架构
- ✅ ARM64 (设备)
- ✅ x86_64 (模拟器)
- ✅ ARM64 (Apple Silicon模拟器)

### 不影响的平台
- ✅ Linux - 无变化
- ✅ macOS - 无变化
- ✅ Windows - 无变化
- ✅ FreeBSD/OpenBSD/NetBSD - 无变化

## 测试建议

1. **设备测试**：
   - 在真实iOS设备上测试
   - 测试网络切换（WiFi ↔ 蜂窝）
   - 测试后台运行

2. **模拟器测试**：
   - 在iOS模拟器上测试基本功能
   - 注意：模拟器网络环境与真机不同

3. **兼容性测试**：
   - 测试不同iOS版本（12.0+）
   - 测试不同设备型号

## 已知限制

1. **iOS/tvOS限制**：
   - 无法直接创建TUN设备
   - 必须通过NEPacketTunnelProvider
   - 需要Network Extension权限

2. **系统限制**：
   - 后台运行可能被系统终止
   - 内存使用受限
   - 网络切换需要处理

3. **开发限制**：
   - 需要付费Apple Developer账号
   - 需要配置Provisioning Profile
   - 调试相对复杂

## 未来改进

1. **功能增强**：
   - [ ] 添加更多配置选项
   - [ ] 支持IPv6
   - [ ] 优化内存使用

2. **开发体验**：
   - [ ] 提供示例Xcode项目
   - [ ] 添加单元测试
   - [ ] 改进错误消息

3. **文档完善**：
   - [ ] 添加视频教程
   - [ ] 提供更多示例
   - [ ] 翻译为英文

## 参考资料

- [tun-rs iOS集成文档](../tun-rs/docs/iOS-Integration.md)
- [Apple NEPacketTunnelProvider文档](https://developer.apple.com/documentation/networkextension/nepackettunnelprovider)
- [WireGuard iOS实现](https://github.com/WireGuard/wireguard-apple)
- [Network Extension编程指南](https://developer.apple.com/library/archive/documentation/NetworkingInternetWeb/Conceptual/NetworkExtensionProgrammingGuide/)

## 许可证

与vnt-Redir主项目保持一致。
