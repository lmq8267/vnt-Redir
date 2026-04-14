# --local-dev 参数使用说明

## 参数作用

`--local-dev` 用于指定 VNT 组网使用的本地物理网卡。

- **留空（推荐）**：由操作系统自动选择路由，适用于大多数场景
- **指定网卡**：强制绑定到特定网卡，适用于多网卡环境或需要 IP 代理和出口节点功能

---

## 支持的输入格式

### Windows

| 格式 | 示例 | 说明 | 支持空格 |
|------|------|------|---------|
| **友好名称** | `以太网` | 控制面板显示的名称 | ✅ 支持 |
| **友好名称（带空格）** | `WLAN 5` | 多个同类网卡时的名称 | ✅ 支持 |
| **友好名称（英文）** | `Ethernet` | 英文系统的名称 | ✅ 支持 |
| **GUID** | `{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}` | 网卡唯一标识符 | ❌ 无空格 |
| **索引号** | `12` | 网卡接口索引 | ❌ 无空格 |

**查看方法：**
```cmd
# 查看友好名称和索引
netsh interface ipv4 show interfaces

# 输出示例：
# Idx     Met         MTU          State                Name
# ---  ----------  ----------  ------------  ---------------------------
#   1          75  4294967295  connected     Loopback Pseudo-Interface 1
#  12          25        1500  connected     以太网
#  15          50        1500  connected     WLAN 5
#  18          35        1500  disconnected  本地连接* 1
```

**使用示例：**
```bash
# 使用友好名称（推荐）
vnt-cli --token xxx --server xxx --local-dev "以太网"
vnt-cli --token xxx --server xxx --local-dev "WLAN 5"
vnt-cli --token xxx --server xxx --local-dev "Ethernet"

# 使用索引号
vnt-cli --token xxx --server xxx --local-dev 12

# 使用 GUID（不推荐，难以记忆）
vnt-cli --token xxx --server xxx --local-dev "{XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX}"
```

---

### Linux

| 格式 | 示例 | 说明 | 支持空格 |
|------|------|------|---------|
| **网卡名** | `eth0` | 有线网卡 | ❌ 无空格 |
| **网卡名** | `wlan0` | 无线网卡 | ❌ 无空格 |
| **网卡名** | `enp3s0` | 新命名规则的网卡 | ❌ 无空格 |

**查看方法：**
```bash
# 方法 1
ip link show

# 方法 2
ifconfig

# 输出示例：
# 1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536
# 2: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500
# 3: wlan0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500
```

**使用示例：**
```bash
vnt-cli --token xxx --server xxx --local-dev eth0
vnt-cli --token xxx --server xxx --local-dev wlan0
vnt-cli --token xxx --server xxx --local-dev enp3s0
```

---

### macOS

| 格式 | 示例 | 说明 | 支持空格 |
|------|------|------|---------|
| **网卡名** | `en0` | 主网卡（通常是有线） | ❌ 无空格 |
| **网卡名** | `en1` | 第二网卡（通常是无线） | ❌ 无空格 |

**查看方法：**
```bash
# 方法 1
ifconfig

# 方法 2
networksetup -listallhardwareports

# 输出示例：
# Hardware Port: Ethernet
# Device: en0
# Ethernet Address: xx:xx:xx:xx:xx:xx
#
# Hardware Port: Wi-Fi
# Device: en1
# Ethernet Address: xx:xx:xx:xx:xx:xx
```

**使用示例：**
```bash
vnt-cli --token xxx --server xxx --local-dev en0
vnt-cli --token xxx --server xxx --local-dev en1
```

---

### Android

| 格式 | 示例 | 说明 | 支持空格 |
|------|------|------|---------|
| **网卡名** | `wlan0` | Wi-Fi 网卡 | ❌ 无空格 |
| **网卡名** | `rmnet_data0` | 移动数据网卡 | ❌ 无空格 |

**查看方法：**
```bash
# 需要 root 权限或 ADB
adb shell ip link show

# 输出示例：
# 1: lo: <LOOPBACK,UP,LOWER_UP>
# 2: wlan0: <BROADCAST,MULTICAST,UP,LOWER_UP>
# 3: rmnet_data0: <BROADCAST,MULTICAST,UP,LOWER_UP>
```

**使用示例：**
```bash
# Flutter 界面输入
wlan0          # Wi-Fi
rmnet_data0    # 移动数据
```

---

## 空格处理

### ✅ 支持空格的场景

**Windows 友好名称：**
- `WLAN 5`
- `本地连接 2`
- `Ethernet 3`
- `Wi-Fi 2`

