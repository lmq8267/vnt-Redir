//
//  VNT-Bridging-Header.h
//  VNT iOS/tvOS 桥接头文件
//
//  此文件定义了Rust FFI函数的C接口，供Swift代码调用
//

#ifndef VNT_Bridging_Header_h
#define VNT_Bridging_Header_h

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// 从文件描述符启动VNT隧道（简化版）
///
/// @param fd 从NEPacketTunnelProvider获取的文件描述符
/// @param server_addr VNT服务器地址（C字符串）
/// @param token 认证令牌（C字符串）
/// @return 0表示成功，负数表示错误码
int32_t vnt_start_tunnel(int32_t fd, const char* server_addr, const char* token);

/// 从文件描述符启动VNT隧道（完整配置）
///
/// @param fd 从NEPacketTunnelProvider获取的文件描述符
/// @param server_addr VNT服务器地址（C字符串）
/// @param token 认证令牌（C字符串）
/// @param config_json JSON格式的配置（可选，传NULL使用默认配置）
/// @return 0表示成功，负数表示错误码
///
/// JSON配置示例：
/// {
///   "name_servers": ["8.8.8.8:53"],
///   "stun_server": ["stun.l.google.com:19302"],
///   "password": "your_password",
///   "mtu": 1400,
///   "cipher_model": "aes_gcm",
///   "first_latency": true,
///   "use_channel_type": "all",
///   "enable_traffic": false
/// }
int32_t vnt_start_tunnel_with_config(
    int32_t fd,
    const char* server_addr,
    const char* token,
    const char* config_json
);

/// 停止VNT隧道
void vnt_stop_tunnel(void);

/// 获取VNT连接状态
///
/// @return 0=离线, 1=在线, -1=无实例
int32_t vnt_get_status(void);

/// 设置日志级别
///
/// @param level 日志级别 (0=Error, 1=Warn, 2=Info, 3=Debug, 4=Trace)
void vnt_set_log_level(int32_t level);

#ifdef __cplusplus
}
#endif

#endif /* VNT_Bridging_Header_h */
