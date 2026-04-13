// Windows 防火墙管理
use std::io;
use std::ptr;
use std::mem;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::NetworkManagement::IpHelper::*;
use windows_sys::Win32::Networking::WinSock::*;
use windows_sys::Win32::System::Com::*;
use windows_sys::Win32::System::Ole::*;
use windows_sys::core::GUID;

type IUnknown = std::ffi::c_void;

// COM GUIDs
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

// VTable 定义
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
    remove: unsafe extern "system" fn(*mut IUnknown, *const u16) -> i32,
    item: unsafe extern "system" fn(*mut IUnknown, *const u16, *mut *mut IUnknown) -> i32,
    get__new_enum: usize,
}

#[repr(C)]
struct INetFwRuleVtbl {
    query_interface: usize, add_ref: usize, release: unsafe extern "system" fn(*mut IUnknown) -> u32,
    get_type_info_count: usize, get_type_info: usize, get_ids_of_names: usize, invoke: usize,
    get_name: usize, put_name: unsafe extern "system" fn(*mut IUnknown, *const u16) -> i32,
    get_description: usize, put_description: unsafe extern "system" fn(*mut IUnknown, *const u16) -> i32,
    get_application_name: usize, put_application_name: unsafe extern "system" fn(*mut IUnknown, *const u16) -> i32,
    get_service_name: usize, put_service_name: usize,
    get_protocol: usize, put_protocol: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
    get_local_ports: usize, put_local_ports: usize, get_remote_ports: usize, put_remote_ports: usize,
    get_local_addresses: usize, put_local_addresses: usize, get_remote_addresses: usize, put_remote_addresses: usize,
    get_icmp_types_and_codes: usize, put_icmp_types_and_codes: usize,
    get_direction: usize, put_direction: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
    get_interfaces: usize, 
    put_interfaces: unsafe extern "system" fn(*mut IUnknown, *const VARIANT) -> i32,
    get_interface_types: usize, put_interface_types: usize,
    get_enabled: usize, put_enabled: unsafe extern "system" fn(*mut IUnknown, i16) -> i32,
    get_grouping: usize, put_grouping: unsafe extern "system" fn(*mut IUnknown, *const u16) -> i32,
    get_profiles: usize, put_profiles: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
    get_edge_traversal: usize, put_edge_traversal: usize,
    get_action: usize, put_action: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
}

#[repr(C)]
struct VARIANT {
    vt: u16,
    reserved1: u16,
    reserved2: u16,
    reserved3: u16,
    data: usize,
}

unsafe fn bstr_from_str(s: &str) -> *const u16 {
    let wide: Vec<u16> = s.encode_utf16().chain(Some(0)).collect();
    let bstr = CoTaskMemAlloc(wide.len() * 2) as *mut u16;
    if !bstr.is_null() {
        ptr::copy_nonoverlapping(wide.as_ptr(), bstr, wide.len());
    }
    bstr
}

