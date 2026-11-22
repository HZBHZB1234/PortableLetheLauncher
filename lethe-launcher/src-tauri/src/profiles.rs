//! Portable profile management.
//!
//! Every portable install keeps its server addresses, login state and launcher
//! preferences in `<game dir>/lethe-profiles/`. Only one profile is active at a
//! time; switching it rewrites the compatibility files in the game directory
//! and the three Lethe.cfg keys the game reads at startup.
//!
//! The paths are intentionally relative to the launcher's working directory:
//! the release is a portable exe dropped next to `UnityPlayer.dll`.

use ini::Ini;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::Instant;

pub const PROFILES_DIR: &str = "lethe-profiles";
const INDEX_FILE: &str = "profiles.json";
const PROFILE_FILE: &str = "profile.json";
const AUTH_FILE: &str = "lethe-auth.json";
const INI_FILE: &str = "lethe-launcher.ini";
const ROOT_AUTH_FILE: &str = "lethe-auth.json";
const ROOT_INI_FILE: &str = "lethe-launcher.ini";
const GAME_CFG: &str = "BepInEx/config/Lethe.cfg";
const LOG_FILE: &str = "lethe-launcher.log";
const MODS_DIR: &str = "BepInEx/plugins/Lethe/mods";

// Sane defaults. They are the values the upstream launcher had hardcoded; a
// profile only needs to override the ones that differ for its server.
pub const DEFAULT_AUTH_BASE_URL: &str = "https://api.lethelc.site";
pub const DEFAULT_MANIFEST_URL: &str = "https://files.lethelc.site/lethe-manifest.json";
pub const DEFAULT_DOWNLOAD_BASE_URL: &str = "https://files.lethelc.site/download/";
pub const DEFAULT_SERVER_ROOT: &str = "https://api.lethelc.site/";
pub const DEFAULT_NEWS_URL: &str = "";
pub const DEFAULT_UPDATE_URL: &str =
    "https://github.com/HZBHZB1234/PortableLetheLauncher/releases/latest/download/latest.json";
pub const DEFAULT_CACHE_PATH: &str = "./LetheCache";

/// In-memory binding to the active profile. Disk remains the source of truth;
/// this cache exists so a stale frontend cannot read another profile's token.
static ACTIVE_ID: OnceLock<RwLock<String>> = OnceLock::new();
static PROFILE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn io_lock() -> &'static Mutex<()> {
    PROFILE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProfileType {
    /// Local/private server profile. Uses browser login against ServerRoot.
    #[default]
    Private,
    /// Official server profile. ZylarNex (not bundled) is only relevant here.
    Official,
}

