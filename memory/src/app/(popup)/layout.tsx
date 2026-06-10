"use client";
// Bare layout for popup-style webview windows (quick-capture, selection-popover).
// Each page owns its own container chrome; we strip the body background so
// transparent windows render only the page-painted pill / panel.

import { useEffect } from "react";

export default function PopupLayout({ children }: { children: React.ReactNode }) {
  useEffect(() => {
    const html = document.documentElement;
    const body = document.body;
    const prevHtmlBg = html.style.background;
    const prevBodyBg = body.style.background;
    html.style.background = "transparent";
    body.style.background = "transparent";
    return () => {
      html.style.background = prevHtmlBg;
      body.style.background = prevBodyBg;
    };
  }, []);
  return <>{children}</>;
}
