//! Launch with Windows: a value in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! running the exe with `--tray` (window hidden). Off by default. The registry key is a
//! parameter so tests never touch the real one.

use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const RUN_VALUE: &str = "Freejazz";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The command registered for `exe`.
pub fn command_for(exe: &Path) -> String {
    format!("\"{}\" --tray", exe.display())
}

/// The command stored under `key`, if any.
pub fn registered_at(key: &str) -> Option<String> {
    let (key, value) = (wide(key), wide(RUN_VALUE));
    let mut buf = vec![0u16; 2048];
    let mut size = (buf.len() * 2) as u32;
    // SAFETY: NUL-terminated names; buffer and size agree.
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// Registers (`Some(command)`) or removes (`None`) the value under `key`.
pub fn set_at(key: &str, command: Option<&str>) -> Result<(), String> {
    let (key, value) = (wide(key), wide(RUN_VALUE));
    // SAFETY: NUL-terminated names; REG_SZ data carries its final NUL.
    let err = unsafe {
        match command {
            Some(cmd) => {
                let data = wide(cmd);
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    value.as_ptr(),
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            }
            None => match RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr()) {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                e => e,
            },
        }
    };
    if err == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("Registre Windows : erreur {err}"))
    }
}

/// Turns the launch with Windows on or off in the real `Run` key.
pub fn set(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        set_at(RUN_KEY, Some(&command_for(&exe)))
    } else {
        set_at(RUN_KEY, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_quotes_the_exe_and_hides_the_window() {
        let cmd = command_for(Path::new(r"C:\Program Files\Freejazz\freejazz.exe"));
        assert_eq!(cmd, r#""C:\Program Files\Freejazz\freejazz.exe" --tray"#);
    }

    #[test]
    fn the_run_value_roundtrips_in_a_scratch_key() {
        let key = r"Software\FreejazzTest\Run";
        assert_eq!(registered_at(key), None);
        set_at(key, Some(r#""C:\x\freejazz.exe" --tray"#)).unwrap();
        assert_eq!(
            registered_at(key).as_deref(),
            Some(r#""C:\x\freejazz.exe" --tray"#)
        );
        set_at(key, None).unwrap();
        assert_eq!(registered_at(key), None);
        set_at(key, None).unwrap(); // removing twice is fine
    }
}
