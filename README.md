<p align="center">
  <img src="https://socialify.git.ci/lmq8267/vnt-Redir/image?custom_description=vnt%E7%9A%84%E9%87%8D%E5%AE%9A%E5%90%91%E7%89%88%E6%9C%AC%EF%BC%8C%E4%BF%AE%E5%A4%8D%E4%B8%80%E4%BA%9B%E9%97%AE%E9%A2%98%E3%80%82&description=1&forks=1&issues=1&logo=https%3A%2F%2Fraw.githubusercontent.com%2Flmq8267%2FVntApp%2Fmaster%2Fandroid%2Fapp%2Fsrc%2Fmain%2Fres%2Fmipmap-xxxhdpi%2Fic_launcher.png&name=1&owner=1&pulls=1&stargazers=1&theme=Auto" alt="" width="640" height="320" />
  <img alt="" src="https://img.shields.io/github/created-at/lmq8267/vnt-Redir?logo=github&label=%E5%88%9B%E5%BB%BA%E6%97%A5%E6%9C%9F">
<a href="https://github.com/lmq8267/vnt-Redir/releases"><img src="https://img.shields.io/github/downloads/lmq8267/vnt-Redir/total?logo=github&label=%E4%B8%8B%E8%BD%BD%E9%87%8F"/></a>
<a href="https://github.com/lmq8267/vnt-Redir/releases/"><img src="https://img.shields.io/github/v/release/lmq8267/vnt-Redir?logo=github&label=%E7%A8%B3%E5%AE%9A%E7%89%88"/></a>
  <a href="https://github.com/lmq8267/vnt-Redir/releases/"><img src="https://img.shields.io/github/v/tag/lmq8267/vnt-Redir?logo=github&label=%E6%9C%80%E6%96%B0%E7%89%88%E6%9C%AC"/></a>
<a href="https://github.com/lmq8267/vnt-Redir/issues"><img src="https://img.shields.io/github/issues-raw/lmq8267/vnt-Redir?logo=github&label=%E9%97%AE%E9%A2%98"/></a>
<a href="https://github.com/lmq8267/vnt-Redir/actions?query=workflow%3ABuild"><img src="https://img.shields.io/github/actions/workflow/status/lmq8267/vnt-Redir/rust.yml?branch=main&logo=github&label=%E6%9E%84%E5%BB%BA%E7%8A%B6%E6%80%81" alt=""/></a>
<a href="https://hub.docker.com/r/lmq8267/vnt"><img src="https://img.shields.io/docker/v/lmq8267/vnt?label=%E9%95%9C%E5%83%8F%E6%9C%80%E6%96%B0%E7%89%88%E6%9C%AC&link=https%3A%2F%2Fhub.docker.com%2Fr%2Flmq8267%2Fvnt&logo=docker"/></a>
<a href="https://hub.docker.com/r/lmq8267/vnt"><img src="https://img.shields.io/docker/pulls/lmq8267/vnt?color=%2348BB78&logo=docker&label=%E6%8B%89%E5%8F%96%E9%87%8F" alt="Downloads"/></a>
</p>

# VNT

[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/lmq8267/vnt-Redir)

🚀An efficient VPN

🚀一个简单、高效、能快速组建虚拟局域网的工具

服务端 https://github.com/lmq8267/vnt_s

下载 点右边的releases

docker运行的话
```shell
#以下只演示加了一个参数 -k  其他参数直接在后面添加即可
docker run --name vnt-cli --net=host --privileged -e TZ=Asia/Shanghai -e LANG=zh_CN --restart=always -d lmq8267/vnt -k test123
```

```
#compose.yaml
version: '3.9'
services:
    vnt:
        image: lmq8267/vnt
        container_name: vnt-cli
        restart: always
        network_mode: host
        environment:
            - LANG=zh_CN
            - TZ=Asia/Shanghai
        privileged: true
        command: '-k test123'

```

