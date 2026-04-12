// Windows 防火墙管理模块
// 参考 WireGuard 和 Tailscale 的实现方式
// 兼容 Windows 7 - Windows 11/12

use std::io;
use std::process::Command;
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Windows 防火墙管理器
pub struct WindowsFirewallManager {
    device_name: String,
    app_rule_name: String,
    interface_rule_name_in: String,
    interface_rule_name_out: String,
}

impl WindowsFirewallManager {
    /// 创建防火墙管理器
    pub fn new(device_name: &str) -> Self {
        Self {
            device_name: device_name.to_string(),
            app_rule_name: "VNT Virtual Network".to_string(),
            interface_rule_name_in: format!("VNT-Interface-{}-In", device_name),
            interface_rule_name_out: format!("VNT-Interface-{}-Out", device_name),
        }
    }

    /// 配置所有防火墙规则（启动时调用）
    pub fn configure_all(&self) -> io::Result<()> {
        log::info!("正在配置 Windows 防火墙规则...");

        // 1. 配置应用程序规则
        if let Err(e) = self.configure_application_rules() {
            log::warn!("配置应用程序防火墙规则失败: {:?}", e);
            log::warn!("这可能影响 UDP 打洞和 P2P 连接");
        } else {
            log::info!("应用程序防火墙规则配置成功");
        }

        // 2. 配置虚拟网卡规则
        if let Err(e) = self.configure_interface_rules() {
            log::warn!("配置虚拟网卡防火墙规则失败: {:?}", e);
            log::warn!("这可能影响虚拟网络内的通信");
        } else {
            log::info!("虚拟网卡防火墙规则配置成功");
        }

        log::info!("Windows 防火墙配置完成");
        Ok(())
    }

    /// 清理所有防火墙规则（退出时调用）
    pub fn cleanup_all(&self) -> io::Result<()> {
        log::info!("正在清理 Windows 防火墙规则...");

        // 清理应用程序规则
        let _ = self.remove_application_rules();

        // 清理虚拟网卡规则
        let _ = self.remove_interface_rules();

        log::info!("Windows 防火墙规则清理完成");
        Ok(())
    }

    /// 配置应用程序防火墙规则（用于 UDP 打洞）
    fn configure_application_rules(&self) -> io::Result<()> {
        let exe_path = std::env::current_exe()?;
        let exe_path_str = exe_path.to_string_lossy();

        log::info!("配置应用程序防火墙规则: {}", self.app_rule_name);

        // 先删除旧规则（如果存在）
        self.remove_application_rules()?;

        // 添加入站规则（UDP 协议，用于 P2P 打洞）
        let cmd = format!(
            "netsh advfirewall firewall add rule name=\"{}\" dir=in action=allow program=\"{}\" protocol=UDP enable=yes profile=any",
            self.app_rule_name, exe_path_str
        );
        self.execute_cmd(&cmd)?;
        log::info!("已添加应用程序入站规则（UDP）");

        // 添加出站规则
        let cmd = format!(
            "netsh advfirewall firewall add rule name=\"{} Out\" dir=out action=allow program=\"{}\" enable=yes profile=any",
            self.app_rule_name, exe_path_str
        );
        self.execute_cmd(&cmd)?;
        log::info!("已添加应用程序出站规则");

        Ok(())
    }

    /// 配置虚拟网卡防火墙规则（入站、出站、转发）
    fn configure_interface_rules(&self) -> io::Result<()> {
        log::info!("配置虚拟网卡防火墙规则: {}", self.device_name);

        // 先删除旧规则（如果存在）
        self.remove_interface_rules()?;

        // 等待网卡创建完成（最多等待 5 秒）
        if !self.wait_for_interface(5000) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("虚拟网卡 {} 未找到，无法配置防火墙规则", self.device_name)
            ));
        }

        // 使用 PowerShell 配置网卡规则（更精确，支持接口别名）
        let ps_script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$interfaceName = '{}'
$ruleNameIn = '{}'
$ruleNameOut = '{}'