unsafe fn free_bstr(bstr: *const u16) {
    if !bstr.is_null() {
        CoTaskMemFree(bstr as _);
    }
}

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
            let hr = CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED as u32);
            if hr < 0 && hr != 0x00000001 {
                log::warn!("COM 初始化失败，跳过防火墙配置");
                return Ok(());
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

    unsafe fn configure_all_internal(&self) -> Result<(), String> {
        // 获取接口索引
        let if_index = self.get_interface_index()?;
        log::info!("找到接口索引: {} ({})", if_index, self.actual_name);

        let mut policy: *mut IUnknown = ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_NETFWPOLICY2,
            ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_INETFWPOLICY2,
            &mut policy as *mut _ as *mut _,
        );

        if hr < 0 {
            return Err(format!("创建防火墙策略失败: 0x{:X}", hr));
        }

        let policy_vtbl = *(policy as *const *const INetFwPolicy2Vtbl);
        let mut rules: *mut IUnknown = ptr::null_mut();
        let hr = ((*policy_vtbl).get_rules)(policy, &mut rules);

        if hr < 0 {
            ((*policy_vtbl).release)(policy);
            return Err(format!("获取规则集合失败: 0x{:X}", hr));
        }

        // 添加规则
        let result = self.add_all_rules(rules, if_index);

        let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
        ((*rules_vtbl).release)(rules);
        ((*policy_vtbl).release)(policy);

        result
    }

    pub fn cleanup_all(&self) -> io::Result<()> {
        log::info!("清理防火墙规则: {}", self.device_name);
        
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
                    log::warn!("防火墙清理失败: {}", e);
                    Ok(())
                }
            }
        }
    }

    unsafe fn cleanup_all_internal(&self) -> Result<(), String> {
        let mut policy: *mut IUnknown = ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_NETFWPOLICY2,
            ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_INETFWPOLICY2,
            &mut policy as *mut _ as *mut _,
        );

        if hr < 0 {
            return Err(format!("创建防火墙策略失败: 0x{:X}", hr));
        }

        let policy_vtbl = *(policy as *const *const INetFwPolicy2Vtbl);
        let mut rules: *mut IUnknown = ptr::null_mut();
        let hr = ((*policy_vtbl).get_rules)(policy, &mut rules);

        if hr < 0 {
            ((*policy_vtbl).release)(policy);
            return Err(format!("获取规则集合失败: 0x{:X}", hr));
        }

        let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
        
        for name in self.get_all_rule_names() {
            let bstr = bstr_from_str(&name);
            let _ = ((*rules_vtbl).remove)(rules, bstr);
            free_bstr(bstr);
        }

        ((*rules_vtbl).release)(rules);
        ((*policy_vtbl).release)(policy);
        
        Ok(())
    }

    unsafe fn get_interface_index(&self) -> Result<u32, String> {
        let mut size = 15000u32;
        let mut buffer = vec![0u8; size as usize];

        let ret = GetAdaptersAddresses(
            AF_UNSPEC as u32,
            0,
            ptr::null_mut(),
            buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
            &mut size,
        );

        if ret != 0 {
            return Err(format!("GetAdaptersAddresses 失败: {}", ret));
        }

        let mut current = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;

        while !current.is_null() {
            let adapter = &*current;

            if !adapter.FriendlyName.is_null() {
                let len = (0..).take_while(|&i| *adapter.FriendlyName.offset(i) != 0).count();
                let name = String::from_utf16_lossy(
                    std::slice::from_raw_parts(adapter.FriendlyName, len)
                );

                let name_lower = name.to_lowercase();
                let actual_lower = self.actual_name.to_lowercase();

                let match_ok =
                    name_lower == actual_lower
                    || name_lower.starts_with(&(actual_lower.clone() + " "))
                    || name_lower.starts_with(&(actual_lower.clone() + " ("));

                if match_ok {
                    return Ok(adapter.Anonymous1.Anonymous.IfIndex);
                }
            }

            current = adapter.Next;
        }

        Err(format!("未找到接口: {}", self.actual_name))
    }

    unsafe fn add_all_rules(&self, rules: *mut IUnknown, if_index: u32) -> Result<(), String> {
        // 程序 UDP 规则
        self.add_app_rule(rules, 17, "UDP")?;
        // 程序 TCP 规则
        self.add_app_rule(rules, 6, "TCP")?;
        
        // 虚拟网卡全协议规则
        self.add_interface_rule(rules, if_index, "Inbound", 1)?;
        self.add_interface_rule(rules, if_index, "Outbound", 2)?;
        
        Ok(())
    }

    unsafe fn add_app_rule(&self, rules: *mut IUnknown, protocol: i32, protocol_name: &str) -> Result<(), String> {
        let exe_path = std::env::current_exe()
            .map_err(|e| format!("获取程序路径失败: {}", e))?;

        let rule_name = format!("VNT Virtual Network - {}", protocol_name);
        
        let mut rule: *mut IUnknown = ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_NETFWRULE,
            ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_INETFWRULE,
            &mut rule as *mut _ as *mut _,
        );

        if hr < 0 {
            return Err(format!("创建规则失败: 0x{:X}", hr));
        }

        let rule_vtbl = *(rule as *const *const INetFwRuleVtbl);
        
        let name_bstr = bstr_from_str(&rule_name);
        let _ = ((*rule_vtbl).put_name)(rule, name_bstr);
        free_bstr(name_bstr);

        let app_bstr = bstr_from_str(&exe_path.to_string_lossy());
        let _ = ((*rule_vtbl).put_application_name)(rule, app_bstr);
        free_bstr(app_bstr);

        let _ = ((*rule_vtbl).put_protocol)(rule, protocol);
        let _ = ((*rule_vtbl).put_direction)(rule, 2); // 出站
        let _ = ((*rule_vtbl).put_enabled)(rule, -1); // VARIANT_TRUE
        let _ = ((*rule_vtbl).put_action)(rule, 1); // NET_FW_ACTION_ALLOW
        let _ = ((*rule_vtbl).put_profiles)(rule, 0x7FFFFFFF);

        let group_bstr = bstr_from_str("VNT");
        let _ = ((*rule_vtbl).put_grouping)(rule, group_bstr);
        free_bstr(group_bstr);

        let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
        let hr = ((*rules_vtbl).add)(rules, rule);

        ((*rule_vtbl).release)(rule);

        if hr < 0 {
            return Err(format!("添加程序 {} 规则失败: 0x{:X}", protocol_name, hr));
        }

        log::info!("已添加程序 {} 规则", protocol_name);
        Ok(())
    }

    unsafe fn add_interface_rule(&self, rules: *mut IUnknown, _if_index: u32, direction: &str, dir_value: i32) -> Result<(), String> {
        let rule_name = format!("VNT-Interface-{} ({})", self.device_name, direction);
        
        let mut rule: *mut IUnknown = ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_NETFWRULE,
            ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_INETFWRULE,
            &mut rule as *mut _ as *mut _,
        );

        if hr < 0 {
            return Err(format!("创建规则失败: 0x{:X}", hr));
        }

        let rule_vtbl = *(rule as *const *const INetFwRuleVtbl);
        
        let name_bstr = bstr_from_str(&rule_name);
        let _ = ((*rule_vtbl).put_name)(rule, name_bstr);
        free_bstr(name_bstr);

        let _ = ((*rule_vtbl).put_protocol)(rule, 256); // 所有协议
        let _ = ((*rule_vtbl).put_direction)(rule, dir_value);
        let _ = ((*rule_vtbl).put_enabled)(rule, -1);
        let _ = ((*rule_vtbl).put_action)(rule, 1);
        let _ = ((*rule_vtbl).put_profiles)(rule, 0x7FFFFFFF);

        let group_bstr = bstr_from_str("VNT");
        let _ = ((*rule_vtbl).put_grouping)(rule, group_bstr);
        free_bstr(group_bstr);

        // 绑定接口
        let interface_bstr = bstr_from_str(&self.actual_name);
        let interface_array = SafeArrayCreateVector(8, 0, 1); // VT_BSTR = 8
        if !interface_array.is_null() {
            let index = 0i32;
            SafeArrayPutElement(interface_array, &index as *const _, interface_bstr as *const _ as *const _);
            
            let mut variant: VARIANT = mem::zeroed();
            variant.vt = 0x2008; // VT_ARRAY | VT_BSTR
            variant.data = interface_array as usize;
            
            let _ = ((*rule_vtbl).put_interfaces)(rule, &variant as *const _);
            
            SafeArrayDestroy(interface_array);
        }
        free_bstr(interface_bstr);

        let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
        let hr = ((*rules_vtbl).add)(rules, rule);

        ((*rule_vtbl).release)(rule);

        if hr < 0 {
            return Err(format!("添加接口规则失败 {}: 0x{:X}", rule_name, hr));
        }

        log::info!("已添加接口规则: {}", rule_name);
        Ok(())
    }

    fn get_all_rule_names(&self) -> Vec<String> {
        vec![
            "VNT Virtual Network - UDP".to_string(),
            "VNT Virtual Network - TCP".to_string(),
            format!("VNT-Interface-{} (Inbound)", self.device_name),
            format!("VNT-Interface-{} (Outbound)", self.device_name),
        ]
    }
}
