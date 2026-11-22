# Lethe Launcher

A custom launcher for Limbus Company that provides intelligent file management, Steam integration, and modding support via BepInEx.

## Building

Use the Tauri CLI, not a bare `cargo build`:

```powershell
cd lethe-launcher
npm ci
npm run tauri build
```

The binary lands at `src-tauri/target/release/LimbusCompany.exe`.

`bundle.active` is `false` in `tauri.conf.json`, so no installer is produced —
only the portable exe. It is meant to be dropped into the game directory next
to `UnityPlayer.dll`; every path it touches is relative to its working
directory.

> **Do not build with `cargo build --release` directly.** It bypasses the Tauri
> CLI, which leaves `tauri-build` unable to see the dev/prod signal
> (`DEP_TAURI_DEV`). The resulting exe is compiled in release mode but still
> loads `devUrl` (`http://localhost:5173`) at runtime and shows
> `ERR_CONNECTION_REFUSED` instead of the bundled frontend. The window title
> will look correct regardless, so this is easy to miss — check the WebView
> title via the UI Automation tree, or simply open the app.

## Downloads

Files are fetched with up to 4 concurrent connections. The manifest is
roughly 2000 files / 11.8 GB, and the host serves HTTP/1.1 with no edge
caching, so throughput is limited by per-connection rate rather than by
the link. Measured against the live endpoint (4 x ~15 MB files, 58.9 MB
total):

| Configuration | Throughput |
| --- | --- |
| Sequential | 0.95 MB/s |
| 4 files in parallel | 1.44 MB/s |
| 8 files in parallel | 1.56 MB/s |
| 12 files in parallel | 1.75 MB/s |

4 is the knee of that curve, which is why it is the default. Files at or
above 100 MB are additionally split into 2 HTTP `Range` requests, which
only helps on top of file-level parallelism.

Downloads are resumable and retried:

- Each file is written to `<path>.lethe-part` and only renamed into
  place once its size matches the manifest, so an interrupted run never
  leaves a truncated file behind. A 1.36 GB download resumes instead of
  restarting.
- A transient failure is retried up to 4 times with exponential backoff
  (500 ms, doubling, capped at 8 s). Ranges already written are skipped
  on retry.
- A single `reqwest::Client` is shared process-wide so the TLS handshake
  is paid once per connection instead of once per file.

The progress bar reports aggregate throughput and an estimated remaining
time. The rate is smoothed (EMA, weight 0.3) because a single 250 ms
window swings too much to display raw.

## Logging

All operations are logged to `lethe-launcher.log` with timestamps:

```
=== Lethe Launcher Started at 2025-10-26 12:43:48 ===
[12:43:48] Configuration loaded from lethe-launcher.ini:
[12:43:48]   DisableAutoUpdate = false
[12:43:49] Downloaded manifest: 150 files, 2048576 bytes
[12:43:49] Starting file synchronization...
[12:43:50] Copied from local: Data/file.dat (1.2 MB)
[12:43:51] Downloaded: Assets/texture.png (512 KB, 1.4 MB/s, 1 attempt(s), 0.4s)
[12:43:51] Resuming Big/asset.resS from 2147483648 (2.0 GB already downloaded)
[12:43:52] Update complete. Starting game...
[12:43:52] ✓ Found UnityPlayer.dll at: C:\Game\UnityPlayer.dll
[12:43:52] ✓ UnityPlayer.dll loaded successfully
[12:43:52] ✓ doorstop.dll loaded successfully
[12:43:52] UnityMain returned: 0
```

