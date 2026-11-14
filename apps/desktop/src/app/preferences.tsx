import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { catalogs, LOCALES, type Locale, type MessageKey } from "../i18n/messages";

export type ThemePreference = "system" | "light" | "dark";

interface Preferences {
  locale: Locale;
  theme: ThemePreference;
  /** F02: offer links copied to the clipboard (off by default). */
  clipboardMonitor: boolean;
  /** F02 ignore rule: sites whose copied links are never offered. */
  ignoredHosts: string[];
  setLocale: (locale: Locale) => void;
  setTheme: (theme: ThemePreference) => void;
  setClipboardMonitor: (on: boolean) => void;
  ignoreHost: (host: string) => void;
  unignoreHost: (host: string) => void;
  t: (key: MessageKey) => string;
}

// Interim per-window storage. Moves to the service settings API (SQLite) in milestone B2.
const STORAGE_KEY = "dm.ui.preferences.v1";

/** Most ignored sites kept (an ignore list, not a history). */
const MAX_IGNORED_HOSTS = 200;

interface Stored {
  locale: Locale;
  theme: ThemePreference;
  clipboardMonitor: boolean;
  ignoredHosts: string[];
}

function load(): Stored {
  const fallbackLocale: Locale = navigator.language.toLowerCase().startsWith("bn") ? "bn" : "en";
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<string, unknown>;
    const locale = LOCALES.includes(raw.locale as Locale) ? (raw.locale as Locale) : fallbackLocale;
    const theme = ["system", "light", "dark"].includes(raw.theme as string)
      ? (raw.theme as ThemePreference)
      : "system";
    const clipboardMonitor = raw.clipboardMonitor === true;
    const ignoredHosts = Array.isArray(raw.ignoredHosts)
      ? raw.ignoredHosts
          .filter((h): h is string => typeof h === "string" && h.length > 0 && h.length <= 253)
          .slice(0, MAX_IGNORED_HOSTS)
      : [];
    return { locale, theme, clipboardMonitor, ignoredHosts };
  } catch {
    return { locale: fallbackLocale, theme: "system", clipboardMonitor: false, ignoredHosts: [] };
  }
}

const PreferencesContext = createContext<Preferences | null>(null);

export function PreferencesProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState(load);

  useEffect(() => {
    const html = document.documentElement;
    html.lang = state.locale;
    if (state.theme === "system") delete html.dataset.theme;
    else html.dataset.theme = state.theme;
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
    } catch {
      // Storage may be unavailable (private mode, policy). Preferences then last for this session.
    }
  }, [state]);

  const setLocale = useCallback((locale: Locale) => setState((s) => ({ ...s, locale })), []);
  const setTheme = useCallback((theme: ThemePreference) => setState((s) => ({ ...s, theme })), []);
  const setClipboardMonitor = useCallback(
    (clipboardMonitor: boolean) => setState((s) => ({ ...s, clipboardMonitor })),
    [],
  );
  const ignoreHost = useCallback(
    (host: string) =>
      setState((s) =>
        s.ignoredHosts.includes(host)
          ? s
          : { ...s, ignoredHosts: [...s.ignoredHosts, host].slice(-MAX_IGNORED_HOSTS) },
      ),
    [],
  );
  const unignoreHost = useCallback(
    (host: string) =>
      setState((s) => ({ ...s, ignoredHosts: s.ignoredHosts.filter((h) => h !== host) })),
    [],
  );
  const value = useMemo<Preferences>(
    () => ({
      ...state,
      setLocale,
      setTheme,
      setClipboardMonitor,
      ignoreHost,
      unignoreHost,
      t: (key) => catalogs[state.locale][key],
    }),
    [state, setLocale, setTheme, setClipboardMonitor, ignoreHost, unignoreHost],
  );
  return <PreferencesContext.Provider value={value}>{children}</PreferencesContext.Provider>;
}

export function usePreferences(): Preferences {
  const ctx = useContext(PreferencesContext);
  if (!ctx) throw new Error("usePreferences must be used inside <PreferencesProvider>");
  return ctx;
}
