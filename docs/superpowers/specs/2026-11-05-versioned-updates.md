# Versioned directories, and a pointer: how updates should work

**Decision (2026-11-05, operator-chosen): approach B — versioned install directories with a stable
entry point and an atomic pointer — over the in-place replacement Tailscale's MSI uses.** In-place
replacement works because MSI transactions are mature; it does not remove the window between "stop
the old process" and "start the new one", and that window is where the 2026-11-04 fleet outage
happened (a port rebind race, a rollback that failed on the same race, and a supervisor that then
gave up). Versioned directories remove the window entirely: an update never overwrites a running
file.

## The layout

```
C:\SummriseConsole\
  summrise-launch.exe        # stable, never replaced by an update; reads current.txt
  current.txt                # the one mutable fact: "versions\0.3.274-ux38"
  versions\
    0.3.271-ux36\summrise-console.exe
    0.3.274-ux38\summrise-console.exe
```

The device side mirrors it (`summrise-launch.exe`, `current.txt`, `versions\<ver>\`), and the tray
reads the service's status; the tray is a separate per-user process and is never the service's host.

## The update, in order

1. The service downloads the new version into `versions\<ver>\` (nothing running is touched).
2. It verifies the signed document against the downloaded bytes (the signing already exists).
3. It health-checks the new binary **before** switching: start it on a scratch port with the same
   data directory, ask `/api/health`, stop it. A build that cannot serve is never made current.
4. It writes `current.txt` (write to a temp file, then `MoveFileEx` with replace — one atomic step).
5. It restarts the service (or the supervisor restarts the child from the current pointer).
6. The previous version stays on disk; pointing `current.txt` back is the rollback, and it is
   immediate.

## Why this is better than in-place replacement

- No running file is ever overwritten, so file locks and the stop→start window disappear.
- Rollback is a pointer write, not a file restore.
- Two versions can coexist, which is what a canary needs.
- "Installed a new version but still running the old process" cannot happen: the restart reads the
  pointer.

## The one-time migration

An installer that writes this layout is needed once per machine (the current install is flat). After
that, every update is the five steps above. The bootstrap installer must:

- create `versions\<current>\` from the running install, write `current.txt`, and add the stable
  launcher;
- stop the old service entry, point it at the launcher, restart it, and check `/api/health`;
- leave the old flat files in place for one release as a fallback.

## What stays as it is

- Signing (document + sidecar), the release channel and its pointer on the mirror.
- The service/tray split: the service is headless, the tray is a per-user client.
- The launch supervisor: it still restarts a crashed child, still waits for the port to be bindable
  (2026-11-04 fix), and still retries instead of giving up once the pointer makes rollback trivial.
