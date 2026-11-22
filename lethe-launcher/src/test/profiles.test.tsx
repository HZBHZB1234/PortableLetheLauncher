import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { SettingsPanel } from '../components/SettingsPanel';
import type { Profile, Diagnostics } from '../types/profile';

const profile: Profile = {
  id: 'local',
  name: 'Local server',
  type: 'private',
  serverVersion: '1.2.4',
  authBaseUrl: 'http://127.0.0.1:8080',
  manifestUrl: 'http://127.0.0.1:8080/lethe-manifest.json',
  downloadBaseUrl: 'http://127.0.0.1:8080/download/',
  pluginUrls: [{ url: 'http://127.0.0.1:8080/Lethe.dll', filename: 'Lethe.dll' }],
  newsUrl: '',
  updateUrl: '',
  serverRoot: 'http://127.0.0.1:8080/',
  ignoreLauncherToken: true,
  cachePath: './LetheCache',
  sameMachine: true,
  mods: [],
};

const diagnostics: Diagnostics = {
  profileId: 'local',
  logTail: '[10:00:00] Switched to profile local',
  token: {
    profileId: 'local',
    isLoggedIn: true,
    username: 'Tester',
    avatarUrl: '',
    expired: false,
  },
  addresses: {
    authBaseUrl: profile.authBaseUrl,
    manifestUrl: profile.manifestUrl,
    downloadBaseUrl: profile.downloadBaseUrl,
    newsUrl: '',
    updateUrl: '',
    serverRoot: profile.serverRoot,
    pluginUrls: profile.pluginUrls,
  },
  workingDir: 'C:\\Game',
  gameConfigPath: 'BepInEx/config/Lethe.cfg',
  gameConfigExists: true,
};

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (command: string) => {
    switch (command) {
      case 'profile_list':
        return [
          {
            id: 'local',
            name: 'Local server',
            type: 'private',
            active: true,
            serverVersion: '1.2.4',
            serverRoot: profile.serverRoot,
            loggedIn: true,
            username: 'Tester',
          },
          {
            id: 'official',
            name: 'Official',
            type: 'official',
            active: false,
            serverVersion: '',
            serverRoot: 'https://api.lethelc.site/',
            loggedIn: false,
            username: '',
          },
        ];
      case 'profile_get':
        return profile;
      case 'profile_addresses':
        return diagnostics.addresses;
      case 'get_diagnostics':
        return diagnostics;
      default:
        return null;
    }
  }),
}));

describe('SettingsPanel admin tabs', () => {
  beforeEach(() => vi.clearAllMocks());

  it('lists profiles and marks the active one', async () => {
    render(
      <SettingsPanel
        isOpen={true}
        skipValidation={false}
        onToggleValidation={() => {}}
        onSwitched={async () => {}}
        auth={null}
        onLogin={async () => {}}
        onLogout={async () => {}}
        onClose={() => {}}
      />
    );
    expect(screen.getByText('Admin Panel')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText('Local server')).toBeInTheDocument());
    expect(screen.getByText('ACTIVE')).toBeInTheDocument();
    expect(screen.getByText('Official')).toBeInTheDocument();
    expect(screen.getByText('Switch')).toBeInTheDocument();
  });

  it('shows token status without revealing the token in diagnostics', async () => {
    render(
      <SettingsPanel
        isOpen={true}
        skipValidation={false}
        onToggleValidation={() => {}}
        onSwitched={async () => {}}
        auth={null}
        onLogin={async () => {}}
        onLogout={async () => {}}
        onClose={() => {}}
      />
    );
    fireEvent.click(screen.getByText('Diagnostics'));
    await waitFor(() => expect(screen.getByText(/valid/)).toBeInTheDocument());
    expect(screen.getByText(/Tester/)).toBeInTheDocument();
    expect(screen.getByText(/Switched to profile local/)).toBeInTheDocument();
    expect(screen.queryByText(/eyJ/)).not.toBeInTheDocument();
  });
});
