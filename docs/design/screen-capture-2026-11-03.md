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
