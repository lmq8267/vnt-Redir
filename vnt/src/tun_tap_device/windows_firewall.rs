// Windows 防火墙管理 - 改进版
use std::io;
use std::ptr;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::System::Com::*;
use windows_sys::core::{GUID, BSTR};

type IUnknown = std::ffi::c_void;

const CLSID_NETFWPOLICY2: GUID = GUID { 
    data1: 0xE2B3C97F, data2: 0x6AE1, data3: 0x41AC, 
    data4: [0x81, 0x7A, 0xF6, 0xF9, 0x21, 0x66, 0xD7, 0xDD] 
};
const IID_INETFWPOLICY2: GUID = GUID { 
    data1: 0x98325047, data2: 0xC671, data3: 0x4174, 
    data4: [0x8D, 0x81, 0xDE, 0xFC, 0xD3, 0xF0, 0x31, 0x86] 
};
const CLSID_NETFWRULE: GUID = GUID { 
    data1: 0x2C5BC43E, data2: 0x3369, data3: 0x4C33, 
    data4: [0xAB, 0x0C, 0xBE, 0x94, 0x69, 0x67, 0x7A, 0xF4] 
};
const IID_INETFWRULE: GUID = GUID { 
    data1: 0xAF230D27, data2: 0xBABA, data3: 0x4E42, 
    data4: [0xAC, 0xED, 0xF5, 0x24, 0xF2, 0x2C, 0xFC, 0xE2] 
};

#[repr(C)]
struct INetFwPolicy2Vtbl {
    query_interface: usize, add_ref: usize, release: unsafe extern "system" fn(*mut IUnknown) -> u32,
    get_type_info_count: usize, get_type_info: usize, get_ids_of_names: usize, invoke: usize,
    get_current_profile_types: usize, get_firewall_enabled: usize, put_firewall_enabled: usize,
    get_excluded_interfaces: usize, put_excluded_interfaces: usize, get_blocked_inbound_traffic: usize,
    put_blocked_inbound_traffic: usize, get_notifications_disabled: usize, put_notifications_disabled: usize,
    get_unicast_responses_to_multicast_broadcast_disabled: usize, put_unicast_responses_to_multicast_broadcast_disabled: usize,
    get_rules: unsafe extern "system" fn(*mut IUnknown, *mut *mut IUnknown) -> i32,
    get_service_restriction: usize, enable_rule_group: usize, is_rule_group_enabled: usize, restore_local_firewall_defaults: usize,
}

#[repr(C)]
struct INetFwRulesVtbl {
    query_interface: usize, add_ref: usize, release: unsafe extern "system" fn(*mut IUnknown) -> u32,
    get_type_info_count: usize, get_type_info: usize, get_ids_of_names: usize, invoke: usize, get_count: usize,
    add: unsafe extern "system" fn(*mut IUnknown, *mut IUnknown) -> i32,
    remove: unsafe extern "system" fn(*mut IUnknown, BSTR) -> i32,
    item: unsafe extern "system" fn(*mut IUnknown, BSTR, *mut *mut IUnknown) -> i32,
    get__new_enum: usize,
}

#[repr(C)]
struct INetFwRuleVtbl {
    query_interface: usize, add_ref: usize, release: unsafe extern "system" fn(*mut IUnknown) -> u32,
    get_type_info_count: usize, get_type_info: usize, get_ids_of_names: usize, invoke: usize,
    get_name: usize, put_name: unsafe extern "system" fn(*mut IUnknown, BSTR) -> i32,
    get_description: usize, put_description: unsafe extern "system" fn(*mut IUnknown, BSTR) -> i32,
    get_application_name: usize, put_application_name: unsafe extern "system" fn(*mut IUnknown, BSTR) -> i32,
    get_service_name: usize, put_service_name: usize,
    get_protocol: usize, put_protocol: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
    get_local_ports: usize, put_local_ports: usize, get_remote_ports: usize, put_remote_ports: usize,
    get_local_addresses: usize, put_local_addresses: usize, get_remote_addresses: usize, put_remote_addresses: usize,
    get_icmp_types_and_codes: usize, put_icmp_types_and_codes: usize,
    get_direction: usize, put_direction: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
    get_interfaces: usize, put_interfaces: usize, get_interface_types: usize, put_interface_types: usize,
    get_enabled: usize, put_enabled: unsafe extern "system" fn(*mut IUnknown, i16) -> i32,
    get_grouping: usize, put_grouping: unsafe extern "system" fn(*mut IUnknown, BSTR) -> i32,
    get_profiles: usize, put_profiles: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
    get_edge_traversal: usize, put_edge_traversal: usize,
    get_action: usize, put_action: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
}

