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
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        Ok(Screen {
            width: width as u32,
            height: height as u32,
            rgba,
        })
    }
}

/// One mounted volume as the operator sees it (`C:\`).
pub struct Volume {
    pub name: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

/// One Windows session: a person, or the service session, and what it is doing.
pub struct Session {
    /// The session id (`WTS_SESSION_INFO.WSessionId`).
    pub id: u32,
    /// The account, when the session has one.
    pub user: Option<String>,
    /// `active`, `connected`, `disconnected`, `idle`, `listen`, or the state's numeric value.
    pub state: String,
}

/// The machine facts the console needs, gathered in one pass.
pub struct HostFacts {
    /// The real Windows version, e.g. `10.0.26100` (`RtlGetVersion`, not the manifest-dependent
    /// `GetVersionEx`, which reports 6.2 on a modern build for an unmanifested executable).
    pub os_version: String,
    /// The client release the build belongs to, e.g. `Windows 11`. Windows 10 and 11 both report a
    /// `10.0.x` version, so the build number is the only discriminator; `os_version` stays raw.
    /// Client builds only — Server 2022's 20348 would read as Windows 11.
    pub os_release: String,
    /// The NetBIOS name.
    pub machine: String,
    /// Seconds since boot.
    pub uptime_s: u64,
    pub memory_total_bytes: u64,
    pub memory_available_bytes: u64,
    /// The machine's CPU busy share over a short sample, or `None` when the counter could not be read.
    ///
    /// Sampled twice, [`CPU_SAMPLE_MS`] apart: `GetSystemTimes` reports monotonically increasing
    /// counters, so one reading says nothing and two say the share of the interval that was not idle.
    pub cpu_percent: Option<u8>,
    pub volumes: Vec<Volume>,
    pub sessions: Vec<Session>,
}

/// How long the CPU sample interval is. Long enough to average out a scheduling burst, short enough
/// that a caller waiting for the facts does not notice.
pub const CPU_SAMPLE_MS: u64 = 200;

/// Gather the host facts; a fact that cannot be read is zero, not an error, because a device that
/// answers with five of six facts is more useful than one that answers with none.
pub fn host_facts() -> io::Result<HostFacts> {
    let os_version = os_version()?;
    let os_release = os_release(&os_version).to_owned();
    Ok(HostFacts {
        os_version,
        os_release,
        machine: machine_name()?,
        uptime_s: uptime_seconds(),
        memory_total_bytes: memory().map(|(total, _)| total).unwrap_or(0),
        memory_available_bytes: memory().map(|(_, available)| available).unwrap_or(0),
        cpu_percent: cpu_percent(),
        volumes: volumes(),
        sessions: sessions(),
    })
}

/// The machine's CPU busy share over [`CPU_SAMPLE_MS`], or `None` when the counters cannot be read.
///
/// `GetSystemTimes` reports three FILE_TIME counters since boot: idle, kernel and user. Kernel time
/// INCLUDES idle time, so busy is `(kernel - idle) + user` and total is `kernel + user`.
pub fn cpu_percent() -> Option<u8> {
    use windows_sys::Win32::System::Threading::GetSystemTimes;

    /// One reading of the three counters, in 100 ns units.
    fn sample() -> Option<(u64, u64, u64)> {
        let mut idle = windows_sys::Win32::Foundation::FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut kernel = idle;
        let mut user = idle;
        // SAFETY: three out pointers, each valid for the call's duration.
        let ok = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };
        if ok == 0 {
            return None;
        }
        // FILETIME holds two 32-bit halves; the counter fits in 64 bits assembled the same way.
        let plain = |time: windows_sys::Win32::Foundation::FILETIME| {
            ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64
        };
        Some((plain(idle), plain(kernel), plain(user)))
    }

    let (idle_before, kernel_before, user_before) = sample()?;
    std::thread::sleep(std::time::Duration::from_millis(CPU_SAMPLE_MS));
    let (idle_after, kernel_after, user_after) = sample()?;

