#!/bin/sh
# vnt hook 示例脚本，适用于 OpenWrt/BusyBox sh。
# 使用方式示例:
#   --hook "sh /path/to/vnt-hook-example.sh"
#
# vnt 会通过环境变量传递状态，脚本自身不要长期阻塞。
# 本脚本仅做示例，实际使用前请按你的防火墙系统调整 iptables/nft/uci 规则。

# 支持的变量:
# VNT_HOOK_EVENT            事件: up/down/reconnect/stop
# VNT_HOOK_STATUS           与 VNT_HOOK_EVENT 相同，兼容状态脚本： up/down/reconnect/stop
# VNT_HOOK_PROTOCOL         当前通道协议: udp/tcp/ws/wss，无法获取时为空
# VNT_HOOK_LOCAL_PORT       当前新的本地随机端口，无法获取时为空；tcp/udp 重连成功时通常会有值
# VNT_HOOK_OLD_LOCAL_PORT   上一次本地随机端口，无法获取时为空
# VNT_HOOK_REMOTE_ADDR      当前远端地址，格式 ip:port，DNS无法解析或获取时为空
# VNT_HOOK_TUN_NAME         虚拟网卡名，无法获取时为空
# VNT_HOOK_DEVICE_NAME      vnt 设备名，无法获取时为空
# VNT_HOOK_DEVICE_ID        vnt 设备 ID，无法获取时为空
# VNT_HOOK_VIRTUAL_IP       服务端分配的虚拟 IP，无法获取时为空
# VNT_HOOK_SERVER_ADDR      配置中的服务端地址，无法获取时为空，不包含协议，例如: vnts.example.com:29872
# VNT_HOOK_RECONNECT_COUNT  当前重连次数，无法获取时为空
# VNT_HOOK_REASON           触发原因: registered/route_timeout/server_disconnect/rebind/stop
# VNT_HOOK_PID              vnt 进程 ID
# VNT_HOOK_TIMESTAMP        触发时间戳，秒级 如 1780720094

EVENT="${VNT_HOOK_EVENT:-}"
PROTOCOL="${VNT_HOOK_PROTOCOL:-}"
LOCAL_PORT="${VNT_HOOK_LOCAL_PORT:-}"
ENV_OLD_LOCAL_PORT="${VNT_HOOK_OLD_LOCAL_PORT:-}"
TUN_NAME="${VNT_HOOK_TUN_NAME:-}"
DEVICE_ID="${VNT_HOOK_DEVICE_ID:-default}"
STATE_FILE="/tmp/vnt-hook-${DEVICE_ID}.state"
TCP_RULE_COMMENT="vnt-hook-${DEVICE_ID}-tcp"
TUN_RULE_COMMENT="vnt-hook-${DEVICE_ID}-tun"

log() {
    logger -t vnt-hook "$*"
}

has_iptables() {
    command -v iptables >/dev/null 2>&1
}

add_tcp_rule() {
    port="$1"
    [ -n "$port" ] || return 0
    has_iptables || {
        log "未找到 iptables，跳过 tcp 端口放行: $port"
        return 0
    }

    iptables -C INPUT -p tcp --dport "$port" -m comment --comment "$TCP_RULE_COMMENT" -j ACCEPT >/dev/null 2>&1 \
        || iptables -I INPUT -p tcp --dport "$port" -m comment --comment "$TCP_RULE_COMMENT" -j ACCEPT >/dev/null 2>&1 \
        || log "放行 tcp 端口失败: $port"
}

del_tcp_rule() {
    port="$1"
    [ -n "$port" ] || return 0
    has_iptables || return 0

    while iptables -C INPUT -p tcp --dport "$port" -m comment --comment "$TCP_RULE_COMMENT" -j ACCEPT >/dev/null 2>&1; do
        iptables -D INPUT -p tcp --dport "$port" -m comment --comment "$TCP_RULE_COMMENT" -j ACCEPT >/dev/null 2>&1 || break
    done
}

