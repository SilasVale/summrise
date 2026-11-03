# Screen capture: why the shell's GDI path is refused on 14RJCR8

Status: investigation complete, implementation deferred to the next change.
Date: 2026-11-03. Machine: DESKTOP-14RJCR8 (Windows 11, RDP session 1, Administrator).

## What works

PowerShell in the logged-in session, started by a Task Scheduler task with an Interactive principal:

```powershell
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$v = [System.Windows.Forms.SystemInformation]::VirtualScreen
$b = New-Object System.Drawing.Bitmap($v.Width, $v.Height)
$g = [System.Drawing.Graphics]::FromImage($b)
$g.CopyFromScreen($v.Location, [System.Drawing.Point]::Empty, $b.Size)
$b.Save('C:\Windows\Temp\capture-test2.png', [System.Drawing.Imaging.ImageFormat]::Png)
```

Result: 25,174-byte PNG, repeated while the same RDP session was in use. The desktop has a
surface; a locked or disconnected session would not produce this.

## What fails

`summrise-shell` runs in session 1, reports `winsta=WinSta0`, `desktop=Default`, and its HTTP
capture handler still gets `BitBlt: ERROR_INVALID_HANDLE (6)`. A standalone console probe built from
the same `summrise-winsys` code reproduces it, so this is not the tray process, the HTTP thread, or
Session 0.

The following were measured and did not change the result:

- `GetDC(0)`, `GetDC(GetDesktopWindow())`, `GetWindowDC(GetDesktopWindow())`, `CreateDC("DISPLAY")`,
  `GetDCEx(..., DCX_WINDOW | DCX_CACHE)`;
- a compatible bitmap and a 32-bit DIB section, with `GetDIBits` deselected before reading;
- `CAPTUREBLT` (turns 6 into `ERROR_ACCESS_DENIED (5)`);
- attaching the thread to the input desktop and opening `WinSta0` with `WINSTA_READSCREEN`.

## What was tried and reverted

A shell fallback that ran PowerShell GDI+ as a child of the shell kept the HTTP handler blocked past
the device's 20 s helper timeout, and left a PowerShell process behind. It was reverted; the shell
installed on 14RJCR8 is the committed build, and capture answers the known error rather than hanging.

## Decision

Do not keep tuning raw GDI in the shell. The next implementation is a dedicated capture helper:

1. A small console helper in the user session, using the GDI+ path that is measured working; the
   shell invokes it with a hard timeout and returns its PNG, or
2. Windows Graphics Capture if a helper process cannot be made reliable.

The helper must return within a few seconds, be killed on timeout, and keep the existing refusal
sentences for locked/disconnected sessions. The shell remains the orchestrator; no capture code runs
on its HTTP thread.

## Acceptance

- On 14RJCR8 with the active RDP session: `screen.capture` returns a PNG, not error 6.
- Locked/disconnected session: the existing refusal, not a hang.
- No orphan PowerShell/helper process after a capture.

## Round 4: the helper must be started by the device, not by the shell

A standalone console probe started by Task Scheduler as the logged-in user (`Session 1`,
`WinSta0`, `Default`) still got `BitBlt: ERROR_INVALID_HANDLE`. The same probe built with
`SetProcessDPIAware` still failed. This rules out the tray process, the HTTP thread and DPI
awareness as the cause.

A task whose action is a `.cmd` wrapper around PowerShell's GDI+ capture, registered with an
**Interactive** principal, produced a 25,174-byte PNG. A task registered from the shell through
`schtasks /create` did not produce a picture: that task is not necessarily interactive. Registering
the task from the shell through PowerShell with `New-ScheduledTaskPrincipal -LogonType Interactive`
then hung past the helper timeout.

Decision for the next change: the capture helper is started by `summrise-launch` (or the device
service) with the user's token and `winsta0\default`, exactly like the shell start path. It writes a
PNG and exits; the device reads it. Do not create the helper from the shell's HTTP handler and do not
use `schtasks /create` to obtain the interactive desktop.

## Round 5: the device cannot start the helper either

Two more paths were measured and rejected:

- `summrise-launch --capture` run through the device's `runs.exec` fails at
  `CreateProcessAsUserW` with ERROR_ACCESS_DENIED. Enabling
  `SeAssignPrimaryTokenPrivilege`/`SeIncreaseQuotaPrivilege` in the launcher did not change it;
  the service host's job does not permit a child in another session. `--start-shell` from the same
  context fails identically, while the installer's one-shot SYSTEM task succeeds.
- A SYSTEM task can run the launcher, but the launcher's PowerShell child then fails
  `CopyFromScreen` with "invalid parameter" (the user desktop is not the one its token receives).
  The same GDI+ script run by a task registered directly for the user produces a 25,174-byte PNG.
- A device-side fallback that registers that task from `tokio::process::Command` failed with empty
  stdout/stderr from the registration child, so it was reverted rather than shipped unverified.

The working primitive remains: **a task registered for the logged-in user with an Interactive
principal, whose action is PowerShell GDI+**. The next implementation must register that task from a
process outside the device's job (the installer's SYSTEM task is the proven host), or replace the
path with Windows Graphics Capture. Raw GDI, launcher `CreateProcessAsUser`, and shell-side task
creation are all closed.

## Round 11: an interactive task is not a guarantee either

A task created with `schtasks /create /ru administrator /it` and running the GDI+ script produced a
25,174-byte PNG at 13:43. At 13:46 the same task, same principal, same script path, failed
`CopyFromScreen` with "invalid parameter", and five retries 400 ms apart all failed while `quser`
reported the RDP session active. The display surface is therefore not a property the task can rely
on; it can disappear while the session is nominally connected.

A shell fallback that registered this task, ran it and waited for the file was measured and
**reverted**: it can hang the helper for 15 s and then answer "no picture" even though the task
exists. The committed shell keeps the existing `BitBlt` refusal. The next real implementation is
Windows Graphics Capture (or a surface check the task itself can fail fast on), not another task
wrapper.

## Round 16: PrintWindow is not a way around the surface

`PrintWindow(GetDesktopWindow(), hdc, PW_RENDERFULLCONTENT)` was measured on 14RJCR8 with a 32-bit
DIB section: it returned 0 while the desktop reported 1024x768, and every pixel was black. The
session had no surface for either GDI path; the refusal is the honest answer. Windows Graphics
Capture remains the only untried native path, and it would still be capturing a desktop that is not
being displayed while the session is in that state.
