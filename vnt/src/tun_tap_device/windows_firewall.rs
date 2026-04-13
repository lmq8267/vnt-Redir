// Windows 防火墙管理 - 使用 windows crate（Win7-12 兼容）
use std::io;
use std::mem::ManuallyDrop;
use windows::Win32::Foundation::*;
use windows::Win32::NetworkManagement::WindowsFirewall::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::Variant::*;
use windows::core::BSTR;

pub struct WindowsFirewallManager {
    device_name: String,
    actual_name: String,
}

impl WindowsFirewallManager {
    pub fn new(device_name: &str) -> Self {
        Self {
            device_name: device_name.to_string(),
            actual_name: device_name.to_string(),
        }
    }
    
    pub fn new_with_actual(device_name: &str, actual_name: &str) -> Self {
        Self {
            device_name: device_name.to_string(),
            actual_name: actual_name.to_string(),
        }
    }

    pub fn configure_all(&self) -> io::Result<()> {
        log::info!("配置防火墙规则 - 规则名: {}, 绑定接口: {}", self.device_name, self.actual_name);
        
        unsafe {
            match CoInitializeEx(None, COINIT_APARTMENTTHREADED) {
                Ok(_) => {},
                Err(e) if e.code().0 == 0x00000001 => {}, // RPC_E_CHANGED_MODE
                Err(_) => {
                    log::warn!("COM 初始化失败，跳过防火墙配置");
                    return Ok(());
                }
            }

            let result = self.configure_all_internal();
            CoUninitialize();
            
            match result {
                Ok(_) => {
                    log::info!("防火墙规则配置完成");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("防火墙配置失败: {}，程序将继续运行", e);
                    Ok(())
                }
            }
        }
    }

    unsafe fn configure_all_internal(&self) -> windows::core::Result<()> {
        let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_ALL)?;
        let rules = policy.Rules()?;

        // 删除已存在的规则
        let rule_names = self.get_all_rule_names();
        if !rule_names.is_empty() {
            log::info!("发现 {} 条已存在的VNT防火墙规则，正在清理...", rule_names.len());
            for name in rule_names {
                let _ = rules.Remove(&BSTR::from(&name));
            }
        }

        // 添加程序规则
        self.add_app_rule(&rules, 6, "TCP")?;
        self.add_app_rule(&rules, 17, "UDP")?;

        // 添加接口规则
        self.add_interface_rule(&rules, "Inbound", NET_FW_RULE_DIR_IN)?;
        self.add_interface_rule(&rules, "Outbound", NET_FW_RULE_DIR_OUT)?;

        Ok(())
    }

    pub fn cleanup_all(&self) -> io::Result<()> {
        log::info!("清理防火墙规则: {}", self.device_name);
        
        unsafe {
            match CoInitializeEx(None, COINIT_APARTMENTTHREADED) {
                Ok(_) => {},
                Err(e) if e.code().0 == 0x00000001 => {},
                Err(_) => return Ok(()),
            }

            let result = self.cleanup_all_internal();
            CoUninitialize();
            
            match result {
                Ok(_) => {
                    log::info!("防火墙规则已清理");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("防火墙清理失败: {}", e);
                    Ok(())
                }
            }
        }
    }

    unsafe fn cleanup_all_internal(&self) -> windows::core::Result<()> {
        let policy: INetFwPolicy2 = CoCreateInstance(&NetFwPolicy2, None, CLSCTX_ALL)?;
        let rules = policy.Rules()?;
        
        for name in self.get_all_rule_names() {
            let _ = rules.Remove(&BSTR::from(&name));
        }
        
        Ok(())
    }

    unsafe fn add_app_rule(&self, rules: &INetFwRules, protocol: i32, protocol_name: &str) -> windows::core::Result<()> {
        let exe_path = std::env::current_exe()
            .map_err(|_| windows::core::Error::from_win32())?;

        // 使用设备名区分不同实例的规则
        let rule_name = format!("VNT-{} - {}", self.device_name, protocol_name);
        let rule: INetFwRule = CoCreateInstance(&NetFwRule, None, CLSCTX_ALL)?;

        rule.SetName(&BSTR::from(&rule_name))?;
        rule.SetDescription(&BSTR::from(format!("Allow VNT {} traffic for {}", protocol_name, self.device_name)))?;
        rule.SetApplicationName(&BSTR::from(exe_path.to_string_lossy().as_ref()))?;
        rule.SetProtocol(protocol)?;
        rule.SetDirection(NET_FW_RULE_DIR_OUT)?;
        rule.SetAction(NET_FW_ACTION_ALLOW)?;
        rule.SetEnabled(VARIANT_TRUE)?;
        rule.SetProfiles(0x7FFFFFFF)?;
        rule.SetGrouping(&BSTR::from("VNT"))?;

        rules.Add(&rule)?;
        log::info!("已添加程序 {} 规则", protocol_name);
        Ok(())
    }

    unsafe fn add_interface_rule(&self, rules: &INetFwRules, direction: &str, dir_value: NET_FW_RULE_DIRECTION) -> windows::core::Result<()> {
        let rule_name = format!("VNT-Interface-{} ({})", self.device_name, direction);
        let rule: INetFwRule = CoCreateInstance(&NetFwRule, None, CLSCTX_ALL)?;

        rule.SetName(&BSTR::from(&rule_name))?;
        rule.SetDescription(&BSTR::from(format!("Allow all traffic on VNT interface {}", self.device_name)))?;
        rule.SetProtocol(256)?; // 所有协议
        rule.SetDirection(dir_value)?;
        rule.SetAction(NET_FW_ACTION_ALLOW)?;
        rule.SetEnabled(VARIANT_TRUE)?;
        rule.SetProfiles(0x7FFFFFFF)?;
        rule.SetGrouping(&BSTR::from("VNT"))?;

        // 绑定到实际接口
        let interface_bstr = BSTR::from(&self.actual_name);
        let interface_array = unsafe { SafeArrayCreateVector(VT_VARIANT, 0, 1) };
        if interface_array.is_null() {
            return Err(windows::core::Error::from_win32());
        }
        
        let index = 0i32;
        let mut variant_interface = VARIANT::default();
        unsafe {
            (*variant_interface.Anonymous.Anonymous).vt = VT_BSTR;
            (*variant_interface.Anonymous.Anonymous).Anonymous.bstrVal = ManuallyDrop::new(interface_bstr);

            SafeArrayPutElement(
                interface_array,
                &index as *const _,
                &variant_interface as *const _ as *const std::ffi::c_void,
            )?;

            let mut interface_variant = VARIANT::default();
            (*interface_variant.Anonymous.Anonymous).vt = VARENUM(VT_ARRAY.0 | VT_VARIANT.0);
            (*interface_variant.Anonymous.Anonymous).Anonymous.parray = interface_array;

            rule.SetInterfaces(interface_variant)?;
        }
        rules.Add(&rule)?;
        
        log::info!("已添加接口规则: {}", rule_name);
        Ok(())
    }

    fn get_all_rule_names(&self) -> Vec<String> {
        vec![
            format!("VNT-{} - TCP", self.device_name),
            format!("VNT-{} - UDP", self.device_name),
            format!("VNT-Interface-{} (Inbound)", self.device_name),
            format!("VNT-Interface-{} (Outbound)", self.device_name),
        ]
    }
}
