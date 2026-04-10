# 内置IP代理问题说明

## 当前状态

内置IP代理功能在所有平台都**无法正常工作**。

## 问题原因

数据包写回TUN后，由于源IP是虚拟IP，系统的反向路径过滤（rp_filter）会丢弃这些包，导致代理服务器无法接收到连接。

## 解决方案

### 方案1：使用系统IP转发（推荐）✅

禁用内置代理，使用系统的IP转发功能：

```bash
# 启动时添加 --no-proxy 参数
./vnt-cli -k <token> --no-proxy

# Linux需要启用IP转发
sudo sysctl -w net.ipv4.ip_forward=1

# 配置iptables NAT
sudo iptables -t nat -A POSTROUTING -s 10.26.0.0/24 -j MASQUERADE
```

### 方案2：配置系统参数（Linux）

如果必须使用内置代理，需要配置系统参数：

```bash
# 禁用反向路径过滤
sudo sysctl -w net.ipv4.conf.all.rp_filter=0
sudo sysctl -w net.ipv4.conf.vnt-tun.rp_filter=0

# 允许本地路由接受非本地源IP
sudo sysctl -w net.ipv4.conf.all.accept_local=1
sudo sysctl -w net.ipv4.conf.vnt-tun.accept_local=1
```

**注意：** 这些配置会降低系统安全性，不推荐在生产环境使用。

### 方案3：实现用户态TCP/UDP协议栈（未来）

使用smoltcp等库实现完整的用户态协议栈，不依赖系统路由。这需要大量开发工作。

## 使用建议

1. **推荐使用 `--no-proxy` 参数**，让系统处理IP转发
2. 如果需要访问局域网设备，配置系统的IP转发和NAT规则
3. 内置代理功能目前仅作为实验性功能保留

## 参数说明

- 默认：启用内置代理（但无法正常工作）
- `--no-proxy`：禁用内置代理，使用系统IP转发 ✅ 推荐

## 示例

```bash
# 正确的使用方式
./vnt-cli -k <token> --no-proxy

# 然后配置系统IP转发（Linux）
sudo sysctl -w net.ipv4.ip_forward=1
sudo iptables -t nat -A POSTROUTING -s 10.26.0.0/24 -o eth0 -j MASQUERADE
```

## 相关Issue

内置IP代理的架构性问题需要重新设计才能解决。欢迎贡献代码！