unsafe fn sys_alloc_string(s: *const u16) -> BSTR {
    let len = (0..).take_while(|&i| *s.offset(i) != 0).count();
    let bstr = CoTaskMemAlloc((len + 1) * 2) as *mut u16;
    if !bstr.is_null() {
        ptr::copy_nonoverlapping(s, bstr, len);
        *bstr.offset(len as isize) = 0;
    }
    bstr
}

unsafe fn sys_free_string(bstr: BSTR) {
    if !bstr.is_null() {
        CoTaskMemFree(bstr as _);
    }
}

pub struct WindowsFirewallManager {
    device_name: String,
}

impl WindowsFirewallManager {
    pub fn new(device_name: &str) -> Self {
        Self {
            device_name: device_name.to_string(),
        }
    }

    pub fn configure_all(&self) -> io::Result<()> {
        log::info!("配置防火墙规则");
        
        unsafe {
            let hr = CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED as u32);
            if hr < 0 && hr != 0x00000001 {
                log::warn!("COM 初始化失败，跳过防火墙配置");
                return Ok(());
            }

            let result = self.configure_all_internal();
            CoUninitialize();
            
            match result {
                Ok(_) => {
                    log::info!("防火墙配置完成");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("防火墙配置失败: {:?}，程序将继续运行", e);
                    Ok(())
                }
            }
        }
    }

    unsafe fn configure_all_internal(&self) -> io::Result<()> {
        self.add_app_rules()?;
        self.add_interface_rules()?;
        Ok(())
    }

    pub fn cleanup_all(&self) -> io::Result<()> {
        log::info!("清理防火墙规则");
        
        unsafe {
            let hr = CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED as u32);
            if hr < 0 && hr != 0x00000001 {
                return Ok(());
            }

            let result = self.cleanup_all_internal();
            CoUninitialize();
            
            match result {
                Ok(_) => {
                    log::info!("防火墙规则已清理");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("防火墙清理失败: {:?}", e);
                    Ok(())
                }
            }
        }
    }

    unsafe fn cleanup_all_internal(&self) -> io::Result<()> {
        let mut policy: *mut IUnknown = ptr::null_mut();
        if CoCreateInstance(&CLSID_NETFWPOLICY2, ptr::null_mut(), CLSCTX_INPROC_SERVER,
            &IID_INETFWPOLICY2, &mut policy as *mut *mut IUnknown as *mut *mut _) < 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "无法创建防火墙策略对象"));
        }

        let mut rules: *mut IUnknown = ptr::null_mut();
        let vtbl = *(policy as *const *const INetFwPolicy2Vtbl);
        
        if ((*vtbl).get_rules)(policy, &mut rules) >= 0 && !rules.is_null() {
            let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
            
            let rule_names = self.get_all_rule_names();
            for name in &rule_names {
                let name_wide: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
                let bstr = sys_alloc_string(name_wide.as_ptr());
                let _ = ((*rules_vtbl).remove)(rules, bstr);
                sys_free_string(bstr);
            }
            
            ((*rules_vtbl).release)(rules);
        }
        
        ((*vtbl).release)(policy);
        Ok(())
    }

    unsafe fn add_app_rules(&self) -> io::Result<()> {
        let exe_path = std::env::current_exe()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("无法获取程序路径: {}", e)))?;
        
        let rule_name = "VNT Virtual Network - UDP (Inbound)";
        if self.rule_exists(rule_name)? {
            let _ = self.delete_rule(rule_name);
        }
        self.create_rule(rule_name, "Allow VNT UDP traffic (Inbound)", 
                     Some(&exe_path.to_string_lossy()), 17, 1)?;
        
        let rule_name = "VNT Virtual Network - UDP (Outbound)";
        if self.rule_exists(rule_name)? {
            let _ = self.delete_rule(rule_name);
        }
        self.create_rule(rule_name, "Allow VNT UDP traffic (Outbound)", 
                     Some(&exe_path.to_string_lossy()), 17, 2)?;
        
        Ok(())
    }

    unsafe fn add_interface_rules(&self) -> io::Result<()> {
        // 入站规则 - 所有协议
        let rule_name = format!("VNT-Interface-{} (Inbound)", self.device_name);
        if self.rule_exists(&rule_name)? {
            let _ = self.delete_rule(&rule_name);
        }
        
        let desc = format!("Allow all traffic on VNT interface {}", self.device_name);
        self.create_rule(&rule_name, &desc, None, 256, 1)?;
        
        // 出站规则 - 所有协议
        let rule_name = format!("VNT-Interface-{} (Outbound)", self.device_name);
        if self.rule_exists(&rule_name)? {
            let _ = self.delete_rule(&rule_name);
        }
        
        self.create_rule(&rule_name, &desc, None, 256, 2)?;
        
        Ok(())
    }

    unsafe fn rule_exists(&self, rule_name: &str) -> io::Result<bool> {
        let mut policy: *mut IUnknown = ptr::null_mut();
        if CoCreateInstance(&CLSID_NETFWPOLICY2, ptr::null_mut(), CLSCTX_INPROC_SERVER,
            &IID_INETFWPOLICY2, &mut policy as *mut *mut IUnknown as *mut *mut _) < 0 {
            return Ok(false);
        }

        let mut rules: *mut IUnknown = ptr::null_mut();
        let policy_vtbl = *(policy as *const *const INetFwPolicy2Vtbl);
        
        let exists = if ((*policy_vtbl).get_rules)(policy, &mut rules) >= 0 && !rules.is_null() {
            let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
            
            let name_wide: Vec<u16> = OsStr::new(rule_name).encode_wide().chain(Some(0)).collect();
            let bstr = sys_alloc_string(name_wide.as_ptr());
            
            let mut rule: *mut IUnknown = ptr::null_mut();
            let hr = ((*rules_vtbl).item)(rules, bstr, &mut rule);
            
            sys_free_string(bstr);
            
            if hr >= 0 && !rule.is_null() {
                let rule_vtbl = *(rule as *const *const INetFwRuleVtbl);
                ((*rule_vtbl).release)(rule);
                true
            } else {
                false
            }
        } else {
            false
        };
        
        if !rules.is_null() {
            let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
            ((*rules_vtbl).release)(rules);
        }
        ((*policy_vtbl).release)(policy);
        
        Ok(exists)
    }

    unsafe fn delete_rule(&self, rule_name: &str) -> io::Result<()> {
        let mut policy: *mut IUnknown = ptr::null_mut();
        if CoCreateInstance(&CLSID_NETFWPOLICY2, ptr::null_mut(), CLSCTX_INPROC_SERVER,
            &IID_INETFWPOLICY2, &mut policy as *mut *mut IUnknown as *mut *mut _) < 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "无法创建策略对象"));
        }

        let mut rules: *mut IUnknown = ptr::null_mut();
        let policy_vtbl = *(policy as *const *const INetFwPolicy2Vtbl);
        
        if ((*policy_vtbl).get_rules)(policy, &mut rules) >= 0 && !rules.is_null() {
            let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
            
            let name_wide: Vec<u16> = OsStr::new(rule_name).encode_wide().chain(Some(0)).collect();
            let bstr = sys_alloc_string(name_wide.as_ptr());
            
            let _ = ((*rules_vtbl).remove)(rules, bstr);
            
            sys_free_string(bstr);
            ((*rules_vtbl).release)(rules);
        }
        
        ((*policy_vtbl).release)(policy);
        Ok(())
    }

    unsafe fn create_rule(&self, name: &str, description: &str, app_path: Option<&str>, 
                      protocol: i32, direction: i32) -> io::Result<()> {
        let mut policy: *mut IUnknown = ptr::null_mut();
        if CoCreateInstance(&CLSID_NETFWPOLICY2, ptr::null_mut(), CLSCTX_INPROC_SERVER,
            &IID_INETFWPOLICY2, &mut policy as *mut *mut IUnknown as *mut *mut _) < 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "无法创建策略对象"));
        }

        let mut rule: *mut IUnknown = ptr::null_mut();
        if CoCreateInstance(&CLSID_NETFWRULE, ptr::null_mut(), CLSCTX_INPROC_SERVER,
            &IID_INETFWRULE, &mut rule as *mut *mut IUnknown as *mut *mut _) < 0 {
            let policy_vtbl = *(policy as *const *const INetFwPolicy2Vtbl);
            ((*policy_vtbl).release)(policy);
            return Err(io::Error::new(io::ErrorKind::Other, "无法创建规则对象"));
        }

        let rule_vtbl = *(rule as *const *const INetFwRuleVtbl);
        
        let name_wide: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
        let name_bstr = sys_alloc_string(name_wide.as_ptr());
        ((*rule_vtbl).put_name)(rule, name_bstr);
        sys_free_string(name_bstr);

        let desc_wide: Vec<u16> = OsStr::new(description).encode_wide().chain(Some(0)).collect();
        let desc_bstr = sys_alloc_string(desc_wide.as_ptr());
        ((*rule_vtbl).put_description)(rule, desc_bstr);
        sys_free_string(desc_bstr);

        if let Some(path) = app_path {
            let path_wide: Vec<u16> = OsStr::new(path).encode_wide().chain(Some(0)).collect();
            let path_bstr = sys_alloc_string(path_wide.as_ptr());
            ((*rule_vtbl).put_application_name)(rule, path_bstr);
            sys_free_string(path_bstr);
        }

        ((*rule_vtbl).put_protocol)(rule, protocol);
        ((*rule_vtbl).put_direction)(rule, direction);
        ((*rule_vtbl).put_action)(rule, 1);
        ((*rule_vtbl).put_enabled)(rule, -1);
        ((*rule_vtbl).put_profiles)(rule, 0x7FFFFFFF);
        
        let group_wide: Vec<u16> = OsStr::new("VNT").encode_wide().chain(Some(0)).collect();
        let group_bstr = sys_alloc_string(group_wide.as_ptr());
        ((*rule_vtbl).put_grouping)(rule, group_bstr);
        sys_free_string(group_bstr);

        let mut rules: *mut IUnknown = ptr::null_mut();
        let policy_vtbl = *(policy as *const *const INetFwPolicy2Vtbl);
        
        let result = if ((*policy_vtbl).get_rules)(policy, &mut rules) >= 0 && !rules.is_null() {
            let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
            let hr = ((*rules_vtbl).add)(rules, rule);
            ((*rules_vtbl).release)(rules);
            
            if hr >= 0 {
                Ok(())
            } else {
                Err(io::Error::new(io::ErrorKind::Other, format!("添加规则失败 (HRESULT: 0x{:08X})", hr)))
            }
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "无法获取规则集合"))
        };

        ((*rule_vtbl).release)(rule);
        ((*policy_vtbl).release)(policy);
        
        result
    }

    fn get_all_rule_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        
        names.push("VNT Virtual Network - UDP (Inbound)".to_string());
        names.push("VNT Virtual Network - UDP (Outbound)".to_string());
        
        names.push(format!("VNT-Interface-{} (Inbound)", self.device_name));
        names.push(format!("VNT-Interface-{} (Outbound)", self.device_name));
        
        names
    }
}
