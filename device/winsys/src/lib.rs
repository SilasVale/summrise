//! The Windows calls the safe crates cannot make.
//!
//! The device app and the launcher are `#![forbid(unsafe_code)]`. That is the right rule for the
//! code that decides what a grant permits, but two questions a device must answer — "what is running
//! on this machine" and "end that process" — have no safe Rust answer without a dependency this
//! project does not want to carry. This crate is the small unsafe surface: two functions, each a
//! short Win32 sequence, with the error mapped to `io::Error` so the caller never sees a `BOOL`.
//!
//! Memory comes from `K32GetProcessMemoryInfo`'s working-set figure. A process this crate cannot
//! open (a protected process, or one that exited between the snapshot and the open) reports `0`
//! rather than failing the list: "I could not read its memory" and "it uses none" are different
//! facts, but a list that refuses to render because one process is protected is useless.

#![cfg(windows)]
#![allow(unsafe_code)]

use std::io;
use std::mem::{size_of, zeroed};

use windows_sys::Win32::Foundation::{CloseHandle, FALSE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{
    OpenProcess, TerminateProcess, PROCESS_QUERY_INFORMATION, PROCESS_TERMINATE, PROCESS_VM_READ,
};

/// One running process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Process {
    /// The process id.
    pub pid: u32,
    /// The executable's file name, e.g. `summrise-device.exe`.
    pub name: String,
    /// The working set in KiB, or `0` when the process could not be opened.
    pub memory_kb: u64,
}

/// Every process the caller can see, from one snapshot.
///
/// The list is the operating system's, including the processes this device did not start. A
/// caller that only wants its own filters them; hiding the rest would make the tool answer a
/// different question than the one an operator asks when a machine is slow.
pub fn processes() -> io::Result<Vec<Process>> {
    // SAFETY: the snapshot handle is checked against `INVALID_HANDLE_VALUE` before use and closed
    // on every return path below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }

    let mut entry: PROCESSENTRY32W = unsafe { zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;

    let mut found = Vec::new();
    // SAFETY: `entry` has its `dwSize` set, which is the contract Process32FirstW checks.
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) };
    while more != 0 {
        found.push(Process {
            pid: entry.th32ProcessID,
            name: name_of(&entry.szExeFile),
            memory_kb: working_set_kb(entry.th32ProcessID),
        });
        // SAFETY: same entry, filled by the previous call.
        more = unsafe { Process32NextW(snapshot, &mut entry) };
    }

    // SAFETY: the handle came from CreateToolhelp32Snapshot and is not used afterwards.
    unsafe { CloseHandle(snapshot) };
    Ok(found)
}

/// End one process. `Ok` means the operating system accepted the termination.
pub fn terminate(pid: u32) -> io::Result<()> {
    // SAFETY: the handle is checked and closed; the call takes no pointers.
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, FALSE, pid) };
    if handle == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a live process handle with PROCESS_TERMINATE.
    let accepted = unsafe { TerminateProcess(handle, 1) };
    let result = if accepted == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    };
    // SAFETY: the handle is not used afterwards.
    unsafe { CloseHandle(handle) };
    result
}

/// The executable name from a NUL-terminated UTF-16 buffer.
fn name_of(raw: &[u16]) -> String {
    let end = raw.iter().position(|unit| *unit == 0).unwrap_or(raw.len());
    String::from_utf16_lossy(&raw[..end])
}