    let idle = idle_after.saturating_sub(idle_before);
    // Kernel includes idle on both readings, so the subtraction is the busy part of kernel time.
    let kernel = kernel_after.saturating_sub(kernel_before);
    let user = user_after.saturating_sub(user_before);
    let total = kernel.saturating_add(user);
    if total == 0 {
        return None;
    }
    let busy = total.saturating_sub(idle);
    Some(((busy.saturating_mul(100) + total / 2) / total).min(100) as u8)
}

/// The session a process is in, which is how the device knows where its helper landed.
pub fn session_of_process(pid: u32) -> Option<u32> {
    use windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId;

    let mut session = 0u32;
    // SAFETY: the out pointer is valid for the call.
    let ok = unsafe { ProcessIdToSessionId(pid, &mut session) };
    (ok != 0).then_some(session)
}

/// The client Windows release a build belongs to. Windows 10 and 11 share the 10.0 major, so the
/// build is the only discriminator; an unreadable build stays the generic family rather than a guess.
fn os_release(version: &str) -> &'static str {
    let build = version
        .rsplit('.')
        .next()
        .and_then(|part| part.parse::<u32>().ok());
    match build {
        Some(22000..) => "Windows 11",
        Some(10240..=19045) => "Windows 10",
        _ => "Windows",
    }
}

fn os_version() -> io::Result<String> {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

    // `RtlGetVersion` is the authoritative call; `GetVersionEx` reports 6.2 for an executable without
    // a compatibility manifest, which is every build of this binary.
    type RtlGetVersion = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;
    let mut info: OSVERSIONINFOW = unsafe { zeroed() };
    info.dwOSVersionInfoSize = size_of::<OSVERSIONINFOW>() as u32;
    // SAFETY: `ntdll` is loaded in every process, and the symbol has the documented signature.
    unsafe {
        let module = windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(windows_sys::w!(
            "ntdll.dll"
        ));
        if module == 0 {
            return Err(io::Error::last_os_error());
        }
        let Some(address) = windows_sys::Win32::System::LibraryLoader::GetProcAddress(
            module,
            c"RtlGetVersion".as_ptr() as *const u8,
        ) else {
            return Err(io::Error::last_os_error());
        };
        let rtl_get_version: RtlGetVersion = std::mem::transmute(address);
        let status = rtl_get_version(&mut info);
        if status != 0 {
            return Err(io::Error::other(format!(
                "RtlGetVersion returned {status:#x}"
            )));
        }
    }
    Ok(format!(
        "{}.{}.{}",
        info.dwMajorVersion, info.dwMinorVersion, info.dwBuildNumber
    ))
}

fn machine_name() -> io::Result<String> {
    use windows_sys::Win32::System::SystemInformation::{
        ComputerNamePhysicalDnsHostname, GetComputerNameExW,
    };

    let mut buffer = [0u16; 256];
    let mut length = buffer.len() as u32;
    // SAFETY: the buffer and its length match.
    let ok = unsafe {
        GetComputerNameExW(
            ComputerNamePhysicalDnsHostname,
            buffer.as_mut_ptr(),
            &mut length,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(String::from_utf16_lossy(&buffer[..length as usize]))
}

fn uptime_seconds() -> u64 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;

    // SAFETY: no arguments, no pointers.
    unsafe { GetTickCount64() / 1000 }
}

fn memory() -> Option<(u64, u64)> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status: MEMORYSTATUSEX = unsafe { zeroed() };
    status.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: the structure is sized and initialized as the call expects.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then_some((status.ullTotalPhys, status.ullAvailPhys))
}

