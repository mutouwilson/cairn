"use client";

// Minimal i18n. Deliberately rolls its own dictionary-lookup instead of pulling
// next-intl / react-i18next: this app is statically exported into the Tauri
// webview, so we don't need server-rendered locales or route-based segments —
// a Context + JSON dictionary covers everything.
//
// Locale resolution order on every render:
//   1. user override (stored in `localStorage` as "en" / "zh-CN")
//   2. system locale (navigator.language, mapped to a known dictionary)
//   3. English fallback
//
// Switching is reactive — the provider re-renders with the new dictionary,
// so every `useT()` consumer updates without a page reload.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import en from "./dictionaries/en.json";
import zhCN from "./dictionaries/zh-CN.json";

export type Locale = "auto" | "en" | "zh-CN";
export type Effective = "en" | "zh-CN";

type Dict = typeof en;

const DICTIONARIES: Record<Effective, Dict> = { "en": en, "zh-CN": zhCN };
const STORAGE_KEY = "cairn.locale";

/** Best-effort: turn navigator.language into one of our known locales. */
function detectSystemLocale(): Effective {
  if (typeof navigator === "undefined") return "en";
  const tag = (navigator.language || "en").toLowerCase();
  // zh, zh-CN, zh-Hans-CN, etc. all map to zh-CN for now.
  // zh-TW / zh-HK keep falling to English until we ship Traditional.
  if (tag === "zh-cn" || tag === "zh" || tag === "zh-hans" || tag.startsWith("zh-hans")) {
    return "zh-CN";
  }
  if (tag.startsWith("zh-cn")) return "zh-CN";
  return "en";
}

function resolveEffective(locale: Locale): Effective {
  return locale === "auto" ? detectSystemLocale() : locale;
}

/** Friendly label for the *currently effective* locale, used in the Settings
 *  "Current: …" line. We keep this here (not in the dictionary) so it stays in
 *  English when the dict happens to be English, in Chinese when it's Chinese. */
function effectiveLabel(eff: Effective): string {
  return eff === "zh-CN" ? "中文" : "English";
}

interface I18nValue {
  /** What the user picked: "auto" / "en" / "zh-CN". */
  locale: Locale;
  /** The dictionary actually in use right now. */
  effective: Effective;
  /** Friendly label of the effective locale (e.g. "中文"). */
  effectiveLabel: string;
  /** Set the user choice. Persists to localStorage. */
  setLocale: (next: Locale) => void;
  /** Look up `key.path` in the dictionary; substitute `{var}` placeholders. */
  t: (key: string, vars?: Record<string, string | number>) => string;
}

const I18nContext = createContext<I18nValue | null>(null);

export function LocaleProvider({ children }: { children: ReactNode }) {
  // Start as "auto" to keep SSR/hydration deterministic. Once we mount on the
  // client we read localStorage and switch — `hydrated` gates the actual swap
  // so the very first paint is always the auto/system branch.
  const [locale, setLocaleState] = useState<Locale>("auto");
  const [hydrated, setHydrated] = useState(false);

  useEffect(() => {
    try {
      const raw = window.localStorage.getItem(STORAGE_KEY);
      if (raw === "en" || raw === "zh-CN" || raw === "auto") {
        setLocaleState(raw);
      }
    } catch {
      // localStorage unavailable (e.g. private mode) — keep "auto" default.
    }
    setHydrated(true);
  }, []);

  const effective = useMemo(() => resolveEffective(locale), [locale]);

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      /* best-effort */
    }
  }, []);

  // Mirror the body class so any non-React surface (CSS-only) can react to
  // locale. Cheap, optional.
  useEffect(() => {
    if (typeof document === "undefined") return;
    document.documentElement.setAttribute("lang", effective);
  }, [effective]);

  const t = useCallback(
    (key: string, vars?: Record<string, string | number>): string => {
      const dict = DICTIONARIES[effective];
      const parts = key.split(".");
      let cur: unknown = dict;
      for (const p of parts) {
        if (cur && typeof cur === "object" && p in (cur as Record<string, unknown>)) {
          cur = (cur as Record<string, unknown>)[p];
        } else {
          cur = undefined;
          break;
        }
      }
      let str = typeof cur === "string" ? cur : key;
      if (vars) {
        for (const [k, v] of Object.entries(vars)) {
          str = str.replace(new RegExp(`\\{${k}\\}`, "g"), String(v));
        }
      }
      return str;
    },
    [effective],
  );

  const value: I18nValue = useMemo(
    () => ({
      locale,
      effective,
      effectiveLabel: effectiveLabel(effective),
      setLocale,
      t,
    }),
    [locale, effective, setLocale, t],
  );

  // Before hydration we still render — children just won't know the user's
  // saved override. That's fine: first paint uses the system default, then
  // we re-render with the persisted choice (if any) on the next tick.
  void hydrated;

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

/** Get the translation function. Throws if `LocaleProvider` is missing. */
export function useT() {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useT() must be used inside <LocaleProvider>");
  return ctx.t;
}

/** Full locale state + setter. Use this in the language picker. */
export function useLocale() {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useLocale() must be used inside <LocaleProvider>");
  return ctx;
}
