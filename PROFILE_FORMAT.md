# Profile format

Profiles live in `<game dir>/lethe-profiles/`:

```
lethe-profiles/
  profiles.json                 # { "active": "local", "profiles": ["local", "official"] }
  local/
    profile.json                # addresses, type, version, plugin list, mod toggles
    lethe-auth.json             # this profile's login state (not distributed)
    lethe-launcher.ini          # this profile's launcher settings
  official/
    profile.json
    ...
```

The launcher creates this layout on first run and migrates a legacy
root-level `lethe-auth.json` into the default `local` profile. You normally
edit profiles in the **Admin Panel → Servers** tab, not by hand.

## `profile.json`

```json
{
  "id": "local",
  "name": "Local private server",
  "type": "private",
  "serverVersion": "1.2.4",
  "authBaseUrl": "http://127.0.0.1:8080",
  "manifestUrl": "http://127.0.0.1:8080/lethe-manifest.json",
  "downloadBaseUrl": "http://127.0.0.1:8080/download/",
  "pluginUrls": [
    { "url": "http://127.0.0.1:8080/Lethe.dll", "filename": "Lethe.dll" }
  ],
  "newsUrl": "",
  "updateUrl": "https://github.com/HZBHZB1234/PortableLetheLauncher/releases/latest/download/latest.json",
  "serverRoot": "http://127.0.0.1:8080/",
  "ignoreLauncherToken": true,
  "cachePath": "./LetheCache",
  "sameMachine": true,
  "mods": [{ "name": "ExampleMod", "enabled": true }]
}
```

* `type` is `private` or `official`. Official profiles only describe the
  official server addresses; **ZylarNex is not bundled or referenced as a
  private-server component** and must be installed by the user if wanted.
* Blank address fields fall back to the upstream defaults:
  `https://api.lethelc.site`, `https://files.lethelc.site/lethe-manifest.json`,
  `https://files.lethelc.site/download/`,
  `https://api.lethelc.site/`, and the PortableLetheLauncher update feed.
* `serverVersion` is free-form compatibility metadata shown in the UI. Record
  the launcher ↔ server protocol version here when shipping an instance.
* `mods` is the desired folder state under `BepInEx/plugins/Lethe/mods/`. On a
  profile switch the launcher renames folders to `DISABLED_<name>` as needed.

## Game-side config

Switching a profile rewrites only these keys in `BepInEx/config/Lethe.cfg`,
preserving every other setting and comment:

* `[General] ServerRoot` ← `serverRoot`
* `[General] IgnoreLauncherToken` ← `ignoreLauncherToken`
* `[General] CachePath` ← `cachePath`

The launcher does **not** rewrite `Lethe.cfg` at startup, so a Hub-exported
package with a preconfigured instance port remains valid until the user
explicitly switches profiles.

## Login state

`lethe-auth.json` is per profile. The in-memory token is cleared on switch,
and `LETHE_TOKEN` is always resolved from the active profile's file (never
from WebView state). Expired JWTs are deleted and require a new login.