像群晖等需要先安装或者加载好tun模块才能使用，vnt客户端依赖tun ，[解决群晖 NAS 无法使用 TUN / TAP 的问题 ](https://www.moewah.com/archives/2750.html)
警告：群晖等nas重要设备开启ssh或者执行下述命令加载tun或者参考网上教程添加脚本等可能会有无法预估的风险，除非你清楚这些命令或脚本是什么意思，否则自行承担数据损坏丢失的风险。
```shell
#检查是否安装了 tun 模块：
lsmod | grep tun
#或者
ls /dev/net/tun

#如果上述结果为空，请尝试加载它：
sudo modprobe tun
#或者
sudo insmod /lib/modules/tun.ko

#上述方法只测试在我的黑裙DSM7.2里是可以成功运行并且访问组网设备的
#加载后还是没有可能需要你自行百度一下如何安装tun模块了，每个系统不一样
```

### vnt-cli参数详解 [参数说明](https://github.com/vnt-dev/vnt/blob/main/vnt-cli/README.md)

**控制台输出日志**

- 创建一个`log4rs.yaml`文件，和`vnt-cli`二进制程序放在一起运行即可在控制台输出日志内容方便调试，内容如下：

```yaml
refresh_rate: 30 seconds

appenders:
  console:
    kind: console
    encoder:
      pattern: "{d(%Y-%m-%d %H:%M:%S.%3f)} [{f}:{L}] {h({l})} {M}:{m}{n}{n}"

root:
  level: info
  appenders:
    - console

loggers:
  # 可针对你的 crate 单独调试
  vnt_cli:
    level: debug
    appenders:
      - console
    additive: false

  vnt:
    level: info

  common:
    level: info
```

### 快速开始：

1. 指定一个token，在多台设备上运行该程序，例如：
    ```shell
      # linux上
      root@DESKTOP-0BCHNIO:/opt# ./vnt-cli -k 123456
      # 在另一台linux上使用nohup后台运行
      root@izj6cemne76ykdzkataftfz vnt# nohup ./vnt-cli -k 123456 &
      # windows上
      D:\vnt\bin_v1>vnt-cli.exe -k 123456
    ```
2. 可以执行info命令查看当前设备的虚拟ip
   ```shell
    root@DESKTOP-0BCHNIO:/opt# ./vnt-cli --info
    Name: Ubuntu 18.04 (bionic) [64-bit]
    Virtual ip: 10.26.0.2
    Virtual gateway: 10.26.0.1
    Virtual netmask: 255.255.255.0
    Connection status: Connected
    NAT type: Cone
    Relay server: 43.139.56.10:29871
    Public ips: 120.228.76.75
    Local ip: 172.25.165.58
    ```
3. 也可以执行list命令查看其他设备的虚拟ip
   ```shell
    root@DESKTOP-0BCHNIO:/opt# ./vnt-cli --list
    Name                                                       Virtual Ip      P2P/Relay      Rt      Status
    Windows 10.0.22621 (Windows 11 Professional) [64-bit]      10.26.0.3       p2p            2       Online
    CentOS 7.9.2009 (Core) [64-bit]                            10.26.0.4       p2p            35      Online
    ```
4. 最后可以用虚拟ip实现设备间相互访问

      <img width="506" alt="ssh" src="https://raw.githubusercontent.com/vnt-dev/vnt/main/documents/img/ssh.jpg">
5. 帮助，使用-h命令查看

### 使用须知

- token的作用是标识一个虚拟局域网，当使用公共服务器时，建议使用一个唯一值当token(比如uuid)，否则有可能连接到其他人创建的虚拟局域网中
- 默认使用公共服务器做注册和中继，目前的配置是2核4G 4Mbps，有需要再扩展~
- vnt-cli需要使用命令行运行
- Mac和Linux下需要加可执行权限(例如:chmod +x ./vnt-cli)
- 可以自己搭中继服务器([server](https://github.com/vnt-dev/vnts))

### 直接使用

[**下载release文件**](https://github.com/vnt-dev/vnt/releases)

[**帮助文档**](https://rustvnt.com)

### 自行编译

<details> <summary>点击展开</summary>

前提条件:安装rust编译环境([install rust](https://www.rust-lang.org/zh-CN/tools/install))

```
到项目根目录下执行 cargo build -p vnt-cli

也可按需编译，将得到更小的二进制文件，使用--no-default-features排除默认features

cargo build -p vnt-cli --no-default-features
```

features说明

| feature           | 说明                             | 是否默认 |
|-------------------|--------------------------------|------|
| openssl           | 使用openssl中的加密算法                | 否    |
| openssl-vendored  | 从源码编译openssl                   | 否    |
| ring-cipher       | 使用ring中的加密算法                   | 否    |
| aes_cbc           | 支持aes_cbc加密                    | 是    |
| aes_ecb           | 支持aes_ecb加密                    | 是    |
| aes_gcm           | 支持aes_gcm加密                    | 是    |
| sm4_cbc           | 支持sm4_cbc加密                    | 是    |
| chacha20_poly1305 | 支持chacha20和chacha20_poly1305加密 | 是    |
| server_encrypt    | 支持服务端加密                        | 是    |
| ip_proxy          | 内置ip代理                         | 是    |
| port_mapping      | 端口映射                           | 是    |
| log               | 日志                             | 是    |
| command           | list、route等命令                  | 是    |
| file_config       | yaml配置文件                       | 是    |
| lz4               | lz4压缩                          | 是    |
| zstd              | zstd压缩                         | 否    |
| upnp              | upnp协议                         | 否    |
| ws                | ws协议                           | 是    |
| wss               | wss协议                          | 是    |

</details>

### 支持平台

- Mac
- Linux
- Windows
    - 默认使用tun网卡 依赖wintun.dll([win-tun](https://www.wintun.net/))(将dll放到同目录下，建议使用版本0.14.1)
    - 可选择使用tap网卡 依赖tap-windows([win-tap](https://build.openvpn.net/downloads/releases/))(建议使用版本9.24.7)
- Android

### GUI

支持安卓和Windows [下载](https://github.com/vnt-dev/VntApp/releases/)

### 特性

- IP层数据转发
- NAT穿透
    - 点对点穿透
    - 服务端中继转发
    - 客户端中继转发
- IP代理(点对点、点对网)
- p2p组播/广播
- 客户端数据加密(`aes-gcm`、`chacha20-poly1305`等多种加密算法)
- 服务端数据加密(`rsa` + `aes-gcm`)
- 多通道UDP应对QOS
- 支持TCP、UDP、WebSocket等多种协议
- 支持数据压缩

### 更多玩法

1. 和远程桌面(如mstsc)搭配，超低延迟的体验
2. 安装samba服务，共享磁盘
3. 点对网,访问内网其他机器、IP代理(结合启动参数'-i'和'-o')

### Todo

- ~~桌面UI(已支持)~~
- 使用FEC、ARQ等方式提升弱网环境的稳定性

### 常见问题

<details> <summary>展开</summary>

#### 问题1: 设置网络地址失败

##### 可能原因:

vnt默认使用10.26.0.0/24网段，和本地网络适配器的ip冲突

##### 解决方法:

1. 方法一：找到冲突的IP，将其改成别的
2. 方法二：自建服务器，指定其他不会冲突的网段
3. 方法三：增加参数-d <device-id> ，设置不同的id会让服务端分配不同的IP，从而绕开有冲突的IP

#### 问题2: windows系统上wintun.dll加载失败

##### 可能原因：

没有下载wintun.dll 或者使用的wintun.dll有问题

##### 解决方法：

1. 下载最新版的wintun.dll [下载链接](https://www.wintun.net/builds/wintun-0.14.1.zip)
2. 解压后找到对应架构的目录,通常是amd64
3. 将对应的wintun.dll放到和vnt-cli同目录下（或者放到C盘Windows目录下）
4. 再次启动vnt-cli

#### 问题3: 丢包严重，或是不能正常组网通信

##### 可能原因：

某些宽带下(比如广电宽带)UDP丢包严重

##### 解决方法：

1. 使用TCP模式中继转发（vnt-cli增加--tcp参数）
2. 如果p2p后效果很差，可以选择禁用p2p（vnt-cli增加--use-channel relay 参数）

#### 问题4：重启后虚拟IP发生变化，或指定了IP不能启动

##### 可能原因：

设备重启后程序自动获取的id值改变，导致注册时重新分配了新的IP，或是IP冲突

##### 解决方法：

1. 命令行启动增加-d参数（使用配置文件启动则在配置文件中增加device_id参数），要保证每个设备的值都不一样，取值可以任意64位以内字符串

</details>

### 交流群

对VNT有任何问题均可以加群联系作者

QQ群1: 1034868233(满人)

QQ群2: 950473757

### 赞助

如果VNT对你有帮助，欢迎打赏作者

 <img width="300" alt="" src="https://github.com/vnt-dev/vnt/assets/49143209/0d3a7311-43fc-4ed7-9507-863b5d69b6b2">

### 其他

可使用社区小伙伴搭建的中继服务器

1. -s vnt.8443.eu.org:29871
2. -s vnt.wherewego.top:29872

### 参与贡献

<a href="https://github.com/vnt-dev/vnt/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=vnt-dev/vnt" />
</a>
