import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AuthResult } from "../types/auth";

interface UseAuthReturn {
  auth: AuthResult | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  loginError: string | null;
  startLogin: () => Promise<void>;
  logout: () => Promise<void>;
  /** Reload the token of the currently active profile (used after a switch). */
  refresh: () => Promise<void>;
}

interface JwtPayload {
  sub?: string;
  name?: string;
  avatar?: string;
  exp?: number;
}

function decodeJwt(token: string): JwtPayload {
  try {
    const parts = token.split(".");
    if (parts.length < 2) return {};
    return JSON.parse(atob(parts[1]));
  } catch {
    return {};
  }
}

function buildAvatarUrl(userId: string, avatarHash?: string): string {
  if (avatarHash) {
    return `https://cdn.discordapp.com/avatars/${userId}/${avatarHash}.webp?size=64`;
  }
  const discriminator = (BigInt(userId) >> 22n) % 6n;
  return `https://cdn.discordapp.com/embed/avatars/${discriminator}.png`;
}

export function useAuth(): UseAuthReturn {
  const [auth, setAuth] = useState<AuthResult | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [loginError, setLoginError] = useState<string | null>(null);

  /** Populate username/avatar from the JWT payload */
  const enrichAuth = useCallback((raw: AuthResult): AuthResult => {
    const claims = decodeJwt(raw.token);
    const userId = claims.sub;
    return {
      ...raw,
      username: raw.username || userId || "Unknown",
      avatar_url: raw.avatar_url || (userId ? buildAvatarUrl(userId) : ""),
    };
  }, []);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    try {
      // The backend resolves the token from the active profile only.
      const saved = await invoke<AuthResult | null>("get_saved_auth");
      setAuth(saved ? enrichAuth(saved) : null);
      setLoginError(null);
    } catch (e) {
      console.error(e);
      setAuth(null);
    } finally {
      setIsLoading(false);
    }
  }, [enrichAuth]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const startLogin = useCallback(async () => {
    setIsLoading(true);
    setLoginError(null);
    try {
      const result = await invoke<AuthResult>("start_oauth");
      const enriched = enrichAuth(result);
      setAuth(enriched);
    } catch (err) {
      setLoginError(String(err));
    } finally {
      setIsLoading(false);
    }
  }, [enrichAuth]);

  const logout = useCallback(async () => {
    await invoke("logout");
    setAuth(null);
  }, []);

  return {
    auth,
    isAuthenticated: auth !== null,
    isLoading,
    loginError,
    startLogin,
    logout,
    refresh,
  };
}
