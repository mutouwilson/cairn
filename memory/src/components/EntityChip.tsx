"use client";
import Link from "next/link";
import { cn } from "@/lib/utils";
import { ENTITY_COLOR_CLASS } from "@/lib/types";

export function EntityChip({
  id,
  type,
  name,
  confidence,
  small,
}: {
  id: string;
  type: string;
  name: string;
  confidence?: number;
  small?: boolean;
}) {
  const color = ENTITY_COLOR_CLASS[type] ?? "bg-ink-100 text-ink-700 border-ink-200";
  return (
    <Link
      href={`/entity?id=${encodeURIComponent(id)}`}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium transition hover:scale-[1.03]",
        color,
        small && "px-2 py-0.5 text-[10px]",
      )}
      title={`${type} · confidence ${(confidence ?? 1).toFixed(2)}`}
    >
      <span className="opacity-60">{type[0]}</span>
      <span>{name}</span>
    </Link>
  );
}
