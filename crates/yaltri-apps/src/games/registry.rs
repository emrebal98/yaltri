//! Tiny read-only Windows registry helpers.

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};

#[derive(Clone, Copy)]
pub enum Hive {
    LocalMachine,
    CurrentUser,
}

fn root(h: Hive) -> HKEY {
    match h {
        Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        Hive::CurrentUser => HKEY_CURRENT_USER,
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// A `REG_SZ` value.
pub fn string(hive: Hive, path: &str, value: &str) -> Option<String> {
    let (path, value) = (wide(path), wide(value));
    let mut len: u32 = 0;
    // SAFETY: size query, then a read into a buffer of that size.
    unsafe {
        if RegGetValueW(root(hive), path.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), std::ptr::null_mut(), &mut len)
            != ERROR_SUCCESS
        {
            return None;
        }
        let mut buf = vec![0u16; (len as usize).div_ceil(2)];
        if RegGetValueW(
            root(hive),
            path.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        ) != ERROR_SUCCESS
        {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

/// Names of the subkeys of `path`.
pub fn subkeys(hive: Hive, path: &str) -> Vec<String> {
    let path = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    let mut out = Vec::new();
    // SAFETY: the key is opened read-only and closed before returning.
    unsafe {
        if RegOpenKeyExW(root(hive), path.as_ptr(), 0, KEY_READ, &mut key) != ERROR_SUCCESS {
            return out;
        }
        let mut name = [0u16; 256];
        for i in 0.. {
            let mut len = name.len() as u32;
            let rc = RegEnumKeyExW(
                key,
                i,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if rc != ERROR_SUCCESS {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        RegCloseKey(key);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_well_known_values() {
        let pf = string(Hive::LocalMachine, r"SOFTWARE\Microsoft\Windows\CurrentVersion", "ProgramFilesDir");
        assert!(pf.is_some_and(|p| p.len() > 3));
        assert!(!subkeys(Hive::LocalMachine, r"SOFTWARE\Microsoft").is_empty());
        assert!(string(Hive::LocalMachine, r"SOFTWARE\NoSuchKeyYaltri", "x").is_none());
    }
}
