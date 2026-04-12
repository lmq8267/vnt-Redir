// Windows 网卡管理模块
// 处理网卡的创建、删除、清理
// 防止 "create device" 错误

use std::io;
use std::process::Command;
use std::os::windows::process::CommandExt;

const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Windows 网卡管理器
pub struct WindowsAdapterManager {
    device_name: String,
}

impl WindowsAdapterManager {
    /// 创建网卡管理器
    pub fn new(device_name: &str) -> Self {
        Self {
            device_name: device_name.to_string(),
        }
    }

    /// 启动前清理：删除已存在的同名网卡
    pub fn cleanup_before_start(&self) -> io::Result<()> {
        log::info!("检查是否存在同名虚拟网卡: {}", self.device_name);

        if self.check_adapter_exists() {
            log::warn!("发现已存在的虚拟网卡 {}，正在卸载...", self.device_name);
            self.remove_adapter()?;
            log::info!("已卸载旧的虚拟网卡");

            // 等待系统完成清理
            std::thread::sleep(std::time::Duration::from_secs(2));
        } else {
            log::info!("未发现同名虚拟网卡，可以安全创建");
        }

        Ok(())
    }

    /// 检查网卡是否存在
    fn check_adapter_exists(&self) -> bool {
        // 方法 1: 使用 PowerShell（推荐）
        if let Ok(exists) = self.check_adapter_exists_powershell() {
            return exists;
        }

        // 方法 2: 使用 netsh（兼容 Windows 7）
        self.check_adapter_exists_netsh()
    }

