@echo off
rem vnt hook Windows 输出示例脚本。
rem 使用方式示例:
rem   --hook "C:\path\to\vnt-hook-example.bat"
rem
rem 本脚本只输出 vnt 传递的变量，不修改 Windows 防火墙。
rem 如果路径或文件名包含空格、中文，请在 --hook 参数中使用引号包裹完整命令。
rem 示例:
rem   --hook ""C:\中文目录\vnt hook\vnt-hook-example.bat""

set "LOG_FILE=%TEMP%\vnt-hook-example.log"

rem 支持的变量:
rem VNT_HOOK_EVENT            事件: up/down/reconnect/stop
rem VNT_HOOK_STATUS           与 VNT_HOOK_EVENT 相同，兼容状态脚本
rem VNT_HOOK_PROTOCOL         当前通道协议: udp/tcp/ws/wss，无法获取时为空
rem VNT_HOOK_LOCAL_PORT       当前新的本地随机端口，无法获取时为空；tcp/udp 重连成功时通常会有值
rem VNT_HOOK_OLD_LOCAL_PORT   上一次本地随机端口，无法获取时为空
rem VNT_HOOK_REMOTE_ADDR      当前远端地址，格式 ip:port，无法获取时为空
rem VNT_HOOK_TUN_NAME         虚拟网卡名，无法获取时为空
rem VNT_HOOK_DEVICE_NAME      vnt 设备名，无法获取时为空
rem VNT_HOOK_DEVICE_ID        vnt 设备 ID，无法获取时为空
rem VNT_HOOK_VIRTUAL_IP       服务端分配的虚拟 IP，无法获取时为空
rem VNT_HOOK_SERVER_ADDR      配置中的服务端地址，无法获取时为空
rem VNT_HOOK_RECONNECT_COUNT  当前重连次数，无法获取时为空
rem VNT_HOOK_REASON           触发原因: registered/route_timeout/server_disconnect/rebind/stop
rem VNT_HOOK_PID              vnt 进程 ID
rem VNT_HOOK_TIMESTAMP        触发时间戳，秒级

(
    echo ========================================
    echo time=%DATE% %TIME%
    echo VNT_HOOK_EVENT=%VNT_HOOK_EVENT%
    echo VNT_HOOK_STATUS=%VNT_HOOK_STATUS%
    echo VNT_HOOK_PROTOCOL=%VNT_HOOK_PROTOCOL%
    echo VNT_HOOK_LOCAL_PORT=%VNT_HOOK_LOCAL_PORT%
    echo VNT_HOOK_OLD_LOCAL_PORT=%VNT_HOOK_OLD_LOCAL_PORT%
    echo VNT_HOOK_REMOTE_ADDR=%VNT_HOOK_REMOTE_ADDR%
    echo VNT_HOOK_TUN_NAME=%VNT_HOOK_TUN_NAME%
    echo VNT_HOOK_DEVICE_NAME=%VNT_HOOK_DEVICE_NAME%
    echo VNT_HOOK_DEVICE_ID=%VNT_HOOK_DEVICE_ID%
    echo VNT_HOOK_VIRTUAL_IP=%VNT_HOOK_VIRTUAL_IP%
    echo VNT_HOOK_SERVER_ADDR=%VNT_HOOK_SERVER_ADDR%
    echo VNT_HOOK_RECONNECT_COUNT=%VNT_HOOK_RECONNECT_COUNT%
    echo VNT_HOOK_REASON=%VNT_HOOK_REASON%
    echo VNT_HOOK_PID=%VNT_HOOK_PID%
    echo VNT_HOOK_TIMESTAMP=%VNT_HOOK_TIMESTAMP%
) >> "%LOG_FILE%" 2>nul

exit /B 0
