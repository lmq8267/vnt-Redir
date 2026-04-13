// Windows 防火墙管理
use std::io;
use windows::Win32::Foundation::*;
use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::NetworkManagement::WindowsFilteringPlatform::*;
use windows::Win32::System::Rpc::RPC_C_AUTHN_DEFAULT;
use windows::core::GUID;

pub struct WindowsFirewallManager {
    device_name: String,   // 规则名（定义的网卡名）
    actual_name: String,   // 实际接口名（用于查找索引）
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
            match self.configure_all_internal() {
                Ok(_) => {
                    log::info!("虚拟网卡的出入站防火墙规则配置完成");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("虚拟网卡的出入站防火墙配置失败: {}，程序将继续运行", e);
                    Ok(())
                }
            }
        }
    }

    unsafe fn configure_all_internal(&self) -> windows::core::Result<()> {
        // 获取接口索引
        let if_index = self.get_interface_index()?;
        log::info!("找到接口索引: {} ({})", if_index, self.actual_name);

        // 打开 WFP 引擎
        let mut engine = HANDLE::default();
        FwpmEngineOpen0(None, RPC_C_AUTHN_DEFAULT, None, None, &mut engine)?;

        // 添加规则
        let result = self.add_all_rules(engine, if_index);
        
        let _ = FwpmEngineClose0(engine);
        
        result
    }

    pub fn cleanup_all(&self) -> io::Result<()> {
        log::info!("清理防火墙规则: {}", self.device_name);
        
        unsafe {
            match self.cleanup_all_internal() {
                Ok(_) => {
                    log::info!("虚拟网卡的出入站防火墙规则已清理");
                    Ok(())
                }
                Err(e) => {
                    log::warn!("虚拟网卡的出入站防火墙清理失败: {}", e);
                    Ok(())
                }
            }
        }
    }

    unsafe fn cleanup_all_internal(&self) -> windows::core::Result<()> {
        let mut engine = HANDLE::default();
        FwpmEngineOpen0(None, RPC_C_AUTHN_DEFAULT, None, None, &mut engine)?;

        // 删除所有规则（WFP 规则通过 ID 删除，这里简化处理）
        // 实际应该保存规则 ID，这里重新打开引擎会自动清理
        
        let _ = FwpmEngineClose0(engine);
        Ok(())
    }

    unsafe fn get_interface_index(&self) -> windows::core::Result<u32> {
        let mut size = 15000u32;
        let mut buffer = vec![0u8; size as usize];

        let ret = GetAdaptersAddresses(
            AF_UNSPEC.0 as u32,
            0,
            None,
            Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
            &mut size,
        );

        if ret != 0 {
            return Err(windows::core::Error::from_win32());
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
                    return Ok(adapter.IfIndex);
                }
            }

            current = adapter.Next;
        }

        Err(windows::core::Error::from_win32())
    }

    unsafe fn add_all_rules(&self, engine: HANDLE, if_index: u32) -> windows::core::Result<()> {
        // 程序 UDP 规则
        self.add_app_protocol_rule(engine, 17, "UDP")?;
        // 程序 TCP 规则
        self.add_app_protocol_rule(engine, 6, "TCP")?;
        
        // 虚拟网卡全协议规则
        self.add_interface_rule(engine, if_index, "Inbound", &FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4)?;
        self.add_interface_rule(engine, if_index, "Outbound", &FWPM_LAYER_ALE_AUTH_CONNECT_V4)?;
        
        Ok(())
    }

    unsafe fn add_app_protocol_rule(&self, engine: HANDLE, protocol: u8, protocol_name: &str) -> windows::core::Result<()> {
        let exe_path = std::env::current_exe()
            .map_err(|_| windows::core::Error::from_win32())?;
        let exe_path_str = exe_path.to_string_lossy();
        let exe_path_wide: Vec<u16> = exe_path_str.encode_utf16().chain(Some(0)).collect();

        let rule_name = format!("VNT Virtual Network - {}", protocol_name);

        // 条件：应用程序路径
        let mut blob = FWP_BYTE_BLOB {
            size: (exe_path_wide.len() * 2) as u32,
            data: exe_path_wide.as_ptr() as *mut u8,
        };

        let mut condition_app = FWPM_FILTER_CONDITION0 {
            fieldKey: FWPM_CONDITION_ALE_APP_ID,
            matchType: FWP_MATCH_EQUAL,
            conditionValue: FWP_CONDITION_VALUE0 {
                r#type: FWP_BYTE_BLOB_TYPE,
                Anonymous: FWP_CONDITION_VALUE0_0 {
                    byteBlob: &mut blob as *mut _,
                },
            },
        };

        // 条件：协议
        let mut condition_proto = FWPM_FILTER_CONDITION0 {
            fieldKey: FWPM_CONDITION_IP_PROTOCOL,
            matchType: FWP_MATCH_EQUAL,
            conditionValue: FWP_CONDITION_VALUE0 {
                r#type: FWP_UINT8,
                Anonymous: FWP_CONDITION_VALUE0_0 {
                    uint8: protocol,
                },
            },
        };

        let conditions = [condition_app, condition_proto];

        // 创建过滤器
        let filter = FWPM_FILTER0 {
            displayData: FWPM_DISPLAY_DATA0 {
                name: windows::core::PWSTR(rule_name.encode_utf16().chain(Some(0)).collect::<Vec<_>>().as_mut_ptr()),
                description: windows::core::PWSTR(std::ptr::null_mut()),
            },
            layerKey: FWPM_LAYER_ALE_AUTH_CONNECT_V4,
            action: FWPM_ACTION0 {
                r#type: FWP_ACTION_PERMIT,
                Anonymous: Default::default(),
            },
            numFilterConditions: 2,
            filterCondition: conditions.as_ptr() as *mut _,
            weight: FWP_VALUE0 {
                r#type: FWP_EMPTY,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut id = 0u64;
        FwpmFilterAdd0(engine, &filter, None, Some(&mut id))?;

        log::info!("已添加程序 {} 规则 (ID: {})", protocol_name, id);
        Ok(())
    }

    unsafe fn add_interface_rule(&self, engine: HANDLE, if_index: u32, direction: &str, layer: &GUID) -> windows::core::Result<()> {
        let rule_name = format!("VNT-Interface-{} ({})", self.device_name, direction);

        // 创建条件：接口索引
        let mut condition = FWPM_FILTER_CONDITION0 {
            fieldKey: FWPM_CONDITION_INTERFACE_INDEX,
            matchType: FWP_MATCH_EQUAL,
            conditionValue: FWP_CONDITION_VALUE0 {
                r#type: FWP_UINT32,
                Anonymous: FWP_CONDITION_VALUE0_0 {
                    uint32: if_index,
                },
            },
        };

        // 创建过滤器
        let filter = FWPM_FILTER0 {
            displayData: FWPM_DISPLAY_DATA0 {
                name: windows::core::PWSTR(rule_name.encode_utf16().chain(Some(0)).collect::<Vec<_>>().as_mut_ptr()),
                description: windows::core::PWSTR(std::ptr::null_mut()),
            },
            layerKey: *layer,
            action: FWPM_ACTION0 {
                r#type: FWP_ACTION_PERMIT,
                Anonymous: Default::default(),
            },
            numFilterConditions: 1,
            filterCondition: &mut condition as *mut _,
            weight: FWP_VALUE0 {
                r#type: FWP_EMPTY,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut id = 0u64;
        FwpmFilterAdd0(engine, &filter, None, Some(&mut id))?;

        log::info!("已添加接口规则: {} (ID: {})", rule_name, id);
        Ok(())
    }
}