add_tun_rules() {
    iface="$1"
    [ -n "$iface" ] || return 0
    has_iptables || {
        log "未找到 iptables，跳过虚拟网卡放行: $iface"
        return 0
    }

    iptables -C INPUT -i "$iface" -m comment --comment "$TUN_RULE_COMMENT-in" -j ACCEPT >/dev/null 2>&1 \
        || iptables -I INPUT -i "$iface" -m comment --comment "$TUN_RULE_COMMENT-in" -j ACCEPT >/dev/null 2>&1 \
        || log "放行虚拟网卡入站失败: $iface"

    iptables -C FORWARD -i "$iface" -m comment --comment "$TUN_RULE_COMMENT-forward-in" -j ACCEPT >/dev/null 2>&1 \
        || iptables -I FORWARD -i "$iface" -m comment --comment "$TUN_RULE_COMMENT-forward-in" -j ACCEPT >/dev/null 2>&1 \
        || log "放行虚拟网卡转发入站失败: $iface"

    iptables -C FORWARD -o "$iface" -m comment --comment "$TUN_RULE_COMMENT-forward-out" -j ACCEPT >/dev/null 2>&1 \
        || iptables -I FORWARD -o "$iface" -m comment --comment "$TUN_RULE_COMMENT-forward-out" -j ACCEPT >/dev/null 2>&1 \
        || log "放行虚拟网卡转发出站失败: $iface"
}

del_tun_rules() {
    iface="$1"
    [ -n "$iface" ] || return 0
    has_iptables || return 0

    while iptables -C INPUT -i "$iface" -m comment --comment "$TUN_RULE_COMMENT-in" -j ACCEPT >/dev/null 2>&1; do
        iptables -D INPUT -i "$iface" -m comment --comment "$TUN_RULE_COMMENT-in" -j ACCEPT >/dev/null 2>&1 || break
    done
    while iptables -C FORWARD -i "$iface" -m comment --comment "$TUN_RULE_COMMENT-forward-in" -j ACCEPT >/dev/null 2>&1; do
        iptables -D FORWARD -i "$iface" -m comment --comment "$TUN_RULE_COMMENT-forward-in" -j ACCEPT >/dev/null 2>&1 || break
    done
    while iptables -C FORWARD -o "$iface" -m comment --comment "$TUN_RULE_COMMENT-forward-out" -j ACCEPT >/dev/null 2>&1; do
        iptables -D FORWARD -o "$iface" -m comment --comment "$TUN_RULE_COMMENT-forward-out" -j ACCEPT >/dev/null 2>&1 || break
    done
}

read_state() {
    OLD_TCP_PORT=""
    OLD_TUN_NAME=""
    [ -f "$STATE_FILE" ] || return 0
    # shellcheck disable=SC1090
    . "$STATE_FILE" 2>/dev/null || true
}

write_state() {
    {
        echo "OLD_TCP_PORT='${LOCAL_PORT}'"
        echo "OLD_TUN_NAME='${TUN_NAME}'"
    } >"$STATE_FILE" 2>/dev/null || log "写入状态文件失败: $STATE_FILE"
}

read_state
[ -n "$ENV_OLD_LOCAL_PORT" ] && OLD_TCP_PORT="$ENV_OLD_LOCAL_PORT"

case "$EVENT" in
    up)
        # 上线时放行当前虚拟网卡；如果当前协议是 tcp 且能拿到本地端口，也放行 tcp 端口。
        add_tun_rules "$TUN_NAME"
        [ "$PROTOCOL" = "tcp" ] && add_tcp_rule "$LOCAL_PORT"
        write_state
        ;;
    reconnect)
        # 重连时 tcp 随机端口可能变化，先撤销上一次的端口，再放行新的端口。
        # udp/ws/wss 没有 tcp 防火墙端口需求时会自动跳过。
        if [ "$PROTOCOL" = "tcp" ]; then
            [ "$OLD_TCP_PORT" != "$LOCAL_PORT" ] && del_tcp_rule "$OLD_TCP_PORT"
            add_tcp_rule "$LOCAL_PORT"
        fi
        # 虚拟网卡名如果发生变化，也撤销旧接口规则并放行新接口规则。
        if [ "$OLD_TUN_NAME" != "$TUN_NAME" ]; then
            del_tun_rules "$OLD_TUN_NAME"
            add_tun_rules "$TUN_NAME"
        fi
        write_state
        ;;
    down)
        # 断线时通常不立即删除规则，避免短时间重连时频繁改防火墙。
        # 如需断线即删除，可取消下面两行注释。
        # del_tcp_rule "$OLD_TCP_PORT"
        # del_tun_rules "$OLD_TUN_NAME"
        ;;
    stop)
        # 停止时清理随机 tcp 端口和虚拟网卡防火墙规则。
        del_tcp_rule "$OLD_TCP_PORT"
        del_tun_rules "$OLD_TUN_NAME"
        rm -f "$STATE_FILE" >/dev/null 2>&1
        ;;
    *)
        log "未知事件: $EVENT protocol=$PROTOCOL port=$LOCAL_PORT tun=$TUN_NAME"
        ;;
esac

exit 0
