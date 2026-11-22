import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Profile, ProfileInput, ProfileType } from '../types/profile';
import { useProfiles } from '../hooks/useProfiles';
import type { AuthResult } from '../types/auth';

interface ProfileManagerProps {
  /** Called after a successful switch so the app can drop stale auth state. */
  onSwitched: () => Promise<void>;
  auth: AuthResult | null;
  onLogin: () => Promise<void>;
  onLogout: () => Promise<void>;
}

const DEFAULT_FORM: ProfileInput = {
  name: '',
  profileType: 'private',
  serverVersion: '',
  authBaseUrl: '',
  manifestUrl: '',
  downloadBaseUrl: '',
  newsUrl: '',
  updateUrl: '',
  serverRoot: '',
  cachePath: '',
  ignoreLauncherToken: true,
};

function pluginsToText(profile: Profile): string {
  return profile.pluginUrls.map((p) => `${p.url}|${p.filename}`).join('\n');
}

function textToPlugins(text: string): { url: string; filename: string }[] {
  return text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const [url, filename] = line.split('|').map((v) => v.trim());
      return {
        url,
        filename: filename || url.split('/').pop() || 'plugin.dll',
      };
    });
}

const inputClass =
  'w-full bg-black/40 border border-stone-800 px-2 py-1 text-[11px] text-stone-300 font-mono focus:border-red-900 outline-none';
const labelClass = 'block text-[10px] uppercase tracking-[0.15em] text-stone-600 mb-1';

