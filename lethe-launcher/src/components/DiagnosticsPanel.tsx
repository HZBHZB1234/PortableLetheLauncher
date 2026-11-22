import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Diagnostics } from '../types/profile';

interface DiagnosticsPanelProps {
  diagnostics: Diagnostics | null;
  onRefresh: () => Promise<void>;
  authUsername: string | null;
}

export function DiagnosticsPanel({ diagnostics, onRefresh, authUsername }: DiagnosticsPanelProps) {
  const [news, setNews] = useState<string[]>([]);
  const [newsError, setNewsError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    invoke<{ title: string }[]>('get_news')
      .then((items) => {
        if (alive) setNews(items.map((i) => i.title));
      })
      .catch((e) => {
        if (alive) setNewsError(String(e));
      });
    return () => {
      alive = false;
    };
  }, [diagnostics?.profileId]);

  const token = diagnostics?.token;
  const addresses = diagnostics?.addresses;

  return (
    <div className="flex flex-col gap-3 text-stone-400">
      <div className="flex items-center justify-between">
        <p className="text-[10px] uppercase tracking-[0.2em] text-stone-500">
          Profile: {diagnostics?.profileId ?? '—'}
        </p>
        <button
          onClick={() => void onRefresh()}
          className="px-2 py-1 text-[10px] uppercase tracking-wider border border-stone-800 hover:border-stone-600 cursor-pointer"
        >
          Refresh
        </button>
      </div>

      <div className="border border-stone-800 p-2 text-[10px] font-mono space-y-1">
        <p>
          <span className="text-stone-600">token: </span>
          {token?.isLoggedIn ? (
            <span className={token.expired ? 'text-amber-400' : 'text-emerald-500'}>
              {token.expired ? 'expired' : 'valid'}
              {token.username ? ` (${token.username})` : ''}
            </span>
          ) : (
            <span className="text-stone-600">none</span>
          )}
        </p>
        <p>
          <span className="text-stone-600">ui auth: </span>
          {authUsername ?? 'not signed in'}
        </p>
        <p>
          <span className="text-stone-600">working dir: </span>
          {diagnostics?.workingDir ?? '—'}
        </p>
        <p>
          <span className="text-stone-600">game cfg: </span>
          {diagnostics?.gameConfigPath ?? '—'} (
          {diagnostics?.gameConfigExists ? 'exists' : 'missing'})
        </p>
        {addresses && (
          <>
            <p>
              <span className="text-stone-600">auth: </span>
              {addresses.authBaseUrl}
            </p>
            <p>
              <span className="text-stone-600">server root: </span>
              {addresses.serverRoot}
            </p>
            <p>
              <span className="text-stone-600">manifest: </span>
              {addresses.manifestUrl}
            </p>
            <p>
              <span className="text-stone-600">download: </span>
              {addresses.downloadBaseUrl}
            </p>
            <p>
              <span className="text-stone-600">plugins: </span>
              {addresses.pluginUrls.map((p) => p.filename).join(', ') || 'none'}
            </p>
          </>
        )}
      </div>

      {news.length > 0 && (
        <div className="text-[10px] text-stone-500">
          <p className="uppercase tracking-[0.15em] mb-1">News</p>
          <ul className="list-disc pl-4 space-y-0.5">
            {news.slice(0, 5).map((title, i) => (
              <li key={`${title}-${i}`}>{title}</li>
            ))}
          </ul>
        </div>
      )}
      {newsError && <p className="text-[10px] text-stone-700">News unavailable: {newsError}</p>}

      <div>
        <p className="text-[10px] uppercase tracking-[0.15em] text-stone-600 mb-1">Log tail</p>
        <pre className="bg-black/50 border border-stone-800 p-2 text-[9px] leading-relaxed text-stone-500 h-40 overflow-auto whitespace-pre-wrap break-all">
          {diagnostics?.logTail || '(no log output)'}
        </pre>
      </div>
    </div>
  );
}
