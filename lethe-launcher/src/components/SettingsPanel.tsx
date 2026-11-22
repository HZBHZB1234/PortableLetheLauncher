import { useEffect, useState } from 'react';
import type { AuthResult } from '../types/auth';
import { ProfileManager } from './ProfileManager';
import { DiagnosticsPanel } from './DiagnosticsPanel';
import { useProfiles } from '../hooks/useProfiles';

interface SettingsPanelProps {
  isOpen: boolean;
  skipValidation: boolean;
  onToggleValidation: () => void;
  onSwitched: () => Promise<void>;
  auth: AuthResult | null;
  onLogin: () => Promise<void>;
  onLogout: () => Promise<void>;
  onClose: () => void;
}

type Tab = 'servers' | 'general' | 'diagnostics';

export function SettingsPanel({
  isOpen,
  skipValidation,
  onToggleValidation,
  onSwitched,
  auth,
  onLogin,
  onLogout,
  onClose,
}: SettingsPanelProps) {
  const [tab, setTab] = useState<Tab>('servers');
  const { diagnostics, refreshDiagnostics } = useProfiles();

  useEffect(() => {
    if (isOpen && tab === 'diagnostics') void refreshDiagnostics();
  }, [isOpen, tab, refreshDiagnostics]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm animate-fade-in">
      <div
        className="border border-stone-800 p-5 w-[38rem] max-w-[95vw] max-h-[88vh] flex flex-col"
        style={{ background: '#111114' }}
      >
        <div className="flex items-center justify-between mb-3 shrink-0">
          <h2 className="text-stone-300 text-sm uppercase tracking-[0.2em] font-medium">
            Admin Panel
          </h2>
          <button
            onClick={onClose}
            className="text-stone-600 hover:text-stone-400 transition-colors cursor-pointer text-lg leading-none"
          >
            ×
          </button>
        </div>

        <div className="flex gap-1 mb-4 shrink-0 border-b border-stone-800">
          {(
            [
              ['servers', 'Servers'],
              ['general', 'General'],
              ['diagnostics', 'Diagnostics'],
            ] as [Tab, string][]
          ).map(([key, label]) => (
            <button
              key={key}
              onClick={() => setTab(key)}
              className={`px-3 py-1.5 text-[10px] uppercase tracking-[0.15em] cursor-pointer border-b ${
                tab === key
                  ? 'border-red-700 text-red-400'
                  : 'border-transparent text-stone-600 hover:text-stone-400'
              }`}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="flex-1 overflow-y-auto pr-1">
          {tab === 'servers' && (
            <ProfileManager onSwitched={onSwitched} auth={auth} onLogin={onLogin} onLogout={onLogout} />
          )}
          {tab === 'general' && (
            <div className="flex flex-col gap-4">
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-stone-300 text-xs">Validate files on startup</p>
                  <p className="text-stone-600 text-[10px] mt-0.5">
                    {skipValidation
                      ? 'Files will not be checked. Game launches immediately.'
                      : 'Checks all game files and downloads updates.'}
                  </p>
                </div>
                <button
                  onClick={onToggleValidation}
                  className={`relative w-10 h-5 rounded-full transition-colors duration-200 cursor-pointer ${
                    skipValidation ? 'bg-stone-700' : 'bg-red-800'
                  }`}
                >
                  <div
                    className={`absolute top-0.5 w-4 h-4 rounded-full bg-stone-200 transition-transform duration-200 ${
                      skipValidation ? 'translate-x-0.5' : 'translate-x-5'
                    }`}
                  />
                </button>
              </div>
              <p className="text-[10px] text-stone-600 leading-relaxed">
                Addresses, login state and mod toggles are stored per profile in{' '}
                <span className="font-mono text-stone-500">lethe-profiles/&lt;id&gt;/</span>. Switching
                profiles rewrites the three server keys in{' '}
                <span className="font-mono text-stone-500">BepInEx/config/Lethe.cfg</span> and clears
                the in-memory token. ZylarNex is never bundled; the official profile only switches
                addresses for a user-installed loading framework.
              </p>
            </div>
          )}
          {tab === 'diagnostics' && (
            <DiagnosticsPanel
              diagnostics={diagnostics}
              onRefresh={refreshDiagnostics}
              authUsername={auth?.username ?? null}
            />
          )}
        </div>

        <div className="w-full h-px bg-stone-800 my-4 shrink-0" />
        <button
          onClick={onClose}
          className="w-full py-2 border border-stone-800 text-stone-500 hover:text-stone-300 hover:border-stone-700
            uppercase tracking-[0.15em] text-[10px] transition-colors cursor-pointer shrink-0"
        >
          Close
        </button>
      </div>
    </div>
  );
}

interface SettingsButtonProps {
  onClick: () => void;
}

export function SettingsButton({ onClick }: SettingsButtonProps) {
  return (
    <button
      onClick={onClick}
      className="text-stone-700 hover:text-stone-400 transition-colors cursor-pointer"
      title="Settings / Admin"
    >
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      >
        <circle cx="12" cy="12" r="3" />
        <path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42" />
      </svg>
    </button>
  );
}
