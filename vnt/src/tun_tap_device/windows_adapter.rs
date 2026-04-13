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
            self.uninstall_device()
        }
    }

    unsafe fn uninstall_device(&self) -> io::Result<bool> {
        let class_guid = GUID { 
            data1: 0x4d36e972, data2: 0xe325, data3: 0x11ce, 
            data4: [0xbf, 0xc1, 0x08, 0x00, 0x2b, 0xe1, 0x03, 0x18] 
        };
        
        let dev_info = SetupDiGetClassDevsW(&class_guid, ptr::null(), ptr::null_mut(), DIGCF_PRESENT);
        if dev_info as isize == -1 {
            return Ok(false);
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
                
                // 获取 DeviceDesc
                if SetupDiGetDeviceRegistryPropertyW(
                    dev_info, &dev_info_data, SPDRP_DEVICEDESC,
                    &mut reg_type, buffer.as_mut_ptr() as *mut u8,
                    (buffer.len() * 2) as u32, ptr::null_mut()
                ) != 0 {
                    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
                    let desc = String::from_utf16_lossy(&buffer[..len]);
                    
                    // 调试：显示所有 wintun 设备
                    if desc.to_lowercase().contains("wintun") {
                        log::info!("检测到 Wintun 设备: {} ({})", name, desc);
                    }
                    
                    // 匹配逻辑
                    let name_lower = name.to_lowercase();
                    let device_lower = self.device_name.to_lowercase();
                    let name_matches = name_lower == device_lower || 
                                      name_lower.starts_with(&format!("{} (", device_lower)) ||
                                      name_lower.starts_with(&format!("{} ", device_lower));
                    
                    if desc.to_lowercase().contains("wintun") && name_matches {
                        log::info!("正在删除网卡: {} ({})", name, desc);
                        
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
                                log::info!("网卡删除成功: {}", name);
                                removed = true;
                            } else {
                                log::warn!("网卡删除失败: {}", name);
                            }
                        }
                    }
                }
            }
            index += 1;
        }
        
        SetupDiDestroyDeviceInfoList(dev_info);
        Ok(removed)
    }
}