impl ProfileType {
    pub fn as_str(self) -> &'static str {
        match self {
            ProfileType::Private => "private",
            ProfileType::Official => "official",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PluginFile {
    pub url: String,
    pub filename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModState {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(rename = "type", alias = "profileType", default)]
    pub profile_type: ProfileType,
    #[serde(default)]
    pub server_version: String,
    #[serde(default)]
    pub auth_base_url: String,
    #[serde(default)]
    pub manifest_url: String,
    #[serde(default)]
    pub download_base_url: String,
    #[serde(default)]
    pub plugin_urls: Vec<PluginFile>,
    #[serde(default)]
    pub news_url: String,
    #[serde(default)]
    pub update_url: String,
    #[serde(default)]
    pub server_root: String,
    #[serde(default)]
    pub ignore_launcher_token: bool,
    #[serde(default)]
    pub cache_path: String,
    #[serde(default)]
    pub same_machine: bool,
    #[serde(default)]
    pub mods: Vec<ModState>,
}

/// Input accepted by `profile_create`. All address fields are optional and get
/// filled with the upstream defaults when omitted.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    #[serde(default)]
    pub id: String,
    pub name: String,
    #[serde(rename = "type", alias = "profileType", default)]
    pub profile_type: ProfileType,
    #[serde(default)]
    pub server_version: String,
    #[serde(default)]
    pub auth_base_url: String,
    #[serde(default)]
    pub manifest_url: String,
    #[serde(default)]
    pub download_base_url: String,
    #[serde(default)]
    pub plugin_urls: Vec<PluginFile>,
    #[serde(default)]
    pub news_url: String,
    #[serde(default)]
    pub update_url: String,
    #[serde(default)]
    pub server_root: String,
    #[serde(default)]
    pub ignore_launcher_token: Option<bool>,
    #[serde(default)]
    pub cache_path: String,
    #[serde(default)]
    pub same_machine: bool,
    #[serde(default)]
    pub mods: Vec<ModState>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: String,
    pub name: String,
    pub profile_type: ProfileType,
    pub active: bool,
    pub server_version: String,
    pub server_root: String,
    pub logged_in: bool,
    pub username: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Addresses {
    pub auth_base_url: String,
    pub manifest_url: String,
    pub download_base_url: String,
    pub news_url: String,
    pub update_url: String,
    pub server_root: String,
    pub plugin_urls: Vec<PluginFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStatus {
    pub profile_id: String,
    pub is_logged_in: bool,
    pub username: String,
    pub avatar_url: String,
    /// True when a token file exists but its JWT `exp` has passed.
    pub expired: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    pub url: String,
    pub ok: bool,
    pub status: Option<u16>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub profile_id: String,
    pub log_tail: String,
    pub token: TokenStatus,
    pub addresses: Addresses,
    pub working_dir: String,
    pub game_config_path: String,
    pub game_config_exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    pub title: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProfileIndex {
    active: String,
    profiles: Vec<String>,
}

// ---------------------------------------------------------------------------
// Paths and defaults
// ---------------------------------------------------------------------------

pub fn profiles_dir() -> PathBuf {
    PathBuf::from(PROFILES_DIR)
}

fn index_path() -> PathBuf {
    profiles_dir().join(INDEX_FILE)
}

pub fn profile_dir(id: &str) -> PathBuf {
    profiles_dir().join(id)
}

pub fn auth_path(id: &str) -> PathBuf {
    profile_dir(id).join(AUTH_FILE)
}

pub fn ini_path(id: &str) -> PathBuf {
    profile_dir(id).join(INI_FILE)
}

pub fn active_auth_path() -> PathBuf {
    auth_path(&active_id())
}

pub fn active_ini_path() -> PathBuf {
    ini_path(&active_id())
}

fn profile_json_path(id: &str) -> PathBuf {
    profile_dir(id).join(PROFILE_FILE)
}

fn validate_id(id: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() || id.len() > 64 {
        return Err("Profile id must be 1-64 characters".to_string());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Profile id may only contain letters, digits, '-' and '_'".to_string());
    }
    Ok(id.to_string())
}

fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "profile".to_string()
    } else {
        slug
    }
}

pub fn default_plugin_files() -> Vec<PluginFile> {
    vec![
        PluginFile {
            url: "https://api.lethelc.site/Lethe.dll".into(),
            filename: "Lethe.dll".into(),
        },
        PluginFile {
            url: "https://api.lethelc.site/ModularSkillScripts.dll".into(),
            filename: "ModularSkillScripts.dll".into(),
        },
        PluginFile {
            url: "https://api.lethelc.site/motions.dll".into(),
            filename: "motions.dll".into(),
        },
    ]
}

impl Profile {
    /// Fill blank fields with the upstream defaults. Called on every load so
    /// older profile.json files keep working when new keys are introduced.
    pub fn with_defaults(mut self, id: &str) -> Self {
        self.id = id.to_string();
        if self.name.trim().is_empty() {
            self.name = id.to_string();
        }
        if self.auth_base_url.trim().is_empty() {
            self.auth_base_url = DEFAULT_AUTH_BASE_URL.into();
        }
        if self.manifest_url.trim().is_empty() {
            self.manifest_url = DEFAULT_MANIFEST_URL.into();
        }
        if self.download_base_url.trim().is_empty() {
            self.download_base_url = DEFAULT_DOWNLOAD_BASE_URL.into();
        }
        if self.plugin_urls.is_empty() {
            self.plugin_urls = default_plugin_files();
        }
        if self.news_url.trim().is_empty() {
            self.news_url = DEFAULT_NEWS_URL.into();
        }
        if self.update_url.trim().is_empty() {
            self.update_url = DEFAULT_UPDATE_URL.into();
        }
        if self.server_root.trim().is_empty() {
            self.server_root = DEFAULT_SERVER_ROOT.into();
        }
        if self.cache_path.trim().is_empty() {
            self.cache_path = DEFAULT_CACHE_PATH.into();
        }
        self
    }

    pub fn new(id: &str, name: &str, profile_type: ProfileType) -> Self {
        let ignore = matches!(profile_type, ProfileType::Private);
        Profile {
            id: id.to_string(),
            name: name.to_string(),
            profile_type,
            server_version: String::new(),
            auth_base_url: String::new(),
            manifest_url: String::new(),
            download_base_url: String::new(),
            plugin_urls: Vec::new(),
            news_url: String::new(),
            update_url: String::new(),
            server_root: String::new(),
            ignore_launcher_token: ignore,
            cache_path: String::new(),
            same_machine: false,
            mods: Vec::new(),
        }
        .with_defaults(id)
    }

    pub fn addresses(&self) -> Addresses {
        Addresses {
            auth_base_url: self.auth_base_url.clone(),
            manifest_url: self.manifest_url.clone(),
            download_base_url: self.download_base_url.clone(),
            news_url: self.news_url.clone(),
            update_url: self.update_url.clone(),
            server_root: self.server_root.clone(),
            plugin_urls: self.plugin_urls.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Index / active profile
// ---------------------------------------------------------------------------

fn read_index() -> Result<ProfileIndex, String> {
    let text = fs::read_to_string(index_path())
        .map_err(|e| format!("Cannot read {}/{}: {}", PROFILES_DIR, INDEX_FILE, e))?;
    serde_json::from_str(&text).map_err(|e| format!("Invalid profiles.json: {}", e))
}

fn write_index(index: &ProfileIndex) -> Result<(), String> {
    fs::create_dir_all(profiles_dir())
        .map_err(|e| format!("Cannot create {}: {}", PROFILES_DIR, e))?;
    let text = serde_json::to_string_pretty(index)
        .map_err(|e| format!("Cannot serialize profiles.json: {}", e))?;
    fs::write(index_path(), text).map_err(|e| format!("Cannot write profiles.json: {}", e))
}

fn active_slot() -> &'static RwLock<String> {
    ACTIVE_ID.get_or_init(|| RwLock::new(String::new()))
}

/// Active profile id. Kept in memory so a stale UI token can never be used
/// after a switch; disk is consulted once on first access.
pub fn active_id() -> String {
    {
        let guard = active_slot().read().unwrap_or_else(|e| e.into_inner());
        if !guard.is_empty() {
            return guard.clone();
        }
    }
    let id = read_index()
        .map(|i| i.active)
        .unwrap_or_else(|_| "local".to_string());
    {
        let mut guard = active_slot().write().unwrap_or_else(|e| e.into_inner());
        if guard.is_empty() {
            *guard = id.clone();
        }
        guard.clone()
    }
}

fn set_active_id(id: &str) {
    let mut guard = active_slot().write().unwrap_or_else(|e| e.into_inner());
    *guard = id.to_string();
}

/// Create the profile layout on first run and migrate the legacy root-level
/// auth / ini files into the default profile. Existing installs are untouched.
pub fn init() -> Result<(), String> {
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    fs::create_dir_all(profiles_dir())
        .map_err(|e| format!("Cannot create {}: {}", PROFILES_DIR, e))?;

    if !index_path().exists() {
        let default_id = "local";
        let profile = Profile::new(default_id, "Local private server", ProfileType::Private);
        save_profile_unlocked(&profile)?;

        // Migration: the old launcher stored its token and settings in the
        // game root. Copy (not move) them so a downgrade still finds them.
        let legacy_auth = Path::new(ROOT_AUTH_FILE);
        if legacy_auth.exists() {
            let _ = fs::copy(legacy_auth, auth_path(default_id));
            crate::lethe_log!("Migrated {} into profile '{}'", ROOT_AUTH_FILE, default_id);
        }
        let legacy_ini = Path::new(ROOT_INI_FILE);
        if legacy_ini.exists() && !ini_path(default_id).exists() {
            let _ = fs::copy(legacy_ini, ini_path(default_id));
        }

        write_index(&ProfileIndex {
            active: default_id.to_string(),
            profiles: vec![default_id.to_string()],
        })?;
    } else {
        let mut index = read_index()?;
        index.profiles.retain(|id| profile_json_path(id).exists());
        if index.profiles.is_empty() {
            let profile = Profile::new("local", "Local private server", ProfileType::Private);
            save_profile_unlocked(&profile)?;
            index.profiles.push("local".to_string());
        }
        if !index.profiles.contains(&index.active) {
            index.active = index.profiles[0].clone();
        }
        write_index(&index)?;
    }

    let id = read_index()?.active;
    set_active_id(&id);
    crate::lethe_log!("Active profile: {}", id);
    Ok(())
}

pub fn list_profiles() -> Result<Vec<Profile>, String> {
    let index = read_index()?;
    let mut out = Vec::with_capacity(index.profiles.len());
    for id in &index.profiles {
        out.push(load_profile(id)?);
    }
    Ok(out)
}

pub fn load_profile(id: &str) -> Result<Profile, String> {
    let id = validate_id(id)?;
    let path = profile_json_path(&id);
    let text =
        fs::read_to_string(&path).map_err(|e| format!("Cannot read profile '{}': {}", id, e))?;
    let profile: Profile =
        serde_json::from_str(&text).map_err(|e| format!("Invalid profile '{}': {}", id, e))?;
    Ok(profile.with_defaults(&id))
}

pub fn active_profile() -> Profile {
    load_profile(&active_id()).unwrap_or_else(|e| {
        crate::lethe_log!("Using fallback profile: {}", e);
        Profile::new("local", "Local private server", ProfileType::Private)
    })
}

pub fn addresses() -> Addresses {
    active_profile().addresses()
}

fn save_profile_unlocked(profile: &Profile) -> Result<(), String> {
    let id = validate_id(&profile.id)?;
    fs::create_dir_all(profile_dir(&id))
        .map_err(|e| format!("Cannot create profile directory: {}", e))?;
    let text = serde_json::to_string_pretty(profile)
        .map_err(|e| format!("Cannot serialize profile: {}", e))?;
    fs::write(profile_json_path(&id), text)
        .map_err(|e| format!("Cannot write profile.json: {}", e))?;

    // The per-profile ini is what the UI's generic settings command writes.
    let mut cfg = Ini::new();
    cfg.with_section(Some("General"))
        .set("Name", profile.name.clone())
        .set("ProfileType", profile.profile_type.as_str().to_string())
        .set("ServerVersion", profile.server_version.clone())
        .set("DisableAutoUpdate", "false");
    cfg.with_section(Some("Addresses"))
        .set("AuthBaseUrl", profile.auth_base_url.clone())
        .set("ManifestUrl", profile.manifest_url.clone())
        .set("DownloadBaseUrl", profile.download_base_url.clone())
        .set("NewsUrl", profile.news_url.clone())
        .set("UpdateUrl", profile.update_url.clone())
        .set("ServerRoot", profile.server_root.clone())
        .set("CachePath", profile.cache_path.clone())
        .set(
            "IgnoreLauncherToken",
            profile.ignore_launcher_token.to_string(),
        );
    cfg.write_to_file(ini_path(&id))
        .map_err(|e| format!("Cannot write profile ini: {}", e))?;
    Ok(())
}

pub fn save_profile(profile: &Profile) -> Result<(), String> {
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    save_profile_unlocked(profile)
}

pub fn create_profile(input: ProfileInput) -> Result<Profile, String> {
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut index = read_index()?;
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }
    let requested = if input.id.trim().is_empty() {
        slugify(&name)
    } else {
        validate_id(&input.id)?
    };
    let mut id = requested.clone();
    let mut suffix = 2;
    while index.profiles.contains(&id) || profile_dir(&id).exists() {
        id = format!("{}-{}", requested, suffix);
        suffix += 1;
    }
    let ignore = input
        .ignore_launcher_token
        .unwrap_or_else(|| matches!(input.profile_type, ProfileType::Private));
    let profile = Profile {
        id: id.clone(),
        name,
        profile_type: input.profile_type,
        server_version: input.server_version,
        auth_base_url: input.auth_base_url,
        manifest_url: input.manifest_url,
        download_base_url: input.download_base_url,
        plugin_urls: input.plugin_urls,
        news_url: input.news_url,
        update_url: input.update_url,
        server_root: input.server_root,
        ignore_launcher_token: ignore,
        cache_path: input.cache_path,
        same_machine: input.same_machine,
        mods: input.mods,
    }
    .with_defaults(&id);
    save_profile_unlocked(&profile)?;
    index.profiles.push(id);
    write_index(&index)?;
    crate::lethe_log!("Created profile '{}' ({})", profile.name, profile.id);
    Ok(profile)
}

pub fn update_profile(id: &str, profile: Profile) -> Result<Profile, String> {
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let id = validate_id(id)?;
    let index = read_index()?;
    if !index.profiles.contains(&id) {
        return Err(format!("Profile '{}' not found", id));
    }
    if profile.id != id {
        return Err("Profile id cannot be changed; create a new profile instead".to_string());
    }
    let profile = profile.with_defaults(&id);
    save_profile_unlocked(&profile)?;
    if active_id() == id {
        sync_compat_files(&profile)?;
        apply_game_config(&profile)?;
    }
    crate::lethe_log!("Updated profile '{}'", id);
    Ok(profile)
}

pub fn delete_profile(id: &str) -> Result<(), String> {
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let id = validate_id(id)?;
    let mut index = read_index()?;
    if !index.profiles.contains(&id) {
        return Err(format!("Profile '{}' not found", id));
    }
    if index.profiles.len() == 1 {
        return Err("Cannot delete the last profile".to_string());
    }
    index.profiles.retain(|p| p != &id);
    if index.active == id {
        index.active = index.profiles[0].clone();
    }
    write_index(&index)?;
    let _ = fs::remove_dir_all(profile_dir(&id));
    if active_id() == id {
        set_active_id(&index.active);
        crate::auth::clear_auth_cache();
        if let Ok(profile) = load_profile(&index.active) {
            let _ = sync_compat_files(&profile);
            let _ = apply_game_config(&profile);
        }
    }
    crate::lethe_log!("Deleted profile '{}'", id);
    Ok(())
}

/// Switch the active profile: update the index and in-memory binding, mirror
/// its auth/ini into the root compatibility files, and rewrite the game config.
pub fn switch_profile(id: &str) -> Result<Profile, String> {
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let id = validate_id(id)?;
    let mut index = read_index()?;
    if !index.profiles.contains(&id) {
        return Err(format!("Profile '{}' not found", id));
    }
    index.active = id.clone();
    write_index(&index)?;
    set_active_id(&id);
    crate::auth::clear_auth_cache();

    let profile = load_profile(&id)?;
    sync_compat_files(&profile)?;
    apply_game_config(&profile)?;
    apply_mods(&profile);
    crate::lethe_log!("Switched to profile '{}' ({})", profile.name, id);
    Ok(profile)
}

/// Root files exist for older tooling that expects them at the game root.
/// They are always overwritten from the active profile so a token can never
/// leak from the previously active profile.
fn sync_compat_files(profile: &Profile) -> Result<(), String> {
    let src_auth = auth_path(&profile.id);
    if src_auth.exists() {
        fs::copy(&src_auth, ROOT_AUTH_FILE).map_err(|e| format!("Cannot sync auth file: {}", e))?;
    } else {
        // No login for this profile: remove the stale one from the old profile.
        let _ = fs::remove_file(ROOT_AUTH_FILE);
    }
    let src_ini = ini_path(&profile.id);
    if src_ini.exists() {
        fs::copy(&src_ini, ROOT_INI_FILE).map_err(|e| format!("Cannot sync ini file: {}", e))?;
    }
    Ok(())
}

pub fn profile_summaries() -> Result<Vec<ProfileSummary>, String> {
    let active = active_id();
    let mut out = Vec::new();
    for profile in list_profiles()? {
        let auth = crate::auth::load_saved_auth_for(&profile.id);
        out.push(ProfileSummary {
            id: profile.id.clone(),
            name: profile.name.clone(),
            profile_type: profile.profile_type,
            active: profile.id == active,
            server_version: profile.server_version.clone(),
            server_root: profile.server_root.clone(),
            logged_in: auth.is_some(),
            username: auth.map(|a| a.username).unwrap_or_default(),
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Lethe.cfg rewriting
// ---------------------------------------------------------------------------

/// Read the game config through rust-ini (parse + validation only). We do not
/// use `Ini::write_to_file` here because rust-ini drops the `##` comments and
/// the long BepInEx setting descriptions; the line writer below preserves them.
fn read_game_cfg(text: &str) -> Result<Ini, String> {
    Ini::load_from_str_noescape(text).map_err(|e| format!("Cannot parse Lethe.cfg: {}", e))
}

/// Rewrite only ServerRoot / IgnoreLauncherToken / CachePath inside [General].
/// Everything else — including comments, ordering and line endings — survives.
pub fn apply_game_config(profile: &Profile) -> Result<(), String> {
    rewrite_game_cfg_at(Path::new(GAME_CFG), profile)
}

/// Same rewrite against an arbitrary path; used by the unit tests so the real
/// implementation (not a copy of it) can be verified against a BepInEx cfg.
pub fn rewrite_game_cfg_at(path: &Path, profile: &Profile) -> Result<(), String> {
    let old = fs::read_to_string(path).unwrap_or_default();
    if !old.trim().is_empty() {
        let parsed = read_game_cfg(&old)?;
        crate::lethe_log!(
            "Lethe.cfg before switch: ServerRoot={}",
            parsed
                .get_from(Some("General"), "ServerRoot")
                .unwrap_or("<unset>")
        );
    }

    let newline = if old.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing_newline = old.ends_with('\n');
    let ignore = profile.ignore_launcher_token.to_string();
    let wanted: Vec<(&str, &str)> = vec![
        ("ServerRoot", profile.server_root.as_str()),
        ("IgnoreLauncherToken", ignore.as_str()),
        ("CachePath", profile.cache_path.as_str()),
    ];
    let mut seen: HashSet<String> = HashSet::new();
    let mut lines: Vec<String> = old.lines().map(|s| s.to_string()).collect();
    let mut in_general = false;
    let mut general_end: Option<usize> = None;

    for (idx, line) in lines.iter_mut().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_general && general_end.is_none() {
                general_end = Some(idx);
            }
            in_general = trimmed[1..trimmed.len() - 1].trim() == "General";
            continue;
        }
        if !in_general {
            continue;
        }
        // Comment lines must not be treated as settings.
        if trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some((key_raw, _)) = trimmed.split_once('=') {
            let key = key_raw.trim().to_string();
            if let Some((_, value)) = wanted.iter().find(|(k, _)| *k == key.as_str()) {
                // Keep the original spacing before the '=' and the indentation.
                let prefix = line
                    .split_once('=')
                    .map(|(p, _)| p.trim_end().to_string())
                    .unwrap_or_else(|| key.clone());
                *line = format!("{}= {}", prefix, value);
                seen.insert(key);
            }
        }
    }

    if seen.len() < wanted.len() {
        let mut insert_at = general_end.unwrap_or(lines.len());
        if general_end.is_none() {
            // No [General] section at all: create one at the top.
            lines.insert(0, "[General]".to_string());
            insert_at = 1;
        }
        let mut additions = Vec::new();
        for (key, value) in &wanted {
            if !seen.contains(*key) {
                additions.push(format!("{} = {}", key, value));
            }
        }
        for (offset, addition) in additions.into_iter().enumerate() {
            lines.insert(insert_at + offset, addition);
        }
    }

    let mut content = lines.join(newline);
    if trailing_newline {
        content.push_str(newline);
    }
    // Validate our own output before touching the real file.
    read_game_cfg(&content)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create config dir: {}", e))?;
    }
    fs::write(path, content).map_err(|e| format!("Cannot write Lethe.cfg: {}", e))?;
    crate::lethe_log!(
        "Lethe.cfg updated: ServerRoot={} IgnoreLauncherToken={} CachePath={}",
        profile.server_root,
        ignore,
        profile.cache_path
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Same-machine helper, mods, connection test, diagnostics
// ---------------------------------------------------------------------------

/// Configure a profile for a local server instance on `port`. Disabling the
/// helper restores the upstream defaults (keeping the profile's name/type).
pub fn apply_same_machine(id: &str, port: u16, enabled: bool) -> Result<Profile, String> {
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut profile = load_profile(id)?;
    if enabled {
        let base = format!("http://127.0.0.1:{}", port);
        profile.auth_base_url = base.clone();
        profile.server_root = format!("{}/", base);
        profile.manifest_url = format!("{}/lethe-manifest.json", base);
        profile.download_base_url = format!("{}/download/", base);
        profile.ignore_launcher_token = true;
        profile.cache_path = DEFAULT_CACHE_PATH.into();
        profile.news_url = String::new();
    } else {
        profile.auth_base_url = DEFAULT_AUTH_BASE_URL.into();
        profile.server_root = DEFAULT_SERVER_ROOT.into();
        profile.manifest_url = DEFAULT_MANIFEST_URL.into();
        profile.download_base_url = DEFAULT_DOWNLOAD_BASE_URL.into();
        profile.ignore_launcher_token = false;
        profile.news_url = DEFAULT_NEWS_URL.into();
    }
    profile.same_machine = enabled;
    let id = profile.id.clone();
    profile = profile.with_defaults(&id);
    save_profile_unlocked(&profile)?;
    if active_id() == profile.id {
        sync_compat_files(&profile)?;
        apply_game_config(&profile)?;
    }
    Ok(profile)
}

/// Apply the profile's mod enable/disable list to the on-disk folder names.
/// The mod folder convention is shared with `commands::get_mods`/`toggle_mod`.
pub fn apply_mods(profile: &Profile) {
    let mods_dir = Path::new(MODS_DIR);
    for state in &profile.mods {
        if state.name.is_empty() || state.name.contains('/') || state.name.contains('\\') {
            continue;
        }
        let enabled = mods_dir.join(&state.name);
        let disabled = mods_dir.join(format!("DISABLED_{}", state.name));
        let full = mods_dir.join(format!("FULLDISABLED_{}", state.name));
        let result = if state.enabled {
            if !enabled.exists() {
                if disabled.exists() {
                    fs::rename(&disabled, &enabled)
                } else if full.exists() {
                    fs::rename(&full, &enabled)
                } else {
                    continue;
                }
            } else {
                continue;
            }
        } else if enabled.exists() {
            fs::rename(&enabled, &disabled)
        } else {
            continue;
        };
        if let Err(e) = result {
            crate::lethe_log!("Failed to apply mod '{}': {}", state.name, e);
        }
    }
}

/// Remember a mod toggle in the active profile so it is restored on switch.
pub fn remember_mod_state(name: &str, enabled: bool) {
    let id = active_id();
    let _guard = io_lock().lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut profile) = load_profile(&id) {
        if let Some(state) = profile.mods.iter_mut().find(|m| m.name == name) {
            state.enabled = enabled;
        } else {
            profile.mods.push(ModState {
                name: name.to_string(),
                enabled,
            });
        }
        let _ = save_profile_unlocked(&profile);
    }
}

fn health_url(input: &str) -> String {
    let mut url = input.trim().trim_end_matches('/').to_string();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("http://{}", url);
    }
    if url.ends_with("/health") {
        url
    } else {
        format!("{}/health", url)
    }
}

/// `GET <base>/health` with a short timeout. Network failures are returned as
/// a failed result rather than an Err so the UI can show the attempted URL.
pub async fn test_connection(base: String) -> ConnectionTest {
    let url = health_url(&base);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let start = Instant::now();
    let response = match client {
        Ok(c) => c.get(&url).send().await,
        Err(e) => Err(e),
    };
    match response {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let ok = resp.status().is_success();
            ConnectionTest {
                url,
                ok,
                status: Some(status),
                latency_ms: Some(start.elapsed().as_millis() as u64),
                error: if ok {
                    None
                } else {
                    Some(format!("HTTP {}", status))
                },
            }
        }
        Err(e) => ConnectionTest {
            url,
            ok: false,
            status: None,
            latency_ms: Some(start.elapsed().as_millis() as u64),
            error: Some(e.to_string()),
        },
    }
}

pub fn diagnostics() -> Diagnostics {
    let profile = active_profile();
    let log_tail = fs::read_to_string(LOG_FILE)
        .unwrap_or_default()
        .lines()
        .rev()
        .take(80)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let cfg = Path::new(GAME_CFG);
    Diagnostics {
        profile_id: profile.id.clone(),
        log_tail,
        token: crate::auth::token_status(),
        addresses: profile.addresses(),
        working_dir: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        game_config_path: cfg.display().to_string(),
        game_config_exists: cfg.exists(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_id() {
        assert_eq!(validate_id(" local-1 ").unwrap(), "local-1");
        assert!(validate_id("../evil").is_err());
        assert!(validate_id("has space").is_err());
        assert!(validate_id("").is_err());
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("My Local Server!"), "my-local-server");
        assert_eq!(slugify("!!!"), "profile");
    }

    #[test]
    fn test_defaults_are_applied() {
        let p = Profile::new("x", "", ProfileType::Private);
        assert_eq!(p.auth_base_url, DEFAULT_AUTH_BASE_URL);
        assert_eq!(p.manifest_url, DEFAULT_MANIFEST_URL);
        assert_eq!(p.download_base_url, DEFAULT_DOWNLOAD_BASE_URL);
        assert_eq!(p.plugin_urls.len(), 3);
        assert!(p.ignore_launcher_token);
    }

    #[test]
    fn test_profile_json_shape_matches_frontend() {
        let profile = Profile::new("local", "Local", ProfileType::Private);
        let json = serde_json::to_value(&profile).unwrap();
        assert_eq!(json["type"], "private");
        assert!(json["authBaseUrl"].is_string());
        assert!(json["pluginUrls"].is_array());
        assert!(json.get("profileType").is_none());

        // The admin panel sends `type`, not the Rust field name.
        let input: ProfileInput = serde_json::from_str(
            r#"{"name":"Local","type":"private","authBaseUrl":"http://127.0.0.1:8080"}"#,
        )
        .unwrap();
        assert_eq!(input.profile_type, ProfileType::Private);
        assert_eq!(input.auth_base_url, "http://127.0.0.1:8080");
    }

    #[test]
    fn test_health_url() {
        assert_eq!(
            health_url("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/health"
        );
        assert_eq!(health_url("example.com"), "http://example.com/health");
        assert_eq!(health_url("http://x/health"), "http://x/health");
    }

    #[test]
    fn test_game_cfg_rewrite_preserves_other_settings_and_comments() {
        let original = "## header\n[General]\n\n# comment\nLogLevel = 2\nServerRoot = https://old/\nShowLogo = true\nCachePath = ./OldCache\n\n[LaunchSettings]\nCRCToggle = false\n";
        let parsed = Ini::load_from_str_noescape(original).unwrap();
        assert_eq!(
            parsed.get_from(Some("General"), "ServerRoot"),
            Some("https://old/")
        );

        let profile = Profile {
            server_root: "http://127.0.0.1:8080/".into(),
            ignore_launcher_token: true,
            cache_path: "./NewCache".into(),
            ..Profile::new("t", "t", ProfileType::Private)
        };
        let path = std::env::temp_dir().join(format!(
            "lethe-cfg-test-{}-{}.cfg",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, original).unwrap();
        rewrite_game_cfg_at(&path, &profile).unwrap();
        let result = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        Ini::load_from_str_noescape(&result).unwrap();
        assert!(result.contains("## header"));
        assert!(result.contains("LogLevel = 2"));
        assert!(result.contains("# comment"));
        assert!(result.contains("ServerRoot= http://127.0.0.1:8080/"));
        assert!(result.contains("CachePath= ./NewCache"));
        assert!(result.contains("IgnoreLauncherToken = true"));
        assert!(result.contains("CRCToggle = false"));
        assert!(!result.contains("https://old/"));

        // A second switch must update the same three keys in place.
        let profile2 = Profile {
            server_root: "https://api.lethelc.site/".into(),
            ignore_launcher_token: false,
            cache_path: "./LetheCache".into(),
            ..Profile::new("t2", "t2", ProfileType::Official)
        };
        fs::write(&path, &result).unwrap();
        rewrite_game_cfg_at(&path, &profile2).unwrap();
        let result2 = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);
        let parsed2 = Ini::load_from_str_noescape(&result2).unwrap();
        assert_eq!(
            parsed2.get_from(Some("General"), "ServerRoot"),
            Some("https://api.lethelc.site/")
        );
        assert_eq!(
            parsed2.get_from(Some("General"), "IgnoreLauncherToken"),
            Some("false")
        );
        assert_eq!(
            parsed2.get_from(Some("General"), "CachePath"),
            Some("./LetheCache")
        );
        assert_eq!(
            parsed2.get_from(Some("LaunchSettings"), "CRCToggle"),
            Some("false")
        );
    }
}
