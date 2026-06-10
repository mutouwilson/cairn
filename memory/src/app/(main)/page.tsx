"use client";
import { useEffect, useState } from "react";
import Link from "next/link";
import { Capture } from "@/components/Capture";
import { HomeChainStrip } from "@/components/HomeChainStrip";
import { Timeline } from "@/components/Timeline";
import { providersGet } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { SectionEyebrow } from "@/components/ui";

export default function HomePage() {
  const t = useT();
  const [refreshKey, setRefreshKey] = useState(0);
  return (
    <div className="max-w-2xl mx-auto px-8 py-8 space-y-8">
      <OnboardingBanner />
      <HomeChainStrip key={`chain-${refreshKey}`} />
      <section className="space-y-3">
        <div>
          <SectionEyebrow>{t("home.eyebrow_capture")}</SectionEyebrow>
          <h1 className="font-display text-xl font-semibold text-ink-900 mt-1">
            {t("home.prompt")}
          </h1>
        </div>
        <Capture onCaptured={() => setRefreshKey((k) => k + 1)} />
      </section>
      <section className="space-y-3">
        <SectionEyebrow>{t("home.eyebrow_recent")}</SectionEyebrow>
        <Timeline refreshKey={refreshKey} />
      </section>
    </div>
  );
}

function OnboardingBanner() {
  const t = useT();
  const [needsSetup, setNeedsSetup] = useState<null | boolean>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    try {
      setDismissed(localStorage.getItem("cairn:onboard-dismissed") === "1");
    } catch {
      /* ignore */
    }
    void (async () => {
      try {
        const v = await providersGet();
        setNeedsSetup(!v.config.extract && !v.config.embed);
      } catch {
        setNeedsSetup(false);
      }
    })();
  }, []);

  function dismiss() {
    try {
      localStorage.setItem("cairn:onboard-dismissed", "1");
    } catch {
      /* ignore */
    }
    setDismissed(true);
  }

  if (!needsSetup || dismissed) return null;

  return (
    <div className="rounded-lg border border-warn/40 bg-warn-dim px-4 py-3 text-xs flex items-start gap-3">
      <div className="flex-1 leading-relaxed text-warn">
        <div className="font-semibold mb-0.5">{t("home.banner.title")}</div>
        <div className="text-ink-700">
          {t("home.banner.body_prefix")}
          <Link href="/settings" className="underline underline-offset-2 font-medium text-ink-900">
            {t("home.banner.body_link")}
          </Link>
          {t("home.banner.body_suffix")}
        </div>
      </div>
      <button
        type="button"
        onClick={dismiss}
        title={t("common.dismiss")}
        className="text-ink-500 hover:text-ink-900 text-2xs underline underline-offset-2 shrink-0"
      >
        {t("home.banner.dismiss")}
      </button>
    </div>
  );
}
