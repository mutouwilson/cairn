import "./globals.css";
import type { Metadata } from "next";
import { Inter, Inter_Tight, Geist_Mono } from "next/font/google";
import { LocaleProvider } from "@/lib/i18n";

// Bundled at build time via next/font — Tauri serves the .woff2 from disk so
// no network access is needed at runtime. Variable fonts keep CSS small.
const inter = Inter({
  subsets: ["latin"],
  variable: "--font-sans",
  display: "swap",
});

const interTight = Inter_Tight({
  subsets: ["latin"],
  variable: "--font-display",
  display: "swap",
});

const geistMono = Geist_Mono({
  subsets: ["latin"],
  variable: "--font-mono",
  display: "swap",
});

export const metadata: Metadata = {
  title: "Cairn",
  description: "Mark where you've been. Read by every agent.",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    // `lang` is also synced reactively from LocaleProvider — the static default
    // here is just what the very first paint shows before client hydration.
    <html lang="en" className={`${inter.variable} ${interTight.variable} ${geistMono.variable}`}>
      <body>
        <LocaleProvider>{children}</LocaleProvider>
      </body>
    </html>
  );
}