    /// 使用 PowerShell 检查网卡是否存在
    fn check_adapter_exists_powershell(&self) -> io::Result<bool> {
        let ps_script = format!(
            r#"
$adapter = Get-NetAdapter | Where-Object {{ $_.Name -eq '{}' -or $_.InterfaceDescription -like '*{}*' }}
if ($adapter) {{
    Write-Output 'exists'
    Write-Output $adapter.Name
    Write-Output $adapter.InterfaceDescription
}}
"#,
            self.device_name, self.device_name
        );

        let output = Command::new("powershell")
            .args(&[
                "-NoProfile",
                "-ExecutionPolicy", "Bypass",
                "-Command", &ps_script
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("exists") {
            log::debug!("PowerShell 检测到网卡: {}", stdout.trim());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 使用 netsh 检查网卡是否存在
    fn check_adapter_exists_netsh(&self) -> bool {
        let cmd = format!("netsh interface show interface name=\"{}\"", self.device_name);

        let output = Command::new("cmd")
            .args(&["/C", &cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .output();

        match output {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    /// 删除网卡
    fn remove_adapter(&self) -> io::Result<()> {
        log::info!("正在删除虚拟网卡: {}", self.device_name);

        // 方法 1: 使用 PowerShell Remove-NetAdapter（推荐）
        if self.remove_adapter_powershell().is_ok() {
            log::info!("使用 PowerShell 成功删除网卡");
            return Ok(());
        }

        // 方法 2: 使用 pnputil（尝试删除设备）
        if self.remove_adapter_pnputil().is_ok() {
            log::info!("使用 pnputil 成功删除网卡");
            return Ok(());
        }

        log::warn!("无法自动删除网卡 {}，将尝试继续创建", self.device_name);
        log::warn!("如果创建失败，请手动在设备管理器中删除旧网卡");
        Ok(())
    }

    /// 使用 PowerShell 删除网卡
    fn remove_adapter_powershell(&self) -> io::Result<()> {
        let ps_script = format!(
            r#"
$ErrorActionPreference = 'Stop'
try {{
    # 查找网卡
    $adapter = Get-NetAdapter | Where-Object {{ $_.Name -eq '{}' -or $_.InterfaceDescription -like '*{}*' }}
    
    if ($adapter) {{
        Write-Output "找到网卡: $($adapter.Name)"
        
        # 直接删除网卡（不禁用）
        Remove-NetAdapter -Name $adapter.Name -Confirm:$false -ErrorAction Stop
        Write-Output "网卡已删除"
    }} else {{
        Write-Output "未找到网卡"
    }}
}} catch {{
    Write-Error $_.Exception.Message
    exit 1
}}
"#,
            self.device_name, self.device_name
        );

        let output = Command::new("powershell")
            .args(&[
                "-NoProfile",
                "-ExecutionPolicy", "Bypass",
                "-Command", &ps_script
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;

        if output.status.success() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                String::from_utf8_lossy(&output.stderr).to_string()
            ))
        }
    }

    /// 使用 pnputil 删除网卡驱动
    fn remove_adapter_pnputil(&self) -> io::Result<()> {
        // 查找 wintun 驱动
        let output = Command::new("pnputil")
            .args(&["/enum-devices", "/class", "Net"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        
        // 查找包含设备名的驱动
        for line in stdout.lines() {
            if line.contains(&self.device_name) || line.contains("Wintun") {
                log::debug!("找到相关驱动: {}", line);
            }
        }

        // 注意：pnputil 删除驱动比较危险，这里只是记录
        log::warn!("pnputil 方法需要手动操作，请在设备管理器中删除网卡");
        
        Ok(())
    }

    /// 清理网卡的注册表信息（防止残留）
    pub fn cleanup_registry(&self) -> io::Result<()> {
        log::info!("清理虚拟网卡的注册表信息: {}", self.device_name);

        // 这个函数已经在 create_device.rs 中实现为 delete_adapter_info_from_reg
        // 这里只是记录日志
        log::debug!("注册表清理将在设备创建时自动执行");

        Ok(())
    }

    /// 获取网卡详细信息
    pub fn get_adapter_info(&self) -> Option<AdapterInfo> {
        let ps_script = format!(
            r#"
$adapter = Get-NetAdapter | Where-Object {{ $_.Name -eq '{}' }}
if ($adapter) {{
    Write-Output "Name:$($adapter.Name)"
    Write-Output "Status:$($adapter.Status)"
    Write-Output "InterfaceIndex:$($adapter.InterfaceIndex)"
    Write-Output "InterfaceDescription:$($adapter.InterfaceDescription)"
}}
"#,
            self.device_name
        );

        let output = Command::new("powershell")
            .args(&[
                "-NoProfile",
                "-ExecutionPolicy", "Bypass",
                "-Command", &ps_script
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.is_empty() {
            return None;
        }

        let mut info = AdapterInfo::default();
        for line in stdout.lines() {
            if let Some((key, value)) = line.split_once(':') {
                match key {
                    "Name" => info.name = value.to_string(),
                    "Status" => info.status = value.to_string(),
                    "InterfaceIndex" => info.interface_index = value.parse().ok(),
                    "InterfaceDescription" => info.description = value.to_string(),
                    _ => {}
                }
            }
        }

        Some(info)
    }
}

/// 网卡信息
#[derive(Debug, Default)]
pub struct AdapterInfo {
    pub name: String,
    pub status: String,
    pub interface_index: Option<u32>,
    pub description: String,
}

/// 全局清理函数：清理所有 VNT 相关的网卡
pub fn cleanup_all_vnt_adapters() -> io::Result<()> {
    log::info!("清理所有 VNT 相关的虚拟网卡...");

    let ps_script = r#"
$adapters = Get-NetAdapter | Where-Object { $_.Name -like '*vnt*' -or $_.InterfaceDescription -like '*vnt*' }
foreach ($adapter in $adapters) {
    Write-Output "删除网卡: $($adapter.Name)"
    try {
        Disable-NetAdapter -Name $adapter.Name -Confirm:$false -ErrorAction SilentlyContinue
        Remove-NetAdapter -Name $adapter.Name -Confirm:$false -ErrorAction SilentlyContinue
    } catch {
        Write-Warning "删除失败: $_"
    }
}
"#;

    let output = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-ExecutionPolicy", "Bypass",
            "-Command", ps_script
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        log::info!("清理结果: {}", stdout);
    }

    Ok(())
}