try {{
    # 检查网卡是否存在
    $adapter = Get-NetAdapter | Where-Object {{ $_.Name -eq $interfaceName }}
    if (-not $adapter) {{
        Write-Error "网卡 $interfaceName 未找到"
        exit 1
    }}

    # 添加入站规则
    New-NetFirewallRule -DisplayName $ruleNameIn `
        -Direction Inbound `
        -InterfaceAlias $interfaceName `
        -Action Allow `
        -Profile Any `
        -ErrorAction Stop | Out-Null
    Write-Output "入站规则已添加"

    # 添加出站规则
    New-NetFirewallRule -DisplayName $ruleNameOut `
        -Direction Outbound `
        -InterfaceAlias $interfaceName `
        -Action Allow `
        -Profile Any `
        -ErrorAction Stop | Out-Null
    Write-Output "出站规则已添加"

    Write-Output "Success"
}} catch {{
    Write-Error $_.Exception.Message
    exit 1
}}
"#,
            self.device_name, self.interface_rule_name_in, self.interface_rule_name_out
        );

        match self.execute_powershell(&ps_script) {
            Ok(_) => {
                log::info!("虚拟网卡防火墙规则配置成功（入站+出站）");
                Ok(())
            }
            Err(e) => {
                // PowerShell 失败，尝试使用 netsh（兼容 Windows 7）
                log::warn!("PowerShell 配置失败，尝试使用 netsh: {:?}", e);
                self.configure_interface_rules_netsh()
            }
        }
    }

    /// 使用 netsh 配置网卡规则（Windows 7 兼容）
    fn configure_interface_rules_netsh(&self) -> io::Result<()> {
        log::info!("使用 netsh 配置虚拟网卡防火墙规则（兼容模式）");

        // netsh 不支持直接按接口名过滤，使用通配符规则
        // 入站规则：允许所有到虚拟网段的流量
        let cmd = format!(
            "netsh advfirewall firewall add rule name=\"{}\" dir=in action=allow enable=yes profile=any",
            self.interface_rule_name_in
        );
        self.execute_cmd(&cmd)?;
        log::info!("已添加虚拟网卡入站规则（netsh）");

        // 出站规则
        let cmd = format!(
            "netsh advfirewall firewall add rule name=\"{}\" dir=out action=allow enable=yes profile=any",
            self.interface_rule_name_out
        );
        self.execute_cmd(&cmd)?;
        log::info!("已添加虚拟网卡出站规则（netsh）");

        Ok(())
    }

    /// 删除应用程序防火墙规则
    fn remove_application_rules(&self) -> io::Result<()> {
        // 删除入站规则
        let cmd = format!(
            "netsh advfirewall firewall delete rule name=\"{}\"",
            self.app_rule_name
        );
        let _ = self.execute_cmd(&cmd);

        // 删除出站规则
        let cmd = format!(
            "netsh advfirewall firewall delete rule name=\"{} Out\"",
            self.app_rule_name
        );
        let _ = self.execute_cmd(&cmd);

        Ok(())
    }

    /// 删除虚拟网卡防火墙规则
    fn remove_interface_rules(&self) -> io::Result<()> {
        // 尝试使用 PowerShell 删除
        let ps_script = format!(
            r#"
Remove-NetFirewallRule -DisplayName '{}' -ErrorAction SilentlyContinue
Remove-NetFirewallRule -DisplayName '{}' -ErrorAction SilentlyContinue
"#,
            self.interface_rule_name_in, self.interface_rule_name_out
        );
        let _ = self.execute_powershell(&ps_script);

        // 使用 netsh 删除（兼容 Windows 7）
        let cmd = format!(
            "netsh advfirewall firewall delete rule name=\"{}\"",
            self.interface_rule_name_in
        );
        let _ = self.execute_cmd(&cmd);

        let cmd = format!(
            "netsh advfirewall firewall delete rule name=\"{}\"",
            self.interface_rule_name_out
        );
        let _ = self.execute_cmd(&cmd);

        Ok(())
    }

    /// 等待网卡创建完成
    fn wait_for_interface(&self, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);

        while start.elapsed() < timeout {
            if self.check_interface_exists() {
                log::info!("虚拟网卡 {} 已就绪", self.device_name);
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        log::warn!("等待虚拟网卡 {} 超时", self.device_name);
        false
    }

    /// 检查网卡是否存在
    fn check_interface_exists(&self) -> bool {
        let ps_script = format!(
            r#"
$adapter = Get-NetAdapter | Where-Object {{ $_.Name -eq '{}' }}
if ($adapter) {{ Write-Output 'exists' }}
"#,
            self.device_name
        );

        match self.execute_powershell(&ps_script) {
            Ok(output) => output.contains("exists"),
            Err(_) => {
                // PowerShell 失败，尝试使用 netsh
                let cmd = format!("netsh interface show interface name=\"{}\"", self.device_name);
                self.execute_cmd(&cmd).is_ok()
            }
        }
    }

    /// 执行 CMD 命令
    fn execute_cmd(&self, cmd: &str) -> io::Result<String> {
        log::debug!("执行命令: {}", cmd);

        let output = Command::new("cmd")
            .args(&["/C", cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("命令执行失败: {}\n错误: {}", cmd, stderr)
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// 执行 PowerShell 脚本
    fn execute_powershell(&self, script: &str) -> io::Result<String> {
        log::debug!("执行 PowerShell 脚本");

        let output = Command::new("powershell")
            .args(&[
                "-NoProfile",
                "-ExecutionPolicy", "Bypass",
                "-Command", script
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("PowerShell 执行失败: {}", stderr)
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

/// 检查防火墙规则是否已存在
pub fn check_firewall_rule_exists(rule_name: &str) -> bool {
    let cmd = format!(
        "netsh advfirewall firewall show rule name=\"{}\"",
        rule_name
    );

    let output = Command::new("cmd")
        .args(&["/C", &cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    match output {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}
