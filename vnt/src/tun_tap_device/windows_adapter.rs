// Windows 网卡管理

use std::io;
use std::ptr;
use std::mem;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::Registry::*;
use windows_sys::core::GUID;

pub struct WindowsAdapterManager {
    device_name: String,
}

impl WindowsAdapterManager {
    pub fn new(device_name: &str) -> Self {
        Self {
            device_name: device_name.to_string(),
        }
    }

    /// 启动前清理
    pub fn check_and_cleanup(&self) -> io::Result<()> {
        log::info!("检查并清理同名的虚拟网卡: {}", self.device_name);

        let removed = unsafe { self.uninstall_device()? };

        if removed {
            log::info!("已删除旧虚拟网卡，等待系统释放...");
            std::thread::sleep(std::time::Duration::from_secs(2));
        }

        Ok(())
    }

    /// 停止时删除
    pub fn remove_adapter(&self) -> io::Result<()> {
        log::info!("删除虚拟网卡: {}", self.device_name);

        unsafe {
            self.uninstall_device()?;
        }

        Ok(())
    }

    /// 核心：枚举 + 精准匹配 + 删除
    unsafe fn uninstall_device(&self) -> io::Result<bool> {
        // 网络适配器类 GUID
        let class_guid = GUID {
            data1: 0x4d36e972,
            data2: 0xe325,
            data3: 0x11ce,
            data4: [0xbf, 0xc1, 0x08, 0x00, 0x2b, 0xe1, 0x03, 0x18],
        };

        let dev_info = SetupDiGetClassDevsW(
            &class_guid,
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_PROFILE,
        );

        if dev_info as isize == -1 {
            return Err(io::Error::last_os_error());
        }

        let mut index = 0;
        let mut removed = false;

        loop {
            let mut dev_info_data: SP_DEVINFO_DATA = mem::zeroed();
            dev_info_data.cbSize = mem::size_of::<SP_DEVINFO_DATA>() as u32;

            if SetupDiEnumDeviceInfo(dev_info, index, &mut dev_info_data) == 0 {
                break;
            }

            // =========================
            // 获取设备描述（判断 Wintun）
            // =========================
            let mut desc_buf = [0u16; 256];
            let mut reg_type = 0u32;

            if SetupDiGetDeviceRegistryPropertyW(
                dev_info,
                &dev_info_data,
                SPDRP_DEVICEDESC,
                &mut reg_type,
                desc_buf.as_mut_ptr() as *mut u8,
                (desc_buf.len() * 2) as u32,
                ptr::null_mut(),
            ) == 0
            {
                index += 1;
                continue;
            }

            let desc_len = desc_buf.iter().position(|&c| c == 0).unwrap_or(desc_buf.len());
            let desc = String::from_utf16_lossy(&desc_buf[..desc_len]);

            log::info!("枚举到网卡设备: {}", desc);

            // 只处理 Wintun
            if !desc.to_lowercase().contains("wintun") {
                index += 1;
                continue;
            }

            // =========================
            // 获取真实网卡名（Connection\Name）
            // =========================
            let conn_name = match get_net_connection_id(dev_info, &dev_info_data) {
                Some(n) => n,
                None => {
                    log::warn!("无法获取 Wintun 设备的连接名称");
                    index += 1;
                    continue;
                }
            };

            log::info!("检测到 Wintun: {} ({})", conn_name, desc);

            let name_lower = conn_name.to_lowercase();
            let device_lower = self.device_name.to_lowercase();

            // =========================
            // 精准匹配规则
            // 1. 完全相同：vnt-tun 
            // 2. 带括号后缀：vnt-tun (Wintun...) 
            // 3. 带空格序号：vnt-tun 4, vnt-tun 5 
            // 不匹配： vnt-tun4 或 vnt-tun1
            // =========================
            let match_ok =
                name_lower == device_lower
                || name_lower.starts_with(&(device_lower.clone() + " "))
                || name_lower.starts_with(&(device_lower.clone() + " ("));

            log::info!("匹配检查: '{}' vs '{}' => {}", name_lower, device_lower, match_ok);

            if !match_ok {
                index += 1;
                continue;
            }

            log::info!("发现已存在的重名虚拟网卡，准备删除: {}", conn_name);

            // =========================
            //  删除设备
            // =========================
            let mut params: SP_REMOVEDEVICE_PARAMS = mem::zeroed();
            params.ClassInstallHeader.cbSize =
                mem::size_of::<SP_CLASSINSTALL_HEADER>() as u32;
            params.ClassInstallHeader.InstallFunction = DIF_REMOVE;
            params.Scope = DI_REMOVEDEVICE_GLOBAL;

            if SetupDiSetClassInstallParamsW(
                dev_info,
                &dev_info_data,
                &params.ClassInstallHeader as *const _ as *const SP_CLASSINSTALL_HEADER,
                mem::size_of::<SP_REMOVEDEVICE_PARAMS>() as u32,
            ) != 0
            {
                if SetupDiCallClassInstaller(DIF_REMOVE, dev_info, &mut dev_info_data) != 0 {
                    log::info!("删除虚拟网卡成功: {}", conn_name);
                    removed = true;
                } else {
                    let err = io::Error::last_os_error();
                    log::warn!("删除虚拟网卡失败: {} 错误: {:?}", conn_name, err);
                }
            } else {
                let err = io::Error::last_os_error();
                log::warn!("设置删除虚拟网卡参数失败: {:?}", err);
            }

            index += 1;
        }

        SetupDiDestroyDeviceInfoList(dev_info);

        Ok(removed)
    }
}

