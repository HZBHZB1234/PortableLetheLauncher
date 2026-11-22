# Portable Lethe Launcher

A **portable, prebuilt** Limbus Company launcher for private and official
server instances. It keeps server addresses, login state and mod toggles in
per-profile directories, so one game installation can switch servers without
rebuilding or editing source code.

The launcher is a single `LimbusCompany.exe` that must be placed next to
`UnityPlayer.dll` in the game directory. Every path it touches is relative to
its working directory. The Hub downloads the prebuilt zip from GitHub
Releases; it never compiles Rust.

> ZylarNex is **not** bundled or referenced by this repository. It belongs only
> to an official-server setup the user installs themselves. Private profiles
> use the Lethe browser-login flow against their own `ServerRoot`.

## Features

* **Server profiles** — create/edit/switch private or official profiles from
  the in-app Admin Panel. All six formerly hardcoded URLs are per-profile:
  `AuthBaseUrl`, `ManifestUrl`, `DownloadBaseUrl`, `PluginUrls[]`, `NewsUrl`,
  `UpdateUrl`.
* **Game-side switching** — a switch rewrites `ServerRoot`,
  `IgnoreLauncherToken` and `CachePath` in `BepInEx/config/Lethe.cfg` while
  preserving all other settings and comments. Startup does not rewrite the
  cfg, so Hub-generated instance configs are respected.
* **Login isolation** — one `lethe-auth.json` per profile, in-memory token
  bound to the active profile id, and `LETHE_TOKEN` injected into the game
  only from that file. Switching clears the cached token; expired JWTs are
  removed and require a fresh login.
* **Same-machine helper** — tick "Same-machine mode", enter the local server
  port and apply: auth/manifest/download/server-root addresses are generated
  automatically, and `IgnoreLauncherToken` is enabled.
* **Connection test** — `GET <base>/health` with latency and HTTP status.
* **Mod toggles** — the existing `BepInEx/plugins/Lethe/mods` folder switch is
  remembered per profile and reapplied when switching.
* **Diagnostics** — log tail, active profile, addresses, and token status
  (valid / expired / none) without ever printing the token itself.

## Directory layout

```
<game dir>/
  LimbusCompany.exe
  UnityPlayer.dll
  BepInEx/config/Lethe.cfg
  lethe-launcher.ini            # root compatibility file (mirrored from active profile)
  lethe-auth.json               # root compatibility file (mirrored from active profile)
  lethe-launcher.log
  lethe-profiles/
    profiles.json               # { active, profiles: [<id>] }
    <id>/profile.json
    <id>/lethe-auth.json
    <id>/lethe-launcher.ini
```

See [PROFILE_FORMAT.md](PROFILE_FORMAT.md) for the full JSON schema and
defaults.

## Using the launcher

1. Unpack `PortableLetheLauncher-win-x64.zip` into the game directory (next to
   `UnityPlayer.dll`) and run `LimbusCompany.exe`.
2. On first run a `local` private profile is created and any legacy root
   `lethe-auth.json` is migrated into it.
3. Click the gear icon (**Admin Panel**) → **Servers** to add profiles, edit
   addresses, test `/health`, and switch. **General** holds the file-validation
   toggle; **Diagnostics** shows the log tail and token state.
4. Log in with the active profile (the login page opens in your browser). The
   token is stored in that profile's directory only.
5. Launch the game. `LETHE_TOKEN` comes from the active profile; when there is
   no valid token the game runs its own browser login against `ServerRoot`.

## Building

Use the Tauri CLI, not a bare `cargo build`:

```powershell
cd lethe-launcher
npm ci
npm run tauri build
```

The binary lands at `src-tauri/target/release/LimbusCompany.exe`.
`bundle.active` is `false` in `tauri.conf.json`, so only the portable exe is
produced. A bare `cargo build --release` bypasses the Tauri CLI and produces an
exe that tries to load `http://localhost:5173` instead of the bundled frontend.

> Local build tip: set `CARGO_TARGET_DIR` to a drive with free space. All E:
> drive paths in this workspace are space-constrained, and the debug target
> tree can exceed 1 GB.

## Releasing

`.github/workflows/build.yml` builds on `windows-latest` and uploads:

| Asset | Purpose |
| --- | --- |
| `PortableLetheLauncher-win-x64.zip` | Hub distribution unit (exe + README + profile format doc) |
| `LimbusCompany.exe` | Raw exe consumed by the self-updater's `latest.json` |
| `latest.json` | Update manifest with SHA-256 of the raw exe |
| `SHA256SUMS.txt` | Checksums of exe and zip |

Push to `main`/`master` produces a nightly pre-release; publishing a GitHub
Release attaches the same assets. Verify the SHA-256 before shipping a package.

The default update feed is
`https://github.com/HZBHZB1234/PortableLetheLauncher/releases/latest/download/latest.json`.
Profiles may override it with their own `updateUrl`, or set it to an empty
string to disable update checks.

## Verification (this checkout)

* `cargo test --offline --lib` — Rust unit tests, including a real
  `BepInEx/config/Lethe.cfg` rewrite test that verifies comments and unrelated
  settings survive two consecutive profile switches.
* `npm run build` — TypeScript typecheck + Vite bundle.
* `npm test` — Vitest component tests including the admin panel.

## Known limitations / stubs

* The news feed is a thin JSON fetcher for `NewsUrl`; an empty URL disables it.
  No news endpoint is bundled.
* No automatic migration of per-profile mod folders from an upstream install
  exists: `mods` starts empty and is populated by UI toggles.
* No ledger of launcher ↔ server protocol compatibility is enforced;
  `serverVersion` is documentation metadata shown in the UI.
* The GitHub remote for this repository may not exist yet; nothing is pushed
  from this workspace.
