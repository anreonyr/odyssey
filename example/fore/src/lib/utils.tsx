// cn() — Tailwind class merger. Required by every shadcn/ui
// component for handling conditional classes (`cn("p-4",
// isActive && "bg-accent")`) without dedup conflicts.
//
// `clsx` handles the falsy-skipping; `twMerge` collapses
// conflicting Tailwind classes (`p-2 p-4` → `p-4`).

import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}
