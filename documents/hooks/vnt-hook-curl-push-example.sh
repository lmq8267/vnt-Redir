#!/bin/sh
# vnt hook curl 推送示例，适用于 Linux/OpenWrt/macOS。
# 使用方式:
#   --hook "sh /path/to/vnt-hook-curl-push-example.sh"
#
# 这个脚本示例用于:
# 1. 与服务端断连时推送
# 2. 重连失败达到换端口阈值、开始使用新端口重连时推送
# 3. 重连成功重新上线时推送
#
# 注意:
# - vnt 会异步启动 hook，不等待 curl 执行完成。
# - curl 自身建议设置超时，避免脚本长时间挂住。
# - PUSH_URL 改成你的推送接口地址。

PUSH_URL="https://example.com/push"

EVENT="${VNT_HOOK_EVENT:-}"
REASON="${VNT_HOOK_REASON:-}"
DEVICE_NAME="${VNT_HOOK_DEVICE_NAME:-}"
DEVICE_ID="${VNT_HOOK_DEVICE_ID:-}"
PROTOCOL="${VNT_HOOK_PROTOCOL:-}"
LOCAL_PORT="${VNT_HOOK_LOCAL_PORT:-}"
OLD_LOCAL_PORT="${VNT_HOOK_OLD_LOCAL_PORT:-}"
REMOTE_ADDR="${VNT_HOOK_REMOTE_ADDR:-}"
VIRTUAL_IP="${VNT_HOOK_VIRTUAL_IP:-}"
SERVER_ADDR="${VNT_HOOK_SERVER_ADDR:-}"
RECONNECT_COUNT="${VNT_HOOK_RECONNECT_COUNT:-}"
TIMESTAMP="${VNT_HOOK_TIMESTAMP:-}"

case "$EVENT:$REASON" in
    down:route_timeout)
        TITLE="vnt与服务端连接超时"
        ;;
    down:server_disconnect)
        TITLE="vnt被服务端断开"
        ;;
    reconnect:rebind)
        TITLE="vnt重连失败，正在使用新端口重连"
        ;;
    up:registered)
        TITLE="vnt重连成功并已上线"
        ;;
    stop:stop)
        TITLE="vnt客户端已停止"
        ;;
    *)
        # 不关心的事件直接退出，避免推送太多。
        exit 0
        ;;
esac

CONTENT="事件: ${EVENT}
原因: ${REASON}
设备名: ${DEVICE_NAME}
设备ID: ${DEVICE_ID}
虚拟IP: ${VIRTUAL_IP}
协议: ${PROTOCOL}
旧端口: ${OLD_LOCAL_PORT}
新端口: ${LOCAL_PORT}
远端地址: ${REMOTE_ADDR}
服务端配置: ${SERVER_ADDR}
重连次数: ${RECONNECT_COUNT}
时间戳: ${TIMESTAMP}"

# 通用表单 POST 示例。
# 如果你的推送服务字段名不同，只需要改这里的字段名。
curl -fsS --connect-timeout 3 --max-time 8 \
    --data-urlencode "title=${TITLE}" \
    --data-urlencode "content=${CONTENT}" \
    "$PUSH_URL" >/dev/null 2>&1

exit 0
