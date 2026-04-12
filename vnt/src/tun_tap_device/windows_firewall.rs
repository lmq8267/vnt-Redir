// Windows 防火墙管理 - 完整 COM API 实现
use std::io;
use std::ptr;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::System::Com::*;
use windows_sys::Win32::NetworkManagement::IpHelper::*;
use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;
use windows_sys::core::*;

const CLSID_NETFWPOLICY2: GUID = GUID { data1: 0xE2B3C97F, data2: 0x6AE1, data3: 0x41AC, data4: [0x81, 0x7A, 0xF6, 0xF9, 0x21, 0x66, 0xD7, 0xDD] };
const IID_INETFWPOLICY2: GUID = GUID { data1: 0x98325047, data2: 0xC671, data3: 0x4174, data4: [0x8D, 0x81, 0xDE, 0xFC, 0xD3, 0xF0, 0x31, 0x86] };
const CLSID_NETFWRULE: GUID = GUID { data1: 0x2C5BC43E, data2: 0x3369, data3: 0x4C33, data4: [0xAB, 0x0C, 0xBE, 0x94, 0x69, 0x67, 0x7A, 0xF4] };
const IID_INETFWRULE: GUID = GUID { data1: 0xAF230D27, data2: 0xBABA, data3: 0x4E42, data4: [0xAC, 0xED, 0xF5, 0x24, 0xF2, 0x2C, 0xFC, 0xE2] };

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
    item: usize, get__new_enum: usize,
}

#[repr(C)]
struct INetFwRuleVtbl {
    query_interface: usize, add_ref: usize, release: unsafe extern "system" fn(*mut IUnknown) -> u32,
    get_type_info_count: usize, get_type_info: usize, get_ids_of_names: usize, invoke: usize,
    get_name: usize, put_name: unsafe extern "system" fn(*mut IUnknown, BSTR) -> i32,
    get_description: usize, put_description: usize, get_application_name: usize,
    put_application_name: unsafe extern "system" fn(*mut IUnknown, BSTR) -> i32,
    get_service_name: usize, put_service_name: usize,
    get_protocol: usize, put_protocol: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
    get_local_ports: usize, put_local_ports: usize, get_remote_ports: usize, put_remote_ports: usize,
    get_local_addresses: usize, put_local_addresses: usize, get_remote_addresses: usize, put_remote_addresses: usize,
    get_icmp_types_and_codes: usize, put_icmp_types_and_codes: usize,
    get_direction: usize, put_direction: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
    get_interfaces: usize, put_interfaces: usize, get_interface_types: usize, put_interface_types: usize,
    get_enabled: usize, put_enabled: unsafe extern "system" fn(*mut IUnknown, i16) -> i32,
    get_grouping: usize, put_grouping: usize,
    get_profiles: usize, put_profiles: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
    get_edge_traversal: usize, put_edge_traversal: usize,
    get_action: usize, put_action: unsafe extern "system" fn(*mut IUnknown, i32) -> i32,
}

pub struct WindowsFirewallManager {
    device_name: String,
    app_rule_name: String,
    interface_rule_in: String,
    interface_rule_out: String,
}

impl WindowsFirewallManager {
    pub fn new(device_name: &str) -> Self {
        Self {
            device_name: device_name.to_string(),
            app_rule_name: "VNT Virtual Network".to_string(),
            interface_rule_in: format!("VNT-Interface-{}-In", device_name),
            interface_rule_out: format!("VNT-Interface-{}-Out", device_name),
        }
    }

    pub fn configure_all(&self) -> io::Result<()> {
        log::info!("正在配置 Windows 防火墙规则...");
        
        unsafe {
            if CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED) < 0 {
                log::warn!("COM 初始化失败，跳过防火墙配置");
                return Ok(());
            }

            let _ = self.remove_existing_rules();
            let _ = self.add_app_rules();
            
            if self.wait_for_adapter(5000) {
                let _ = self.add_interface_rules();
            } else {
                log::warn!("虚拟网卡未就绪，跳过网卡防火墙规则");
            }
            
            CoUninitialize();
        }
        
