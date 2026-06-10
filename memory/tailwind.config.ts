import type { Config } from "tailwindcss";

const config: Config = {
  content: ["./src/**/*.{ts,tsx,mdx}"],
  darkMode: "media",
  theme: {
    extend: {
      fontFamily: {
        // Latin runs in Inter / Inter Tight (loaded via next/font); the
        // CJK glyphs fall through to whichever family the OS owns —
        // PingFang SC on macOS, Microsoft YaHei on Windows, Noto Sans
        // CJK on Linux. Without these the browser picks an arbitrary
        // serif/ugly fallback and Chinese text looks mismatched against
        // the Inter-styled English.
        sans: [
          "var(--font-sans)",
          "Inter",
          "-apple-system",
          "BlinkMacSystemFont",
          "Helvetica",
          "Arial",
          // CJK fallbacks — order: macOS native → Hiragino (older macOS
          // / fallback) → Windows → Linux Noto → Source Han.
          "PingFang SC",
          "Hiragino Sans GB",
          "Microsoft YaHei",
          "Noto Sans CJK SC",
          "Noto Sans SC",
          "Source Han Sans SC",
          "sans-serif",
        ],
        display: [
          "var(--font-display)",
          "Inter Tight",
          "Inter",
          "-apple-system",
          "BlinkMacSystemFont",
          "PingFang SC",
          "Hiragino Sans GB",
          "Microsoft YaHei",
          "Noto Sans CJK SC",
          "Noto Sans SC",
          "Source Han Sans SC",
          "sans-serif",
        ],
        mono: [
          "var(--font-mono)",
          "Geist Mono",
          "ui-monospace",
          "SF Mono",
          "Menlo",
          "Consolas",
          // CJK in mono blocks is rare but does happen (timestamps,
          // path components). Keep the chain Latin-first, CJK as
          // last-resort fallback.
          "PingFang SC",
          "monospace",
        ],
      },
      colors: {
        ink: {
          50: "#FAFAF9",
          100: "#F5F5F4",
          200: "#E5E5E5",
          300: "#D4D4D4",
          400: "#A3A3A3",
          500: "#737373",
          600: "#525252",
          700: "#404040",
          800: "#171717",
          900: "#0A0A0A",
        },
        surface: {
          DEFAULT: "#FFFFFF",
          sunken: "#F5F5F4",
          inverse: "#0A0A0A",
        },
        accent: {
          DEFAULT: "#00875A",
          dim: "#E6F4EE",
          fg: "#00603F",
        },
        warn: {
          DEFAULT: "#D97706",
          dim: "#FEF3C7",
        },
        danger: {
          DEFAULT: "#DC2626",
          dim: "#FEE2E2",
        },
        entity: {
          person: "#D97757",
          event: "#5B8DEF",
          preference: "#6B8E23",
          belief: "#9A6CB2",
          goal: "#D4A017",
          asset: "#0D6E6E",
          skill: "#B85450",
          location: "#5A8A3A",
        },
        chain: {
          link: "#D4D4D4",
          node: "#0A0A0A",
        },
      },
      fontSize: {
        "2xs": ["11px", { lineHeight: "1.45" }],
        xs: ["12px", { lineHeight: "1.45" }],
        sm: ["13px", { lineHeight: "1.45" }],
        base: ["14px", { lineHeight: "1.45" }],
        md: ["16px", { lineHeight: "1.45" }],
        lg: ["20px", { lineHeight: "1.3" }],
        xl: ["28px", { lineHeight: "1.2" }],
      },
      letterSpacing: {
        caps: "0.06em",
      },
      borderRadius: {
        sm: "4px",
        md: "6px",
        lg: "8px",
        xl: "12px",
        "2xl": "16px",
      },
      spacing: {
        nav: "56px",
        header: "56px",
        inspector: "320px",
      },
      animation: {
        "pulse-soft": "pulse-soft 2s cubic-bezier(0.4, 0, 0.6, 1) infinite",
      },
      keyframes: {
        "pulse-soft": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0.6" },
        },
      },
    },
  },
  plugins: [],
};

export default config;