export function ProfileManager({ onSwitched, auth, onLogin, onLogout }: ProfileManagerProps) {
  const {
    profiles,
    selectedId,
    selectProfile,
    activeId,
    busy,
    error,
    notice,
    connection,
    createProfile,
    updateProfile,
    deleteProfile,
    switchProfile,
    testConnection,
    applySameMachine,
    refreshDiagnostics,
    clearNotice,
  } = useProfiles();

  const [form, setForm] = useState<ProfileInput>(DEFAULT_FORM);
  const [pluginsText, setPluginsText] = useState('');
  const [port, setPort] = useState(8080);
  const [sameMachine, setSameMachine] = useState(false);
  const [editing, setEditing] = useState(false);
  const [loadingForm, setLoadingForm] = useState(false);

  const selected = profiles.find((p) => p.id === selectedId) ?? null;
  const isCreating = editing && !selectedId;

  useEffect(() => {
    if (!selectedId || isCreating) return;
    let cancelled = false;
    setLoadingForm(true);
    invoke<Profile>('profile_get', { id: selectedId })
      .then((profile) => {
        if (cancelled) return;
        setForm({ ...profile });
        setPluginsText(pluginsToText(profile));
        setSameMachine(profile.sameMachine);
      })
      .catch((e) => console.error('Failed to load profile:', e))
      .finally(() => {
        if (!cancelled) setLoadingForm(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedId, isCreating]);

  function startCreate() {
    setEditing(true);
    setForm({ ...DEFAULT_FORM });
    setPluginsText('');
    setSameMachine(false);
  }

  function startEdit() {
    setEditing(true);
  }

  async function save() {
    const input: ProfileInput = {
      ...form,
      pluginUrls: textToPlugins(pluginsText),
      ignoreLauncherToken: form.ignoreLauncherToken ?? true,
    };
    let saved: Profile | null;
    if (selectedId) {
      let current: Profile;
      try {
        current = await invoke<Profile>('profile_get', { id: selectedId });
      } catch (e) {
        console.error('Failed to load profile for save:', e);
        return;
      }
      const updated: Profile = {
        ...current,
        name: form.name || current.name,
        profileType: form.profileType ?? current.profileType,
        serverVersion: form.serverVersion ?? '',
        authBaseUrl: form.authBaseUrl ?? '',
        manifestUrl: form.manifestUrl ?? '',
        downloadBaseUrl: form.downloadBaseUrl ?? '',
        newsUrl: form.newsUrl ?? '',
        updateUrl: form.updateUrl ?? '',
        serverRoot: form.serverRoot ?? '',
        cachePath: form.cachePath ?? '',
        pluginUrls: input.pluginUrls ?? current.pluginUrls,
        ignoreLauncherToken: input.ignoreLauncherToken ?? true,
        sameMachine,
      };
      saved = await updateProfile(updated);
    } else {
      saved = await createProfile(input);
    }
    if (saved) {
      setEditing(false);
      await refreshDiagnostics();
    }
  }

  async function doSwitch(id: string) {
    if (await switchProfile(id)) {
      await onSwitched();
      await refreshDiagnostics();
    }
  }

  async function doSameMachine() {
    const id = selectedId || activeId;
    if (!id) return;
    if (await applySameMachine(id, port, sameMachine)) {
      // Reload the editor to show the helper's generated addresses.
      const profile = await invoke<Profile>('profile_get', { id });
      setForm({ ...profile });
    }
  }

  return (
    <div className="flex flex-col gap-3 text-stone-400">
      <div className="flex items-center justify-between">
        <p className="text-[10px] uppercase tracking-[0.2em] text-stone-500">
          Profiles ({profiles.length}) · active: {activeId || 'none'}
        </p>
        <button
          onClick={startCreate}
          className="px-2 py-1 text-[10px] uppercase tracking-[0.15em] border border-stone-800 hover:border-red-900 text-stone-500 hover:text-red-400 cursor-pointer"
        >
          + New
        </button>
      </div>

      {/* Profile list */}
      <div className="space-y-1 max-h-40 overflow-y-auto pr-1">
        {profiles.map((p) => (
          <div
            key={p.id}
            className={`flex items-center justify-between px-2 py-1.5 border cursor-pointer transition-colors ${
              p.id === selectedId
                ? 'border-red-900/50 bg-red-950/10'
                : 'border-stone-900 hover:border-stone-700'
            }`}
            onClick={() => void selectProfile(p.id)}
          >
            <div className="min-w-0">
              <p className="text-xs text-stone-300 truncate">
                {p.name}
                {p.active && <span className="ml-2 text-[9px] text-emerald-500">ACTIVE</span>}
              </p>
              <p className="text-[9px] text-stone-600 truncate font-mono">
                {p.profileType} · {p.serverRoot}
              </p>
            </div>
            <div className="flex items-center gap-1 shrink-0">
              {p.loggedIn && <span className="text-[9px] text-sky-500">token</span>}
              {!p.active && (
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    void doSwitch(p.id);
                  }}
                  className="px-1.5 py-0.5 text-[9px] uppercase tracking-wider border border-stone-800 hover:border-emerald-800 hover:text-emerald-400 cursor-pointer"
                >
                  Switch
                </button>
              )}
            </div>
          </div>
        ))}
        {profiles.length === 0 && (
          <p className="text-[11px] text-stone-600 italic py-2">No profiles yet.</p>
        )}
      </div>

      {(error || notice) && (
        <p className={`text-[10px] ${error ? 'text-red-400' : 'text-emerald-500'}`}>
          {error ?? notice}
          {notice && !error && (
            <button onClick={clearNotice} className="ml-2 text-stone-600 hover:text-stone-400">
              ×
            </button>
          )}
        </p>
      )}

      {/* Editor */}
      {editing && (
        <div className="border-t border-stone-800 pt-3 flex flex-col gap-2">
          <p className="text-[10px] uppercase tracking-[0.2em] text-stone-500">
            {isCreating ? 'New profile' : `Edit ${selected?.name ?? selectedId}`}
          </p>
          {loadingForm && <p className="text-[10px] text-stone-600">Loading…</p>}
          <div className="grid grid-cols-2 gap-2">
            <div>
              <label className={labelClass}>Name</label>
              <input
                className={inputClass}
                value={form.name ?? ''}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </div>
            <div>
              <label className={labelClass}>Type</label>
              <select
                className={inputClass}
                value={form.profileType ?? 'private'}
                onChange={(e) =>
                  setForm({ ...form, profileType: e.target.value as ProfileType })
                }
              >
                <option value="private">private</option>
                <option value="official">official</option>
              </select>
            </div>
          </div>
          <div>
            <label className={labelClass}>Auth base URL (login + token poll)</label>
            <input
              className={inputClass}
              value={form.authBaseUrl ?? ''}
              onChange={(e) => setForm({ ...form, authBaseUrl: e.target.value })}
              placeholder="https://api.lethelc.site"
            />
          </div>
          <div>
            <label className={labelClass}>Server root (game + /health test)</label>
            <input
              className={inputClass}
              value={form.serverRoot ?? ''}
              onChange={(e) => setForm({ ...form, serverRoot: e.target.value })}
              placeholder="http://127.0.0.1:8080/"
            />
          </div>
          <div>
            <label className={labelClass}>Manifest URL</label>
            <input
              className={inputClass}
              value={form.manifestUrl ?? ''}
              onChange={(e) => setForm({ ...form, manifestUrl: e.target.value })}
            />
          </div>
          <div>
            <label className={labelClass}>Download base URL</label>
            <input
              className={inputClass}
              value={form.downloadBaseUrl ?? ''}
              onChange={(e) => setForm({ ...form, downloadBaseUrl: e.target.value })}
            />
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <label className={labelClass}>News URL (blank = off)</label>
              <input
                className={inputClass}
                value={form.newsUrl ?? ''}
                onChange={(e) => setForm({ ...form, newsUrl: e.target.value })}
              />
            </div>
            <div>
              <label className={labelClass}>Update feed URL</label>
              <input
                className={inputClass}
                value={form.updateUrl ?? ''}
                onChange={(e) => setForm({ ...form, updateUrl: e.target.value })}
              />
            </div>
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <label className={labelClass}>Cache path</label>
              <input
                className={inputClass}
                value={form.cachePath ?? ''}
                onChange={(e) => setForm({ ...form, cachePath: e.target.value })}
              />
            </div>
            <div>
              <label className={labelClass}>Server version</label>
              <input
                className={inputClass}
                value={form.serverVersion ?? ''}
                onChange={(e) => setForm({ ...form, serverVersion: e.target.value })}
              />
            </div>
          </div>
          <div>
            <label className={labelClass}>Plugin DLLs (url|filename, one per line)</label>
            <textarea
              className={`${inputClass} h-16 resize-none`}
              value={pluginsText}
              onChange={(e) => setPluginsText(e.target.value)}
            />
          </div>
          <label className="flex items-center gap-2 text-[10px] text-stone-500 cursor-pointer">
            <input
              type="checkbox"
              checked={form.ignoreLauncherToken ?? true}
              onChange={(e) => setForm({ ...form, ignoreLauncherToken: e.target.checked })}
            />
            IgnoreLauncherToken (game logs in against ServerRoot)
          </label>

          {/* Same-machine helper */}
          <div className="border border-stone-800 p-2 flex flex-col gap-2">
            <label className="flex items-center gap-2 text-[10px] text-stone-500 cursor-pointer">
              <input
                type="checkbox"
                checked={sameMachine}
                onChange={(e) => setSameMachine(e.target.checked)}
              />
              Same-machine mode (server runs on this PC)
            </label>
            <div className="flex items-center gap-2">
              <span className="text-[10px] text-stone-600">Port</span>
              <input
                className={`${inputClass} w-20`}
                type="number"
                min={1}
                max={65535}
                value={port}
                onChange={(e) => setPort(Number(e.target.value))}
              />
              <button
                onClick={() => void doSameMachine()}
                disabled={busy}
                className="px-2 py-1 text-[10px] uppercase tracking-wider border border-stone-800 hover:border-amber-800 hover:text-amber-400 cursor-pointer disabled:opacity-50"
              >
                Apply to profile
              </button>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={() => void save()}
              disabled={busy || !(form.name ?? '').trim()}
              className="flex-1 py-1.5 border border-red-900/60 text-red-400 hover:bg-red-950/20 uppercase tracking-[0.15em] text-[10px] cursor-pointer disabled:opacity-50"
            >
              Save
            </button>
            <button
              onClick={() => {
                setEditing(false);
                clearNotice();
              }}
              className="px-3 py-1.5 border border-stone-800 text-stone-500 hover:text-stone-300 uppercase tracking-[0.15em] text-[10px] cursor-pointer"
            >
              Cancel
            </button>
            {!isCreating && selectedId && (
              <button
                onClick={() => void deleteProfile(selectedId)}
                disabled={busy || profiles.length <= 1}
                className="px-3 py-1.5 border border-stone-800 text-stone-600 hover:border-red-900 hover:text-red-500 uppercase tracking-[0.15em] text-[10px] cursor-pointer disabled:opacity-40"
              >
                Delete
              </button>
            )}
          </div>
        </div>
      )}

      {!editing && selected && (
        <div className="flex items-center gap-2">
          <button
            onClick={startEdit}
            className="px-2 py-1 text-[10px] uppercase tracking-wider border border-stone-800 hover:border-stone-600 cursor-pointer"
          >
            Edit selected
          </button>
          <button
            onClick={() => void testConnection(form.authBaseUrl || selected.serverRoot)}
            disabled={busy}
            className="px-2 py-1 text-[10px] uppercase tracking-wider border border-stone-800 hover:border-sky-800 hover:text-sky-400 cursor-pointer disabled:opacity-50"
          >
            Test connection
          </button>
        </div>
      )}

      {connection && (
        <p
          className={`text-[10px] font-mono break-all ${
            connection.ok ? 'text-emerald-500' : 'text-red-400'
          }`}
        >
          {connection.ok ? 'OK' : 'FAIL'} · {connection.url} · HTTP {connection.status ?? '—'} ·{' '}
          {connection.latencyMs ?? '—'} ms
          {connection.error ? ` · ${connection.error}` : ''}
        </p>
      )}

      {/* Account for the active profile */}
      <div className="border-t border-stone-800 pt-3">
        <p className="text-[10px] uppercase tracking-[0.2em] text-stone-500 mb-2">
          Account · active profile
        </p>
        <p className="text-[11px] text-stone-400 mb-2">
          {auth ? `Signed in as ${auth.username}` : 'Not signed in'}
        </p>
        <div className="flex gap-2">
          <button
            onClick={() => void onLogin()}
            className="px-2 py-1 text-[10px] uppercase tracking-wider border border-stone-800 hover:border-red-900 hover:text-red-400 cursor-pointer"
          >
            Login
          </button>
          <button
            onClick={() => void onLogout()}
            disabled={!auth}
            className="px-2 py-1 text-[10px] uppercase tracking-wider border border-stone-800 hover:border-stone-600 cursor-pointer disabled:opacity-40"
          >
            Logout
          </button>
        </div>
      </div>
    </div>
  );
}