        log::info!("Windows 防火墙配置完成");
        Ok(())
    }

    pub fn cleanup_all(&self) -> io::Result<()> {
        log::info!("正在清理 Windows 防火墙规则...");
        unsafe {
            if CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED) >= 0 {
                let _ = self.remove_existing_rules();
                CoUninitialize();
            }
        }
        Ok(())
    }

    fn wait_for_adapter(&self, timeout_ms: u64) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed().as_millis() < timeout_ms as u128 {
            if self.check_adapter_ready() {
                log::info!("虚拟网卡 {} 已就绪", self.device_name);
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        false
    }

    fn check_adapter_ready(&self) -> bool {
        unsafe {
            let mut size = 15000u32;
            let mut buffer = vec![0u8; size as usize];
            
            if GetAdaptersAddresses(AF_UNSPEC as u32, GAA_FLAG_INCLUDE_PREFIX, ptr::null_mut(),
                buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH, &mut size) != 0 {
                return false;
            }

            let mut current = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
            while !current.is_null() {
                let adapter = &*current;
                
                let friendly_name = if !adapter.FriendlyName.is_null() {
                    let len = (0..).take_while(|&i| *adapter.FriendlyName.offset(i) != 0).count();
                    String::from_utf16_lossy(std::slice::from_raw_parts(adapter.FriendlyName, len))
                } else { String::new() };
                
                let description = if !adapter.Description.is_null() {
                    let len = (0..).take_while(|&i| *adapter.Description.offset(i) != 0).count();
                    String::from_utf16_lossy(std::slice::from_raw_parts(adapter.Description, len))
                } else { String::new() };

                if description.to_lowercase().contains("wintun") && 
                   friendly_name.to_lowercase().contains(&self.device_name.to_lowercase()) {
                    return true;
                }
                current = adapter.Next;
            }
            false
        }
    }

    unsafe fn remove_existing_rules(&self) -> io::Result<()> {
        let mut policy: *mut IUnknown = ptr::null_mut();
        if CoCreateInstance(&CLSID_NETFWPOLICY2, ptr::null_mut(), CLSCTX_INPROC_SERVER,
            &IID_INETFWPOLICY2, &mut policy as *mut *mut IUnknown as *mut *mut _) < 0 {
            return Ok(());
        }

        let mut rules: *mut IUnknown = ptr::null_mut();
        let vtbl = *(policy as *const *const INetFwPolicy2Vtbl);
        if ((*vtbl).get_rules)(policy, &mut rules) >= 0 && !rules.is_null() {
            let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
            
            for name in &[&self.app_rule_name, &format!("{} Out", self.app_rule_name), 
                          &self.interface_rule_in, &self.interface_rule_out] {
                let name_wide: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
                let bstr = SysAllocString(name_wide.as_ptr());
                let _ = ((*rules_vtbl).remove)(rules, bstr);
                SysFreeString(bstr);
            }
            ((*rules_vtbl).release)(rules);
        }
        ((*vtbl).release)(policy);
        Ok(())
    }

    unsafe fn add_app_rules(&self) -> io::Result<()> {
        let exe_path = std::env::current_exe()?;
        log::info!("添加应用程序防火墙规则");
        
        self.add_rule(&self.app_rule_name, &exe_path.to_string_lossy(), 17, 1, true)?;
        self.add_rule(&format!("{} Out", self.app_rule_name), &exe_path.to_string_lossy(), 17, 2, false)?;
        Ok(())
    }

    unsafe fn add_interface_rules(&self) -> io::Result<()> {
        log::info!("添加虚拟网卡防火墙规则");
        self.add_rule(&self.interface_rule_in, "", 256, 1, false)?;
        self.add_rule(&self.interface_rule_out, "", 256, 2, false)?;
        Ok(())
    }

    unsafe fn add_rule(&self, name: &str, app_path: &str, protocol: i32, direction: i32, is_app: bool) -> io::Result<()> {
        let mut policy: *mut IUnknown = ptr::null_mut();
        if CoCreateInstance(&CLSID_NETFWPOLICY2, ptr::null_mut(), CLSCTX_INPROC_SERVER,
            &IID_INETFWPOLICY2, &mut policy as *mut *mut IUnknown as *mut *mut _) < 0 {
            return Err(io::Error::new(io::ErrorKind::Other, "无法创建策略对象"));
        }

        let mut rule: *mut IUnknown = ptr::null_mut();
        if CoCreateInstance(&CLSID_NETFWRULE, ptr::null_mut(), CLSCTX_INPROC_SERVER,
            &IID_INETFWRULE, &mut rule as *mut *mut IUnknown as *mut *mut _) < 0 {
            (*(*(policy as *const *const INetFwPolicy2Vtbl)).release)(policy);
            return Err(io::Error::new(io::ErrorKind::Other, "无法创建规则对象"));
        }

        let rule_vtbl = *(rule as *const *const INetFwRuleVtbl);
        
        let name_wide: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
        let name_bstr = SysAllocString(name_wide.as_ptr());
        ((*rule_vtbl).put_name)(rule, name_bstr);
        SysFreeString(name_bstr);

        if is_app && !app_path.is_empty() {
            let path_wide: Vec<u16> = OsStr::new(app_path).encode_wide().chain(Some(0)).collect();
            let path_bstr = SysAllocString(path_wide.as_ptr());
            ((*rule_vtbl).put_application_name)(rule, path_bstr);
            SysFreeString(path_bstr);
        }

        ((*rule_vtbl).put_protocol)(rule, protocol);
        ((*rule_vtbl).put_direction)(rule, direction);
        ((*rule_vtbl).put_action)(rule, 1);
        ((*rule_vtbl).put_enabled)(rule, -1);
        ((*rule_vtbl).put_profiles)(rule, 0x7FFFFFFF);

        let mut rules: *mut IUnknown = ptr::null_mut();
        let policy_vtbl = *(policy as *const *const INetFwPolicy2Vtbl);
        if ((*policy_vtbl).get_rules)(policy, &mut rules) >= 0 && !rules.is_null() {
            let rules_vtbl = *(rules as *const *const INetFwRulesVtbl);
            let result = ((*rules_vtbl).add)(rules, rule);
            ((*rules_vtbl).release)(rules);
            
            if result >= 0 {
                log::info!("已添加防火墙规则: {}", name);
            }
        }

        ((*rule_vtbl).release)(rule);
        ((*policy_vtbl).release)(policy);
        Ok(())
    }
}
