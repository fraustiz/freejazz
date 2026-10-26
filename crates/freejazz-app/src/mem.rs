//! Memory: gives the working set back to Windows (the pages stay mapped and return on
//! demand). Used when the window is hidden and after a long idle.

/// Trims the working set of this process.
pub fn trim_working_set() {
    // SAFETY: documented call on the current-process pseudo handle; (-1, -1) means
    // « empty the working set ».
    unsafe {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}
