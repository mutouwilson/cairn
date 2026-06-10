"use client";
// Primitives matching the Pencil design system (Cairn).
// One file so the imports stay readable: `import { EntityTag, BtnPrimary } from "@/components/ui";`

import { cn } from "@/lib/utils";
import { ENTITY_COLOR_CLASS } from "@/lib/types";
import type { ReactNode, ButtonHTMLAttributes } from "react";

// ---------- Buttons ----------

type BtnProps = ButtonHTMLAttributes<HTMLButtonElement> & { children: ReactNode };

export function BtnPrimary({ className, children, ...p }: BtnProps) {
  return (
    <button
      {...p}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md bg-ink-900 px-3 h-8 text-xs font-medium text-white",
        "hover:bg-ink-800 disabled:opacity-40 disabled:cursor-not-allowed transition-colors",
        className,
      )}
    >
      {children}
    </button>
  );
}

export function BtnSecondary({ className, children, ...p }: BtnProps) {
  return (
    <button
      {...p}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md border border-ink-200 bg-white px-3 h-8 text-xs font-medium text-ink-900",
        "hover:bg-ink-100 hover:border-ink-300 disabled:opacity-40 disabled:cursor-not-allowed transition-colors",
        className,
      )}
    >
      {children}
    </button>
  );
}

export function BtnGhost({ className, children, ...p }: BtnProps) {
  return (
    <button
      {...p}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md px-2.5 h-7 text-xs font-medium text-ink-500",
        "hover:text-ink-900 hover:bg-ink-100 disabled:opacity-40 transition-colors",
        className,
      )}
    >
      {children}
    </button>
  );
}

export function BtnAccent({ className, children, ...p }: BtnProps) {
  return (
    <button
      {...p}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md bg-accent px-3 h-8 text-xs font-medium text-white",
        "hover:bg-accent-fg disabled:opacity-40 transition-colors",
        className,
      )}
    >
      {children}
    </button>
  );
}

// ---------- Status pill ----------

type Tone = "neutral" | "accent" | "warn" | "danger" | "muted";

const TONE: Record<Tone, string> = {
  neutral: "bg-ink-100 text-ink-700",
  muted: "bg-ink-100 text-ink-500",
  accent: "bg-accent-dim text-accent-fg",
  warn: "bg-warn-dim text-warn",
  danger: "bg-danger-dim text-danger",
};

export function StatusPill({
  tone = "neutral",
  dot,
  children,
  className,
}: {
  tone?: Tone;
  dot?: boolean;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full px-2 h-5 text-2xs font-medium",
        TONE[tone],
        className,
      )}
    >
      {dot && (
        <span
          className={cn(
            "h-1.5 w-1.5 rounded-full",
            tone === "accent" && "bg-accent",
            tone === "warn" && "bg-warn",
            tone === "danger" && "bg-danger",
            (tone === "neutral" || tone === "muted") && "bg-ink-500",
          )}
        />
      )}
      {children}
    </span>
  );
}

// ---------- Entity tag ----------

export function EntityTag({
  type,
  name,
  className,
  small,
}: {
  type: string;
  name?: ReactNode;
  className?: string;
  small?: boolean;
}) {
  const dot = ENTITY_DOT[type as keyof typeof ENTITY_DOT] ?? "bg-ink-400";
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1.5 rounded-md",
        small ? "text-2xs h-5 px-1.5" : "text-xs h-6 px-2",
        "bg-ink-100/70 text-ink-700",
        className,
      )}
    >
      <span className={cn("rounded-full", small ? "h-1.5 w-1.5" : "h-2 w-2", dot)} />
      <span className="font-medium lowercase">{type}</span>
      {name && <span className="text-ink-500">· {name}</span>}
    </span>
  );
}

const ENTITY_DOT: Record<string, string> = {
  Person: "bg-entity-person",
  Event: "bg-entity-event",
  Preference: "bg-entity-preference",
  Belief: "bg-entity-belief",
  Goal: "bg-entity-goal",
  Asset: "bg-entity-asset",
  Skill: "bg-entity-skill",
  Location: "bg-entity-location",
};

export { ENTITY_DOT };

// Re-export legacy entity color class map for any callers still using it.
export { ENTITY_COLOR_CLASS };

// ---------- Hash chip ----------

export function HashChip({ hash, className }: { hash: string; className?: string }) {
  const short = hash.length > 10 ? `${hash.slice(0, 4)}…${hash.slice(-4)}` : hash;
  return (
    <code
      title={hash}
      className={cn(
        "inline-flex items-center rounded bg-ink-100 px-1.5 h-5 text-2xs font-mono text-ink-500",
        className,
      )}
    >
      {short}
    </code>
  );
}

// ---------- Kbd ----------

export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        "inline-flex items-center justify-center rounded border border-ink-200 bg-white px-1.5 h-5",
        "text-2xs font-mono text-ink-500 shadow-[inset_0_-1px_0_0] shadow-ink-200",
        className,
      )}
    >
      {children}
    </kbd>
  );
}

// ---------- Inspector row ----------

export function InspectorRow({
  label,
  value,
  mono,
  className,
}: {
  label: string;
  value: ReactNode;
  mono?: boolean;
  className?: string;
}) {
  return (
    <div className={cn("flex items-baseline justify-between gap-3 py-1.5", className)}>
      <span className="text-2xs text-ink-500 shrink-0">{label}</span>
      <span
        className={cn(
          "text-xs text-ink-800 text-right truncate",
          mono && "font-mono text-ink-700",
        )}
      >
        {value}
      </span>
    </div>
  );
}

// ---------- Section title ----------

export function SectionEyebrow({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <h3 className={cn("eyebrow", className)}>{children}</h3>
  );
}

// ---------- Card surface ----------

export function Card({
  children,
  className,
  padded = true,
}: {
  children: ReactNode;
  className?: string;
  padded?: boolean;
}) {
  return (
    <div
      className={cn(
        "rounded-lg border border-ink-200 bg-white",
        padded && "p-4",
        className,
      )}
    >
      {children}
    </div>
  );
}
