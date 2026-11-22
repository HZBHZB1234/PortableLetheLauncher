import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type {
  Addresses,
  ConnectionTest,
  Diagnostics,
  Profile,
  ProfileInput,
  ProfileSummary,
} from '../types/profile';

interface UseProfilesReturn {
  profiles: ProfileSummary[];
  selectedId: string;
  setSelectedId: (id: string) => void;
  activeId: string;
  busy: boolean;
  error: string | null;
  notice: string | null;
  addresses: Addresses | null;
  diagnostics: Diagnostics | null;
  connection: ConnectionTest | null;
  refresh: () => Promise<void>;
  selectProfile: (id: string) => Promise<void>;
  createProfile: (input: ProfileInput) => Promise<Profile | null>;
  updateProfile: (profile: Profile) => Promise<Profile | null>;
  deleteProfile: (id: string) => Promise<boolean>;
  switchProfile: (id: string) => Promise<boolean>;
  testConnection: (url: string) => Promise<ConnectionTest | null>;
  applySameMachine: (id: string, port: number, enabled: boolean) => Promise<boolean>;
  refreshDiagnostics: () => Promise<void>;
  clearNotice: () => void;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function useProfiles(): UseProfilesReturn {
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [selectedId, setSelectedId] = useState('');
  const [activeId, setActiveId] = useState('');
  const [addresses, setAddresses] = useState<Addresses | null>(null);
  const [diagnostics, setDiagnostics] = useState<Diagnostics | null>(null);
  const [connection, setConnection] = useState<ConnectionTest | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refreshAddresses = useCallback(async () => {
    try {
      setAddresses(await invoke<Addresses>('profile_addresses'));
    } catch (e) {
      console.error('Failed to load addresses:', e);
    }
  }, []);

  const refreshDiagnostics = useCallback(async () => {
    try {
      setDiagnostics(await invoke<Diagnostics>('get_diagnostics'));
    } catch (e) {
      console.error('Failed to load diagnostics:', e);
    }
  }, []);

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<ProfileSummary[]>('profile_list');
      setProfiles(list);
      const active = list.find((p) => p.active)?.id ?? '';
      setActiveId(active);
      setSelectedId((current) => {
        if (current && list.some((p) => p.id === current)) return current;
        return active || list[0]?.id || '';
      });
      await refreshAddresses();
      await refreshDiagnostics();
      setError(null);
    } catch (e) {
      setError(messageOf(e));
    }
  }, [refreshAddresses, refreshDiagnostics]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  /** Select a profile in the editor without switching the active one. */
  const selectProfile = useCallback(
    async (id: string) => {
      setSelectedId(id);
      setConnection(null);
      setNotice(null);
    },
    []
  );

  const createProfile = useCallback(
    async (input: ProfileInput): Promise<Profile | null> => {
      setBusy(true);
      setError(null);
      try {
        const created = await invoke<Profile>('profile_create', { input });
        setNotice(`Created profile '${created.name}'`);
        setSelectedId(created.id);
        await refresh();
        return created;
      } catch (e) {
        setError(messageOf(e));
        return null;
      } finally {
        setBusy(false);
      }
    },
    [refresh]
  );

  const updateProfile = useCallback(
    async (profile: Profile): Promise<Profile | null> => {
      setBusy(true);
      setError(null);
      try {
        const saved = await invoke<Profile>('profile_update', {
          id: profile.id,
          profile,
        });
        setNotice(`Saved profile '${saved.name}'`);
        await refresh();
        return saved;
      } catch (e) {
        setError(messageOf(e));
        return null;
      } finally {
        setBusy(false);
      }
    },
    [refresh]
  );

  const deleteProfile = useCallback(
    async (id: string): Promise<boolean> => {
      setBusy(true);
      setError(null);
      try {
        await invoke('profile_delete', { id });
        setNotice('Profile deleted');
        setSelectedId('');
        await refresh();
        return true;
      } catch (e) {
        setError(messageOf(e));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [refresh]
  );

  const switchProfile = useCallback(
    async (id: string): Promise<boolean> => {
      setBusy(true);
      setError(null);
      try {
        const profile = await invoke<Profile>('profile_switch', { id });
        setNotice(`Switched to '${profile.name}'`);
        setSelectedId(id);
        await refresh();
        return true;
      } catch (e) {
        setError(messageOf(e));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [refresh]
  );

  const testConnection = useCallback(async (url: string): Promise<ConnectionTest | null> => {
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<ConnectionTest>('profile_test_connection', { base: url });
      setConnection(result);
      return result;
    } catch (e) {
      setError(messageOf(e));
      return null;
    } finally {
      setBusy(false);
    }
  }, []);

  const applySameMachine = useCallback(
    async (id: string, port: number, enabled: boolean): Promise<boolean> => {
      setBusy(true);
      setError(null);
      try {
        await invoke('profile_apply_same_machine', { id, port, enabled });
        setNotice(enabled ? `Local server addresses set to port ${port}` : 'Restored default addresses');
        await refresh();
        return true;
      } catch (e) {
        setError(messageOf(e));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [refresh]
  );

  const clearNotice = useCallback(() => setNotice(null), []);

  return {
    profiles,
    selectedId,
    setSelectedId,
    activeId,
    busy,
    error,
    notice,
    addresses,
    diagnostics,
    connection,
    refresh,
    selectProfile,
    createProfile,
    updateProfile,
    deleteProfile,
    switchProfile,
    testConnection,
    applySameMachine,
    refreshDiagnostics,
    clearNotice,
  };
}
