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
  setLocale: (locale: Locale) => void;
  setTheme: (theme: ThemePreference) => void;
  t: (key: MessageKey) => string;
}

// Interim per-window storage. Moves to the service settings API (SQLite) in milestone B2.
const STORAGE_KEY = "dm.ui.preferences.v1";

function load(): { locale: Locale; theme: ThemePreference } {
  const fallbackLocale: Locale = navigator.language.toLowerCase().startsWith("bn") ? "bn" : "en";
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}") as Record<string, unknown>;
    const locale = LOCALES.includes(raw.locale as Locale) ? (raw.locale as Locale) : fallbackLocale;
    const theme = ["system", "light", "dark"].includes(raw.theme as string)
      ? (raw.theme as ThemePreference)
      : "system";
    return { locale, theme };
  } catch {
    return { locale: fallbackLocale, theme: "system" };
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
  const value = useMemo<Preferences>(
    () => ({ ...state, setLocale, setTheme, t: (key) => catalogs[state.locale][key] }),
    [state, setLocale, setTheme],
  );
  return <PreferencesContext.Provider value={value}>{children}</PreferencesContext.Provider>;
}

export function usePreferences(): Preferences {
  const ctx = useContext(PreferencesContext);
  if (!ctx) throw new Error("usePreferences must be used inside <PreferencesProvider>");
  return ctx;
}
