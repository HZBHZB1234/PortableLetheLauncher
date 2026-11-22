export type ProfileType = 'private' | 'official';

export interface PluginFile {
  url: string;
  filename: string;
}

export interface ModState {
  name: string;
  enabled: boolean;
}

/** Full per-profile configuration (mirrors Rust `profiles::Profile`). */
export interface Profile {
  id: string;
  name: string;
  profileType: ProfileType;
  serverVersion: string;
  authBaseUrl: string;
  manifestUrl: string;
  downloadBaseUrl: string;
  pluginUrls: PluginFile[];
  newsUrl: string;
  updateUrl: string;
  serverRoot: string;
  ignoreLauncherToken: boolean;
  cachePath: string;
  sameMachine: boolean;
  mods: ModState[];
}

/** Fields accepted by `profile_create`; blank addresses get sane defaults. */
export interface ProfileInput {
  id?: string;
  name: string;
  profileType?: ProfileType;
  serverVersion?: string;
  authBaseUrl?: string;
  manifestUrl?: string;
  downloadBaseUrl?: string;
  pluginUrls?: PluginFile[];
  newsUrl?: string;
  updateUrl?: string;
  serverRoot?: string;
  ignoreLauncherToken?: boolean;
  cachePath?: string;
  sameMachine?: boolean;
  mods?: ModState[];
}

export interface ProfileSummary {
  id: string;
  name: string;
  profileType: ProfileType;
  active: boolean;
  serverVersion: string;
  serverRoot: string;
  loggedIn: boolean;
  username: string;
}

export interface Addresses {
  authBaseUrl: string;
  manifestUrl: string;
  downloadBaseUrl: string;
  newsUrl: string;
  updateUrl: string;
  serverRoot: string;
  pluginUrls: PluginFile[];
}

/** Never contains the token itself — safe for the diagnostics panel. */
export interface TokenStatus {
  profileId: string;
  isLoggedIn: boolean;
  username: string;
  avatarUrl: string;
  expired: boolean;
}

export interface ConnectionTest {
  url: string;
  ok: boolean;
  status: number | null;
  latencyMs: number | null;
  error: string | null;
}

export interface Diagnostics {
  profileId: string;
  logTail: string;
  token: TokenStatus;
  addresses: Addresses;
  workingDir: string;
  gameConfigPath: string;
  gameConfigExists: boolean;
}

export interface NewsItem {
  title: string;
  date: string;
  url: string;
  body: string;
}
