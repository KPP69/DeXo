//! Native fatal-startup-error feedback.
//!
//! The release build uses the Windows GUI subsystem, so there is no console
//! to print to. Failures that happen before the main window exists (for
//! example an unwritable data directory) are shown in a native dialog that
//! names the failed stage and never contains secrets.

/// Shows a fatal startup error to the user and identifies the failed stage.
#[cfg(target_os = "windows")]
pub fn show_fatal_error(stage: &str, detail: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

    let text: Vec<u16> = OsStr::new(&format!("DeXo 启动失败（{stage}）\n\n{detail}"))
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let caption: Vec<u16> = OsStr::new("DeXo")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

/// On non-Windows platforms the console is the available channel.
#[cfg(not(target_os = "windows"))]
pub fn show_fatal_error(stage: &str, detail: &str) {
    eprintln!("DeXo startup failed ({stage}): {detail}");
}