**命令行使用：**
```bash
# 需要用引号包裹
vnt-cli --token xxx --server xxx --local-dev "WLAN 5"
vnt-cli --token xxx --server xxx --local-dev "本地连接 2"
```

**Flutter 界面：**
- 直接输入即可，无需引号
- 示例：`WLAN 5`

### ❌ 不支持空格的场景

**Linux/macOS/Android 网卡名：**
- 系统网卡名不包含空格
- 示例：`eth0`、`wlan0`、`en0`

**Windows GUID 和索引：**
- GUID 格式固定，无空格
- 索引号是纯数字，无空格

---

## 识别优先级

当指定 `--local-dev` 参数时，按以下顺序尝试匹配：

1. **索引号匹配**（如果输入是纯数字）
   - 示例：`12` → 查找索引为 12 的网卡

2. **GUID/网卡名精确匹配**
   - Windows: `{XXXX-XXXX}` 或 GUID
   - Linux/macOS/Android: `eth0`、`wlan0`、`en0`

3. **Windows 友好名称匹配**（仅 Windows）
   - 通过 Windows API 查找
   - 支持带空格的名称：`WLAN 5`

**日志输出：**
```
# 索引匹配成功
✓ 按索引找到网卡: Index=12, GUID={XXXX-XXXX}, IP=192.168.1.100

# GUID 匹配成功
✓ 按 GUID/名称找到网卡: {XXXX-XXXX}, IP=192.168.1.100

# 友好名称匹配成功
✓ 通过友好名称 'WLAN 5' 找到网卡: GUID={XXXX-XXXX}, Index=15, IP=192.168.1.50

# 匹配失败
未找到指定的网卡 'WLAN 6'，请检查网卡名称、索引或友好名称是否正确
```

---

## 常见问题

### Q1: Windows 如何输入带空格的友好名称？

**命令行：**
```bash
# 使用双引号包裹
vnt-cli --local-dev "WLAN 5"

# 或使用单引号（PowerShell）
vnt-cli --local-dev 'WLAN 5'
```

**配置文件：**
```toml
[vnt]
local-dev = "WLAN 5"
```

**Flutter 界面：**
- 直接输入：`WLAN 5`
- 无需引号

---

### Q2: 如何确认网卡名称是否正确？

**Windows：**
```cmd
netsh interface ipv4 show interfaces
```
复制 "Name" 列的内容，包括空格。

**Linux/macOS：**
```bash
ip link show
# 或
ifconfig
```
复制网卡名称（冒号前的部分）。

---

### Q3: 为什么指定了网卡还是连接失败？

可能原因：
1. **网卡名称拼写错误**（包括空格）
2. **网卡已断开连接**
3. **网卡没有 IPv4 地址**
4. **权限不足**（Linux/macOS 可能需要 sudo）

**解决方法：**
- 检查日志输出，确认是否找到网卡
- 确认网卡已连接并有 IP 地址
- 尝试使用索引号代替名称

---

### Q4: 多个同名网卡怎么办？

Windows 会自动编号：
- `WLAN` → 第一个无线网卡
- `WLAN 2` → 第二个无线网卡
- `WLAN 5` → 第五个无线网卡

使用完整名称（包括编号）即可。

---

## 技术实现

### Windows API
使用 `GetAdaptersAddresses` API 获取网卡信息：
- 支持 Windows 7 - Windows 12
- 兼容精简系统
- 直接读取友好名称（UTF-16 编码）
- 支持空格和特殊字符

### Linux/macOS
使用 `network_interface` crate：
- 读取系统网卡列表
- 按名称精确匹配
- 通过 `SO_BINDTODEVICE` 绑定

### Android
使用 `network_interface` crate：
- 读取系统网卡列表
- 按名称精确匹配
- 注意：Android VPN 模式下绑定效果有限

---

## 推荐用法

### 一般用户（推荐留空）
```bash
# 命令行
vnt-cli --token xxx --server xxx

# Flutter 界面
本地物理网卡：[留空]
```

### 多网卡用户
```bash
# Windows - 指定有线网卡
vnt-cli --token xxx --server xxx --local-dev "以太网"

# Windows - 指定无线网卡
vnt-cli --token xxx --server xxx --local-dev "WLAN 5"

# Linux - 指定有线网卡
vnt-cli --token xxx --server xxx --local-dev eth0

# Linux - 指定无线网卡
vnt-cli --token xxx --server xxx --local-dev wlan0
```

### 需要 IP 代理功能
```bash
# 必须指定网卡才能使用 --in-ips 和 --out-ips
vnt-cli --token xxx --server xxx \
  --local-dev "以太网" \
  --in-ips 10.0.0.0/24 \
  --out-ips 192.168.1.0/24
```

---

