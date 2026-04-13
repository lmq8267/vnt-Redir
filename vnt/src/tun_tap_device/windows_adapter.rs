// Windows 网卡管理
use std::io;
use std::ptr;
use std::mem;
use windows_sys::Win32::NetworkManagement::IpHelper::{GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH, GAA_FLAG_INCLUDE_PREFIX};
use windows_sys::Win32::Networking::WinSock::AF_UNSPEC;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::core::GUID;

pub struct WindowsAdapterManager {
    device_name: String,
}

impl WindowsAdapterManager {
    pub fn new(device_name: &str) -> Self {
        Self { device_name: device_name.to_string() }
    }

    /// 启动前检测并清理同名网卡
    pub fn check_and_cleanup(&self) -> io::Result<()> {
        log::info!("检查同名虚拟网卡: {}", self.device_name);
        
        match self.find_and_remove_adapter() {
            Ok(removed) => {
                if removed {
                    log::info!("已删除旧网卡，等待系统清理...");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
                Ok(())
            }
            Err(e) => {
                log::warn!("检查网卡失败: {:?}，继续创建", e);
                Ok(())
            }
        }
    }

    /// 停止时删除网卡
    pub fn remove_adapter(&self) -> io::Result<()> {
        log::info!("删除虚拟网卡: {}", self.device_name);
        
        match self.find_and_remove_adapter() {
            Ok(removed) => {
                if removed {
                    log::info!("虚拟网卡已删除");
                }
                Ok(())
            }
            Err(e) => {
                log::warn!("删除网卡失败: {:?}", e);
                Ok(())
            }
        }
    }

    fn find_and_remove_adapter(&self) -> io::Result<bool> {
        unsafe {
            let mut size = 15000u32;
            let mut buffer = vec![0u8; size as usize];
            
            let result = GetAdaptersAddresses(
                AF_UNSPEC as u32,
                GAA_FLAG_INCLUDE_PREFIX,
                ptr::null_mut(),
                buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            );
            
            if result != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::Other, 
                    format!("GetAdaptersAddresses 失败，错误码: {}", result)
                ));
            }

            let mut found = false;
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

                // 调试：显示所有 wintun 网卡
                if description.to_lowercase().contains("wintun") {
                    log::info!("检测到 Wintun 网卡: {} ({})", friendly_name, description);
                }

                // 匹配逻辑：
                // 1. 完全相同：vnt-tun
                // 2. 带括号后缀：vnt-tun (Wintun...)
                // 3. 带空格序号：vnt-tun 4, vnt-tun 5
                // 不匹配： vnt-tun4 或 vnt-tun1
                let name_lower = friendly_name.to_lowercase();
                let device_lower = self.device_name.to_lowercase();
                let name_matches = name_lower == device_lower || 
                                  name_lower.starts_with(&format!("{} (", device_lower)) ||
                                  name_lower.starts_with(&format!("{} ", device_lower));
                
                if description.to_lowercase().contains("wintun") && name_matches {
                    log::info!("发现已存在的虚拟网卡: {}", friendly_name);
                    found = true;
                    break;
                }
                current = adapter.Next;
            }

            if found {
                self.uninstall_device()?;
            }
            
            Ok(found)
        }
    }

    unsafe fn uninstall_device(&self) -> io::Result<()> {
        // 网络适配器类 GUID
        let class_guid = GUID { 
            data1: 0x4d36e972, data2: 0xe325, data3: 0x11ce, 
            data4: [0xbf, 0xc1, 0x08, 0x00, 0x2b, 0xe1, 0x03, 0x18] 
        };
        
        let dev_info = SetupDiGetClassDevsW(
            &class_guid, 
            ptr::null(), 
            ptr::null_mut(), 
            DIGCF_PRESENT
        );
        
        if dev_info as isize == -1 {
            return Err(io::Error::new(io::ErrorKind::Other, "无法获取虚拟网卡设备列表"));
        }

        let mut index = 0u32;
        let mut removed = false;
        
        loop {
            let mut dev_info_data: SP_DEVINFO_DATA = mem::zeroed();
            dev_info_data.cbSize = mem::size_of::<SP_DEVINFO_DATA>() as u32;
            
            if SetupDiEnumDeviceInfo(dev_info, index, &mut dev_info_data) == 0 {
                break;
            }
            
            let mut buffer = [0u16; 256];
            let mut reg_type = 0u32;
            
            // 获取 FriendlyName
            if SetupDiGetDeviceRegistryPropertyW(
                dev_info, &dev_info_data, SPDRP_FRIENDLYNAME,
                &mut reg_type, buffer.as_mut_ptr() as *mut u8,
                (buffer.len() * 2) as u32, ptr::null_mut()
            ) != 0 {
                let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
                let name = String::from_utf16_lossy(&buffer[..len]);
                
                // 匹配逻辑：
                // 1. 完全相同：vnt-tun
                // 2. 带括号后缀：vnt-tun (Wintun...)
                // 3. 带空格序号：vnt-tun 4, vnt-tun 5
                let name_lower = name.to_lowercase();
                let device_lower = self.device_name.to_lowercase();
                let name_matches = name_lower == device_lower || 
                                  name_lower.starts_with(&format!("{} (", device_lower)) ||
                                  name_lower.starts_with(&format!("{} ", device_lower));
                
                if name_matches {
                    // 获取 DeviceDesc 确认是 Wintun
                    if SetupDiGetDeviceRegistryPropertyW(
                        dev_info, &dev_info_data, SPDRP_DEVICEDESC,
                        &mut reg_type, buffer.as_mut_ptr() as *mut u8,
                        (buffer.len() * 2) as u32, ptr::null_mut()
                    ) != 0 {
                        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
                        let desc = String::from_utf16_lossy(&buffer[..len]);
                        
                        if desc.to_lowercase().contains("wintun") {
                            log::info!("正在删除虚拟网卡: {} ({})", name, desc);
                            
                            let mut params: SP_REMOVEDEVICE_PARAMS = mem::zeroed();
                            params.ClassInstallHeader.cbSize = mem::size_of::<SP_CLASSINSTALL_HEADER>() as u32;
                            params.ClassInstallHeader.InstallFunction = DIF_REMOVE;
                            params.Scope = DI_REMOVEDEVICE_GLOBAL;
                            
                            if SetupDiSetClassInstallParamsW(
                                dev_info, &dev_info_data,
                                &params.ClassInstallHeader as *const _ as *const SP_CLASSINSTALL_HEADER,
                                mem::size_of::<SP_REMOVEDEVICE_PARAMS>() as u32
                            ) != 0 {
                                if SetupDiCallClassInstaller(DIF_REMOVE, dev_info, &mut dev_info_data) != 0 {
                                    log::info!("虚拟网卡删除成功: {}", name);
                                    removed = true;
                                } else {
                                    let err = io::Error::last_os_error();
                                    log::warn!("虚拟网卡删除失败: {} (错误: {})", name, err);
                                }
                            } else {
                                let err = io::Error::last_os_error();
                                log::warn!("设置删除参数失败 (错误: {})", err);
                            }
                        }
                    }
                }
            }
            index += 1;
        }
        
        SetupDiDestroyDeviceInfoList(dev_info);
        
        if removed {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "未找到匹配的虚拟网卡"))
        }
    }
}