/// The working set in KiB, or `0` when the process cannot be opened.
fn working_set_kb(pid: u32) -> u64 {
    // SAFETY: the handle is checked and closed; the counters struct is zeroed and its `cb` set.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, pid) };
    if handle == 0 {
        return 0;
    }

    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: the struct is the size its `cb` declares and the handle has query rights.
    let read = unsafe { K32GetProcessMemoryInfo(handle, &mut counters, counters.cb) };
    // SAFETY: the handle is not used afterwards.
    unsafe { CloseHandle(handle) };

    if read == 0 {
        0
    } else {
        counters.WorkingSetSize as u64 / 1024
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn the_list_names_this_test_process() {
        let processes = processes().expect("a snapshot is available to a normal process");
        let me = std::process::id();
        let found = processes
            .iter()
            .find(|process| process.pid == me)
            .expect("the test's own process is in its own snapshot");
        assert!(!found.name.is_empty());
    }

    #[test]
    fn a_spawned_process_can_be_terminated_by_pid() {
        // `ping` is a console program that sleeps between replies, which makes it a process that is
        // alive long enough to be ended on purpose rather than one that may have exited already.
        let mut child = Command::new("cmd")
            .args(["/C", "ping", "-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the child starts");
        let pid = child.id();
        assert!(
            processes()
                .expect("a snapshot")
                .iter()
                .any(|process| process.pid == pid),
            "the child is listed before it is killed"
        );

        terminate(pid).expect("the child can be terminated");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match child.try_wait().expect("the child's status is readable") {
                Some(_) => break,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    panic!("the terminated child did not exit");
                }
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        }
    }
}

/// One frame of the interactive desktop, top-down RGBA.
pub struct Screen {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The name of the desktop the user is looking at: `Default` when the session is unlocked, `Winlogon`
/// (or `Screen-saver`) when it is locked.
///
/// A capture of a locked session is not a picture of anything, and Windows will happily return a black
/// frame for one; the caller is expected to refuse instead.
pub fn input_desktop_name() -> io::Result<String> {
    use windows_sys::Win32::System::StationsAndDesktops::{
        CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, UOI_NAME,
    };

    // SAFETY: the handle is closed on every path below; the name buffer is sized by the call itself.
    unsafe {
        let desktop = OpenInputDesktop(0, FALSE, 0x0001 /* DESKTOP_READOBJECTS */);
        if desktop == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut needed = 0u32;
        let _ = GetUserObjectInformationW(desktop, UOI_NAME, std::ptr::null_mut(), 0, &mut needed);
        let mut buffer = vec![0u16; (needed as usize / 2).max(1)];
        let read = GetUserObjectInformationW(
            desktop,
            UOI_NAME,
            buffer.as_mut_ptr() as *mut _,
            (buffer.len() * 2) as u32,
            &mut needed,
        );
        let error = io::Error::last_os_error();
        CloseDesktop(desktop);
        if read == 0 {
            return Err(error);
        }
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        Ok(String::from_utf16_lossy(&buffer[..end]))
    }
}

/// Capture the primary screen as top-down RGBA.
///
/// The sequence is the shortest GDI one that produces pixels: a screen DC, a memory DC with a
/// compatible bitmap, one `BitBlt`, and one `GetDIBits` into a 32-bit top-down buffer. Every handle is
/// released on every path, including the failure paths, which is why the calls are sequenced rather
/// than nested.
pub fn capture_primary_screen() -> io::Result<Screen> {
    use windows_sys::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        SRCCOPY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    // SAFETY: every handle created here is released before the function returns, and the buffers are
    // sized by the values Windows reports.
    unsafe {
        let width = GetSystemMetrics(SM_CXSCREEN);
        let height = GetSystemMetrics(SM_CYSCREEN);
        if width <= 0 || height <= 0 {
            return Err(io::Error::other("the desktop reports no size"));
        }
        let screen = GetDC(0);
        if screen == 0 {
            return Err(io::Error::last_os_error());
        }
        let memory = CreateCompatibleDC(screen);
        if memory == 0 {
            ReleaseDC(0, screen);
            return Err(io::Error::last_os_error());
        }
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        if bitmap == 0 {
            DeleteDC(memory);
            ReleaseDC(0, screen);
            return Err(io::Error::last_os_error());
        }
        let previous = SelectObject(memory, bitmap);
        let copied = BitBlt(memory, 0, 0, width, height, screen, 0, 0, SRCCOPY);

        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width;
        info.bmiHeader.biHeight = -height;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
        let lines = if copied != 0 {
            GetDIBits(
                memory,
                bitmap,
                0,
                height as u32,
                rgba.as_mut_ptr() as *mut _,
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        let error = io::Error::last_os_error();

        SelectObject(memory, previous);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(0, screen);

        if copied == 0 {
            return Err(error);
        }
        if lines == 0 {
            return Err(error);
        }
        // GDI hands back BGRA; the PNG the caller writes wants RGBA.
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }
        Ok(Screen {
            width: width as u32,
            height: height as u32,
            rgba,
        })
    }
}