fn volumes() -> Vec<Volume> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives,
    };
    use windows_sys::Win32::System::WindowsProgramming::{DRIVE_FIXED, DRIVE_REMOTE};

    // SAFETY: every call below takes a NUL-terminated wide string or no arguments.
    unsafe {
        let mask = GetLogicalDrives();
        let mut volumes = Vec::new();
        for index in 0..26u32 {
            if mask & (1 << index) == 0 {
                continue;
            }
            let letter = (b'A' + index as u8) as u16;
            let root = [letter, b':' as u16, b'\\' as u16, 0u16];
            let kind = GetDriveTypeW(root.as_ptr());
            if kind != DRIVE_FIXED && kind != DRIVE_REMOTE {
                continue;
            }
            let mut free = 0u64;
            let mut total = 0u64;
            if GetDiskFreeSpaceExW(root.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) == 0
            {
                continue;
            }
            volumes.push(Volume {
                name: String::from_utf16_lossy(&root[..3]),
                total_bytes: total,
                free_bytes: free,
            });
        }
        volumes
    }
}

fn sessions() -> Vec<Session> {
    use windows_sys::Win32::System::RemoteDesktop::{
        WTSEnumerateSessionsW, WTSFreeMemory, WTS_CURRENT_SERVER_HANDLE, WTS_SESSION_INFOW,
    };

    let mut sessions = Vec::new();
    let mut pointer: *mut WTS_SESSION_INFOW = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: the call fills a newly allocated array and its count; both are released below.
    let ok =
        unsafe { WTSEnumerateSessionsW(WTS_CURRENT_SERVER_HANDLE, 0, 1, &mut pointer, &mut count) };
    if ok == 0 {
        return sessions;
    }
    for index in 0..count as usize {
        // SAFETY: the array has `count` entries.
        let info = unsafe { &*pointer.add(index) };
        let user = session_user(info.SessionId);
        sessions.push(Session {
            id: info.SessionId,
            user,
            state: state_name(info.State),
        });
    }
    // SAFETY: the pointer came from `WTSEnumerateSessionsW`.
    unsafe { WTSFreeMemory(pointer as *mut _) };
    sessions
}

fn session_user(session: u32) -> Option<String> {
    use windows_sys::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSQuerySessionInformationW, WTSUserName, WTS_CURRENT_SERVER_HANDLE,
    };

    let mut buffer: *mut u16 = std::ptr::null_mut();
    let mut bytes = 0u32;
    // SAFETY: the call allocates the buffer and its byte length; both are released below.
    let ok = unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            session,
            WTSUserName,
            &mut buffer,
            &mut bytes,
        )
    };
    if ok == 0 || buffer.is_null() || bytes == 0 {
        return None;
    }
    let name = String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(buffer, (bytes / 2) as usize)
    });
    // SAFETY: the buffer came from `WTSQuerySessionInformationW`.
    unsafe { WTSFreeMemory(buffer as *mut _) };
    let name = name.trim_end_matches('\0').to_owned();
    (!name.is_empty()).then_some(name)
}

fn state_name(state: windows_sys::Win32::System::RemoteDesktop::WTS_CONNECTSTATE_CLASS) -> String {
    // The numeric values are stable and documented; the names are for a human reading the console.
    match state {
        0 => "active".to_owned(),
        1 => "connected".to_owned(),
        2 => "connect-query".to_owned(),
        3 => "shadow".to_owned(),
        4 => "disconnected".to_owned(),
        5 => "idle".to_owned(),
        6 => "listen".to_owned(),
        7 => "reset".to_owned(),
        8 => "down".to_owned(),
        9 => "init".to_owned(),
        other => format!("state-{other}"),
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
    fn the_build_names_the_client_release() {
        assert_eq!(os_release("10.0.22631"), "Windows 11");
        assert_eq!(os_release("10.0.26100"), "Windows 11");
        assert_eq!(os_release("10.0.19045"), "Windows 10");
        assert_eq!(os_release("10.0.10240"), "Windows 10");
        assert_eq!(os_release("garbage"), "Windows");
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