/// 获取实际网卡名（创建后可能带序号）
pub fn get_actual_adapter_name(device_name: &str) -> io::Result<String> {
    unsafe {
        let class_guid = GUID {
            data1: 0x4d36e972,
            data2: 0xe325,
            data3: 0x11ce,
            data4: [0xbf, 0xc1, 0x08, 0x00, 0x2b, 0xe1, 0x03, 0x18],
        };

        let dev_info = SetupDiGetClassDevsW(
            &class_guid,
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_PROFILE,
        );

        if dev_info as isize == -1 {
            return Err(io::Error::last_os_error());
        }

        let mut index = 0;
        let mut result = None;

        loop {
            let mut dev_info_data: SP_DEVINFO_DATA = mem::zeroed();
            dev_info_data.cbSize = mem::size_of::<SP_DEVINFO_DATA>() as u32;

            if SetupDiEnumDeviceInfo(dev_info, index, &mut dev_info_data) == 0 {
                break;
            }

            let mut desc_buf = [0u16; 256];
            let mut reg_type = 0u32;

            if SetupDiGetDeviceRegistryPropertyW(
                dev_info,
                &dev_info_data,
                SPDRP_DEVICEDESC,
                &mut reg_type,
                desc_buf.as_mut_ptr() as *mut u8,
                (desc_buf.len() * 2) as u32,
                ptr::null_mut(),
            ) == 0
            {
                index += 1;
                continue;
            }

            let desc_len = desc_buf.iter().position(|&c| c == 0).unwrap_or(desc_buf.len());
            let desc = String::from_utf16_lossy(&desc_buf[..desc_len]);

            if !desc.to_lowercase().contains("wintun") {
                index += 1;
                continue;
            }

            let conn_name = match get_net_connection_id(dev_info, &dev_info_data) {
                Some(n) => n,
                None => {
                    index += 1;
                    continue;
                }
            };

            let name_lower = conn_name.to_lowercase();
            let device_lower = device_name.to_lowercase();

            let match_ok = name_lower == device_lower
                || name_lower.starts_with(&(device_lower.clone() + " "))
                || name_lower.starts_with(&(device_lower.clone() + " ("));

            if match_ok {
                result = Some(conn_name);
                break;
            }

            index += 1;
        }

        SetupDiDestroyDeviceInfoList(dev_info);

        result.ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "未找到匹配的网卡"))
    }
}

// =========================
// 获取 NetConnectionID
// =========================
unsafe fn get_net_connection_id(
    dev_info: HDEVINFO,
    dev_info_data: &SP_DEVINFO_DATA,
) -> Option<String> {
    let hkey = SetupDiOpenDevRegKey(
        dev_info,
        dev_info_data,
        DICS_FLAG_GLOBAL,
        0,
        DIREG_DRV,
        KEY_READ,
    );

    if hkey.is_null() || hkey == INVALID_HANDLE_VALUE {
        return None;
    }

    let mut conn_key: HKEY = ptr::null_mut();

    let subkey: Vec<u16> = "Connection\0".encode_utf16().collect();

    if RegOpenKeyExW(hkey, subkey.as_ptr(), 0, KEY_READ, &mut conn_key) != 0 {
        RegCloseKey(hkey);
        return None;
    }

    let mut buf = [0u16; 256];
    let mut buf_len = (buf.len() * 2) as u32;

    let value_name: Vec<u16> = "Name\0".encode_utf16().collect();

    let result = RegQueryValueExW(
        conn_key,
        value_name.as_ptr(),
        ptr::null_mut(),
        ptr::null_mut(),
        buf.as_mut_ptr() as *mut u8,
        &mut buf_len,
    );

    RegCloseKey(conn_key);
    RegCloseKey(hkey);

    if result == 0 {
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    } else {
        None
    }
}
