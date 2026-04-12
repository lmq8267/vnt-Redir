// Windows 网卡管理 - 纯 API 实现
use std::io;
use std::ptr;
use std::mem;
use windows_sys::Win32::NetworkManagement::IpHelper::*;
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

    pub fn cleanup_before_start(&self) -> io::Result<()> {
        log::info!("检查是否存在同名虚拟网卡: {}", self.device_name);
        
        match self.check_and_remove_adapter() {
            Ok(removed) => {
                if removed {
                    log::info!("已卸载旧的虚拟网卡");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                } else {
                    log::info!("未发现同名虚拟网卡，可以安全创建");
                }
            }
            Err(e) => {
                log::warn!("检查网卡时出错: {:?}，继续创建", e);
            }
        }
        Ok(())
    }

    fn check_and_remove_adapter(&self) -> io::Result<bool> {
        unsafe {
            let mut size = 15000u32;
            let mut buffer = vec![0u8; size as usize];
            
            if GetAdaptersAddresses(
                AF_UNSPEC as u32,
                GAA_FLAG_INCLUDE_PREFIX,
                ptr::null_mut(),
                buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH,
                &mut size,
            ) != 0 {
                return Ok(false);
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

                if description.to_lowercase().contains("wintun") && 
                   friendly_name.to_lowercase().contains(&self.device_name.to_lowercase()) {
                    log::warn!("发现已存在的虚拟网卡: {} ({})", friendly_name, description);
                    found = true;
                    break;
                }
                current = adapter.Next;
            }

            if found {
                self.remove_adapter_by_setupdi()?;
            }
            
            Ok(found)
        }
    }

    unsafe fn remove_adapter_by_setupdi(&self) -> io::Result<()> {
        let class_guid = GUID { 
            data1: 0x4d36e972, data2: 0xe325, data3: 0x11ce, 
            data4: [0xbf, 0xc1, 0x08, 0x00, 0x2b, 0xe1, 0x03, 0x18] 
        };
        
        let dev_info = SetupDiGetClassDevsW(&class_guid, ptr::null(), ptr::null_mut(), DIGCF_PRESENT);
        if dev_info as isize == INVALID_HANDLE_VALUE {
            return Err(io::Error::new(io::ErrorKind::Other, "无法获取设备列表"));
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
                
                if name.to_lowercase().contains(&self.device_name.to_lowercase()) {
                    // 获取 DeviceDesc 确认是 Wintun
                    if SetupDiGetDeviceRegistryPropertyW(
                        dev_info, &dev_info_data, SPDRP_DEVICEDESC,
                        &mut reg_type, buffer.as_mut_ptr() as *mut u8,
                        (buffer.len() * 2) as u32, ptr::null_mut()
                    ) != 0 {
                        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
                        let desc = String::from_utf16_lossy(&buffer[..len]);
                        
                        if desc.to_lowercase().contains("wintun") {
                            log::info!("正在删除网卡: {}", name);
                            
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
                                    log::info!("成功删除网卡: {}", name);
                                    removed = true;
                                } else {
                                    log::warn!("删除网卡失败: {}", name);
                                }
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
            Err(io::Error::new(io::ErrorKind::NotFound, "未找到匹配的网卡"))
        }
    }
}
