---
date: 2026-09-17T02:02:29+0800
author: anreonyr
commit: 3c8b0dc
branch: master
repository: odyssey
topic: "frontend monospace-forward brutalist raw — token/font/motion/spatial"
tags: [design, frontend, design-system, example-fore]
status: ready
parent: ".rpiv/artifacts/research/2026-09-17_02-02-29_frontend-monospace-brutalist.md"
phase_count: 4
phases:
  - n: 1
    title: Foundation — tokens, fonts, noise, motion dead-code
    files:
      - example/fore/package.json
      - example/fore/src/main.tsx
      - example/fore/src/index.css
      - example/fore/tailwind.config.tsx
    depends_on: []
  - n: 2
    title: UI primitive hardening — shadows to borders, rounded outliers
    files:
      - example/fore/src/components/ui/card.tsx
      - example/fore/src/components/ui/input.tsx
      - example/fore/src/components/ui/textarea.tsx
      - example/fore/src/components/ui/button.tsx
      - example/fore/src/components/ui/dialog.tsx
      - example/fore/src/components/ui/tooltip.tsx
      - example/fore/src/components/ui/tabs.tsx
    depends_on: [1]
  - n: 3
    title: Layout & app components — Header mono, app-component spacing
    files:
      - example/fore/src/components/layout/Header.tsx
      - example/fore/src/components/StepRow.tsx
      - example/fore/src/components/MemoryPanel.tsx
      - example/fore/src/components/JsonEditor.tsx
    depends_on: [1]
  - n: 4
    title: Page-level propagation — divide-y, form labels, mono
    files:
      - example/fore/src/pages/Overview.tsx
      - example/fore/src/pages/Caps.tsx
      - example/fore/src/pages/CapDetail.tsx
      - example/fore/src/pages/Checkpoints.tsx
      - example/fore/src/pages/Invoke.tsx
      - example/fore/src/pages/AgentSession.tsx
      - example/fore/src/pages/Agent.tsx
      - example/fore/src/pages/Playground.tsx
    depends_on: [2, 3]
last_updated: 2026-09-17T02:02:29+0800
last_updated_by: anreonyr
---

# Design: Frontend Monospace-Forward Brutalist Raw

## Summary

Implement the `/skill:frontend-design` brief for `example/fore/`. Token-level foundation (JetBrains Mono Variable font via Fontsource, thickened `--border`/`--input` for visible structure, `body::before` SVG noise overlay, dead-code accordion keyframes removed). UI primitives hardened (shadows → borders, rounded outliers → `--radius`). Layout + app components + pages propagate the mono-forward + structured-grid system.

## Requirements

- Monospace-forward: every label/header/number renders in JetBrains Mono Variable (weight 400–800), no system fallback
- Brutalist raw: visible 1px borders replace shadows; rounded outliers normalized; structured grid baseline
- Subtle micro-interactions: keep existing `transition-*`; remove unused `accordion-down`/`accordion-up` keyframes
- Solid + noise/grain: SVG `feTurbulence` overlay at 4% opacity via `body::before`
- Type-safe: `pnpm build` passes (`tsc --noEmit && vite build`)
- Lint clean: `pnpm lint`, `pnpm format:check`

## Current State Analysis

See research artifact `2026-09-17_02-02-29_frontend-monospace-brutalist.md` §Current State Analysis. Key state:

- Tokens at `src/index.css:11-58`; HSL channel triplet discipline honored everywhere
- Tailwind config routes `font-mono` to `var(--font-mono)` at `tailwind.config.tsx:79`
- 53× `font-mono` adoption across 16 files (mono is already pervasive)
- 22× `border-border` + 3× `border-input` = 25 borders moving in lockstep with the new value
- 6 `shadow-*` in UI primitives (clean blast radius)
- 5 inline `border-b` list-row separators ready to collapse to `divide-y`

## Scope

### Building

- **Tokens**: `--font-ui`/`--font-mono` → JetBrains Mono Variable + system fallback; `--border`/`--input` `220 13% 18%` → `220 14% 25%`
- **Noise overlay**: `body::before` SVG `feTurbulence` data URI, 4% opacity, fixed positioning, `mix-blend-mode: overlay`
- **Motion cleanup**: drop `accordion-down`/`accordion-up` keyframes + `animation` entries in `tailwind.config.tsx`
- **UI primitives**: drop all `shadow-*`; replace `rounded-lg`→`rounded-md`, `rounded-sm`→`rounded-md`; tabs drop `data-[state=active]:shadow`
- **Layout**: `Header` h1 → mono + tabular numerics
- **App components**: `StepRow`, `MemoryPanel`, `JsonEditor` — round fractional/half-step spacing to 8px multiples
- **Pages**: collapse inline `border-b` list-rows → `divide-y`; round form-label `mb-1` → `mb-2`; propagate mono on remaining labels
- **Validation**: `pnpm install`, `pnpm build`, `pnpm lint`, `pnpm format:check`

### Not Building

- A new component library, theme switcher, or design-system package
- Migrating to Geist/Satoshi/Inter (explicit brief ban)
- Adding `tailwindcss-animate` or new motion dependencies
- Generating icons or custom SVG assets beyond the inline noise data URI
- Touching `vite.config.tsx`, `tsconfig.json`, `index.html` (no font link needed; Fontsource imports via JS)
- Refactoring app-level architecture (router, hooks, API client)
- Touching `docs/`, `crate/`, or any non-`example/fore/` files

## Decisions

### Font swap

**Decision**: Install `@fontsource-variable/jetbrains-mono`; import in `src/main.tsx`; change `--font-mono` value to `"JetBrains Mono Variable", ui-monospace, "SF Mono", Menlo, monospace`. Set `--font-ui` to the same stack so the Header h1 (the only sans headline) inherits JetBrains Mono Variable via Tailwind `font-sans`.

**Evidence**: research §1 ("No webfont imports anywhere") + §2 ("Mono is already pervasive"); font audit agent recommends `@fontsource-variable/jetbrains-mono` as single-package solution (one ~120KB woff2, weight 400–800).

**Rationale**: Cleanest path; zero `@font-face` declarations; zero `index.html` link; Vite fingerprints and inlines. Mono is the design's identity — no reason to keep a separate sans stack.

### Border thickening

**Decision**: Set `--border` and `--input` to `220 14% 25%` (was `220 13% 18%`). Both move in lockstep to preserve the current coupling.

**Evidence**: research §4 (22× `border-border`, 3× `border-input`, identical values today); integration-scanner verdict "Safe, just bump `--input` together".

**Rationale**: brutalist raw = visible structure. Muted `220 13% 18%` is invisible against the `220 16% 12%` card surface; `25%` lifts it to visible-against-card territory without going chrome.

### Noise overlay

**Decision**: `body::before` pseudo-element with inline SVG `feTurbulence` data URI. `position: fixed; inset: 0; pointer-events: none; z-index: 1; opacity: 0.04; mix-blend-mode: overlay`.

**Evidence**: research §8 (zero existing noise); web-search-researcher recommendation (option C) with Vercel Geist / shadcn new-york / Codrops precedents.

**Rationale**: Pure-CSS, zero JSX render, zero binary asset, zero z-index plumbing. One rule, one block.

### Accordion keyframes

**Decision**: Delete `accordion-down`/`accordion-up` `keyframes` and `animation` entries from `tailwind.config.tsx:73-82`.

**Evidence**: motion audit agent — zero `animate-accordion-*` references; no Accordion primitive in `src/components/ui/`; keyframes are pure dead code.

**Rationale**: Brief chose "subtle micro-interactions"; these keyframes served no consumer.

### UI primitive hardening

**Decision**: Drop all 6 `shadow-*` usages in `card.tsx`, `input.tsx`, `textarea.tsx`, `dialog.tsx`, `tooltip.tsx`, `tabs.tsx`. Replace `rounded-lg` (card, dialog) → `rounded-md`; `rounded-sm` (dialog close, tabs trigger) → `rounded-md`. Tabs drop `data-[state=active]:shadow`.

**Evidence**: spatial audit agent — 6× `shadow-*` all in UI primitives; 2× `rounded-lg`, 2× `rounded-sm` outside the `--radius` token.

**Rationale**: brutalist raw = flat surfaces with visible borders, no float.

### Page propagation

**Decision**: Collapse the 5 inline `border-b` list-row patterns (Checkpoints, Invoke, AgentSession, Agent, Caps) to `divide-y divide-border/50` on the parent `<ul>`. Round 6 form-label `mb-1` → `mb-2`. Round `py-2.5`/`px-2.5` → `py-3`/`px-3`. Round `gap-1.5` → `gap-2`. Keep `border-border/50` opacity as the inline-row precedent.

**Evidence**: spatial audit agent counts (5× inline border-b, ~30 fractional/half-step values).

**Rationale**: brutalist = structured grid; collapse repeated borders to single parent divide; 8px-multiple spacing = visible structure.

### max-w-md outlier

**Decision**: `pages/Caps.tsx:86` `max-w-md` (search input column) → `max-w-2xl` to fit inside the `max-w-6xl` content rail baseline.

**Evidence**: spatial audit — single deviation from `max-w-6xl` baseline at `AppShell.tsx:26`.

**Rationale**: brief = structured grid, symmetric; outliers break the rail.

## Architecture

### example/fore/package.json — MODIFY

Add `@fontsource-variable/jetbrains-mono` to `dependencies`:

```json
  "dependencies": {
    "@fontsource-variable/jetbrains-mono": "^5.2.5",
    "class-variance-authority": "^0.7.1",
```

### example/fore/src/main.tsx — MODIFY

Add Fontsource import before `./index.css` import:

```tsx
import "@fontsource-variable/jetbrains-mono";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";

import { App } from "./App";

import "./index.css";
```

### example/fore/src/index.css — MODIFY

Replace the file contents:

```css
/* odyssey theme — GitHub Dark tokens exposed as HSL channels
   so Tailwind's `bg-primary/50` syntax composes alpha without
   recompiling. The previous theme.css used solid hex tokens;
   shadcn/ui requires the split-channel form to keep its
   Tailwind class API working.

   JetBrains Mono Variable loaded via @fontsource-variable in
   main.tsx; the variable stack below covers both display and
   body so the entire product renders mono. */

@tailwind base;
@tailwind components;
@tailwind utilities;

@layer base {
  :root {
    /* Base */
    --background: 220 22% 9%; /* #0d1117 */
    --foreground: 217 18% 80%; /* #c9d1d9 */

    /* Surfaces */
    --card: 220 16% 12%; /* #161b22 */
    --card-foreground: 217 18% 80%;
    --popover: 220 16% 12%;
    --popover-foreground: 217 18% 80%;

    /* Muted text + bg */
    --muted: 220 13% 18%; /* #21262d */
    --muted-foreground: 215 11% 65%; /* #8b949e */

    /* Subtle accent for hover/active rows */
    --accent: 220 13% 18%;
    --accent-foreground: 217 18% 80%;

    /* Primary — GitHub blue */
    --primary: 212 92% 68%; /* #58a6ff */
    --primary-foreground: 220 22% 9%;

    /* Secondary — subdued */
    --secondary: 220 13% 18%;
    --secondary-foreground: 217 18% 80%;

    /* Status colours */
    --success: 137 56% 50%; /* #3fb950 */
    --success-foreground: 220 22% 9%;
    --warning: 41 70% 47%; /* #d29922 */
    --warning-foreground: 220 22% 9%;
    --destructive: 358 75% 60%; /* #f85149 */
    --destructive-foreground: 217 18% 80%;

    /* Borders + inputs — thickened for brutalist structure (was 18%, now 25%) */
    --border: 220 14% 25%; /* #30363d → visible against #161b22 */
    --input: 220 14% 25%;
    --ring: 212 92% 68%;

    --radius: 0.5rem;

    /* Type stacks — JetBrains Mono Variable (loaded via
       @fontsource-variable/jetbrains-mono in main.tsx) for
       display + body, ui-monospace as fallback only. */
    --font-ui: "JetBrains Mono Variable", ui-monospace, "SF Mono", Menlo, Consolas, monospace;
    --font-mono: "JetBrains Mono Variable", ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  }
}

@layer base {
  * {
    @apply border-border;
  }

  html,
  body {
    @apply bg-background text-foreground antialiased;
    font-family: var(--font-ui);
    font-size: 13px;
    line-height: 1.5;
  }

  body {
    min-height: 100vh;
  }

  code,
  pre,
  kbd {
    font-family: var(--font-mono);
    font-size: 12px;
  }
}

/* Mono-sized labels and chips everywhere they appear. */
.font-mono,
code,
pre,
kbd,
samp {
  font-family: var(--font-mono);
  font-size: 12px;
}

/* Paper-grain noise overlay — 4% opacity SVG turbulence over
   the GitHub Dark body. Fixed positioning follows scroll.
   pointer-events: none keeps it decorative; mix-blend-mode
   composites over card/surface fills without darkening
   content text. */
body::before {
  content: "";
  position: fixed;
  inset: 0;
  pointer-events: none;
  z-index: 1;
  opacity: 0.04;
  mix-blend-mode: overlay;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='240' height='240'><filter id='n'><feTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='2' stitchTiles='stitch'/><feColorMatrix values='0 0 0 0 1  0 0 0 0 1  0 0 0 0 1  0 0 0 0.6 0'/></filter><rect width='100%25' height='100%25' filter='url(%23n)'/></svg>");
}

/* Subtle scrollbar for dark theme; the default chromium bar
   looks out of place against the muted bg. */
::-webkit-scrollbar {
  width: 10px;
  height: 10px;
}
::-webkit-scrollbar-track {
  background: hsl(var(--background));
}
::-webkit-scrollbar-thumb {
  background: hsl(var(--border));
  border-radius: 5px;
}
::-webkit-scrollbar-thumb:hover {
  background: hsl(var(--muted-foreground) / 0.4);
}
```

### example/fore/tailwind.config.tsx — MODIFY

Replace the file contents (drop `accordion-down`/`accordion-up` keyframes + animation):

```tsx
// Tailwind config — GitHub Dark palette as CSS variables so a
// future re-theme only changes `src/index.css`. shadcn/ui
// requires this layout: every token is a HSL channel triplet
// (`<channel> <channel> <channel> / <alpha>`), so the components
// can apply opacity via `bg-primary/50` etc.

import type { Config } from "tailwindcss";

const config: Config = {
  darkMode: ["class"],
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    container: {
      center: true,
      padding: "1.5rem",
    },
    extend: {
      colors: {
        border: "hsl(var(--border) / <alpha-value>)",
        input: "hsl(var(--input) / <alpha-value>)",
        ring: "hsl(var(--ring) / <alpha-value>)",
        background: "hsl(var(--background) / <alpha-value>)",
        foreground: "hsl(var(--foreground) / <alpha-value>)",
        primary: {
          DEFAULT: "hsl(var(--primary) / <alpha-value>)",
          foreground: "hsl(var(--primary-foreground) / <alpha-value>)",
        },
        secondary: {
          DEFAULT: "hsl(var(--secondary) / <alpha-value>)",
          foreground: "hsl(var(--secondary-foreground) / <alpha-value>)",
        },
        destructive: {
          DEFAULT: "hsl(var(--destructive) / <alpha-value>)",
          foreground: "hsl(var(--destructive-foreground) / <alpha-value>)",
        },
        success: {
          DEFAULT: "hsl(var(--success) / <alpha-value>)",
          foreground: "hsl(var(--success-foreground) / <alpha-value>)",
        },
        warning: {
          DEFAULT: "hsl(var(--warning) / <alpha-value>)",
          foreground: "hsl(var(--warning-foreground) / <alpha-value>)",
        },
        muted: {
          DEFAULT: "hsl(var(--muted) / <alpha-value>)",
          foreground: "hsl(var(--muted-foreground) / <alpha-value>)",
        },
        accent: {
          DEFAULT: "hsl(var(--accent) / <alpha-value>)",
          foreground: "hsl(var(--accent-foreground) / <alpha-value>)",
        },
        popover: {
          DEFAULT: "hsl(var(--popover) / <alpha-value>)",
          foreground: "hsl(var(--popover-foreground) / <alpha-value>)",
        },
        card: {
          DEFAULT: "hsl(var(--card) / <alpha-value>)",
          foreground: "hsl(var(--card-foreground) / <alpha-value>)",
        },
      },
      borderRadius: {
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
      },
      fontFamily: {
        sans: ["var(--font-ui)"],
        mono: ["var(--font-mono)"],
      },
    },
  },
  plugins: [],
};

export default config;
```

### example/fore/src/components/ui/card.tsx — MODIFY

```tsx
// Card root className: drop `shadow-sm`, replace `rounded-lg` with `rounded-md`
// CardHeader: `space-y-1.5 p-4` → `space-y-2 p-4`
// All other parts unchanged.

const Card = ... (
    <div
      ref={ref}
      className={cn("bg-card text-card-foreground rounded-md border", className)}
      {...props}
    />
  ),

const CardHeader = ... (
    <div ref={ref} className={cn("flex flex-col space-y-2 p-4", className)} {...props} />
  ),

// CardContent: `p-4 pt-0` → `p-4 pt-0` (unchanged)
// CardFooter: `flex items-center p-4 pt-0` → `flex items-center p-4 pt-0` (unchanged)
// CardTitle/Description unchanged
```

### example/fore/src/components/ui/input.tsx — MODIFY

```tsx
// Drop `shadow-sm` from the className string.

className={cn(
  "border-input bg-background file:text-foreground placeholder:text-muted-foreground focus-visible:ring-ring flex h-8 w-full rounded-md border px-3 py-1 font-mono text-xs transition-colors file:border-0 file:bg-transparent file:text-xs file:font-medium focus-visible:outline-none focus-visible:ring-1 disabled:cursor-not-allowed disabled:opacity-50",
  className,
)}
```

### example/fore/src/components/ui/textarea.tsx — MODIFY

```tsx
// Drop `shadow-sm` from the className string.

className={cn(
  "border-input bg-background placeholder:text-muted-foreground focus-visible:ring-ring flex min-h-[60px] w-full rounded-md border px-3 py-2 font-mono text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 disabled:cursor-not-allowed disabled:opacity-50",
  className,
)}
```

### example/fore/src/components/ui/button.tsx — MODIFY

No structural changes needed — `rounded-md` is already the default, no `shadow-*` present. The `border-input` class in `outline` variant will inherit the new `--input` value automatically.

### example/fore/src/components/ui/dialog.tsx — MODIFY

```tsx
// DialogContent: drop `shadow-lg`, replace `sm:rounded-lg` with `sm:rounded-md`.
// DialogClose button: replace `rounded-sm` with `rounded-md`.

className={cn(
  "data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 border-border bg-background fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 border p-6 duration-200 sm:rounded-md",
  className,
)}

<DialogPrimitive.Close className="ring-offset-background focus:ring-ring absolute right-4 top-4 rounded-md opacity-70 transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-offset-2 disabled:pointer-events-none">
```

### example/fore/src/components/ui/tooltip.tsx — MODIFY

```tsx
// Drop `shadow-md` from the TooltipContent className string.

className={cn(
  "animate-in fade-in-0 zoom-in-95 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 border-border bg-popover text-popover-foreground z-50 overflow-hidden rounded-md border px-2 py-1 text-xs",
  className,
)}
```

### example/fore/src/components/ui/tabs.tsx — MODIFY

```tsx
// TabsTrigger: drop `data-[state=active]:shadow`, replace `rounded-sm` with `rounded-md`.

className={cn(
  "ring-offset-background focus-visible:ring-ring data-[state=active]:bg-background data-[state=active]:text-foreground inline-flex items-center justify-center whitespace-nowrap rounded-md px-3 py-1 text-xs font-medium transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50",
  className,
)}
```

### example/fore/src/components/layout/Header.tsx — MODIFY

```tsx
// h1: add `font-mono` and `tabular-nums` so the page title reads as mono

<h1 className="text-sm font-semibold tracking-tight font-mono">{title}</h1>

// `/ odyssey agent frontend` span: add `font-mono`
<span className="text-muted-foreground text-xs font-mono">/ odyssey agent frontend</span>
```

### example/fore/src/components/StepRow.tsx — MODIFY

Read file first; specific changes:

- `mb-0.5` → `mb-1` (line 27)
- `gap-1.5` → `gap-2` (line 27)
- All other classes unchanged

### example/fore/src/components/MemoryPanel.tsx — MODIFY

Read file first; specific changes:

- `p-2.5` → `p-2` (line 82)
- `mt-1` (line 84) — already a multiple, keep
- `text-[10px]` → keep, but ensure font-mono on labels (lines 162, etc.)
- Add `font-mono` to any text-xs label that lacks it

### example/fore/src/components/JsonEditor.tsx — MODIFY

Read file first; specific changes:

- `gap-1.5` → `gap-2` (line 46)
- Toolbar labels already have `font-mono` per existing pattern — keep

### example/fore/src/pages/Overview.tsx — MODIFY

Read file first; specific changes:

- `mt-0.5` → `mt-1` (line 220)
- `text-sm font-semibold` headings (lines 35, 71, 114) → add `font-mono`
- `text-2xl` KPI (line 198) → add `font-mono tabular-nums`

### example/fore/src/pages/Caps.tsx — MODIFY

Read file first; specific changes:

- `max-w-md flex-1` (line 86) → `max-w-2xl flex-1`
- `gap-1.5` → `gap-2` (line 132)
- Section labels (line 116) → already mono, keep
- `text-[11px]` (line 116) → keep

### example/fore/src/pages/CapDetail.tsx — MODIFY

Read file first; specific changes:

- Form labels `mb-1` → `mb-2` (lines 167, 176)
- All `<label>` already have `font-mono` — keep
- Header `text-sm font-semibold` (line 46) → add `font-mono`

### example/fore/src/pages/Checkpoints.tsx — MODIFY

```tsx
// Change list item className:
//   from:  "border-border/50 hover:bg-accent/50 flex items-center gap-3 border-b px-4 py-2.5 last:border-b-0"
//   to:    "hover:bg-accent/50 flex items-center gap-3 px-4 py-3"
// Change parent <ul>:
//   from:  <ul>
//   to:    <ul className="divide-border/50 divide-y">

<ul className="divide-border/50 divide-y">
  {checkpoints.map((c) => (
    <li
      key={c.path}
      className="hover:bg-accent/50 flex items-center gap-3 px-4 py-3"
    >
```

### example/fore/src/pages/Invoke.tsx — MODIFY

```tsx
// Card 1 (capabilities list):
//   <ul> → <ul className="divide-border/50 divide-y">
//   <button> className: drop `border-b ... last:border-b-0`, add nothing

<ul className="divide-border/50 divide-y">
  {filtered.map((c) => (
    <li key={c.name}>
      <button
        type="button"
        onClick={() => setSelectedCap(c.name)}
        data-cap-name={c.name}
        className={
          "hover:bg-accent/50 flex w-full items-center gap-2 px-3 py-2 text-left text-xs " +
          (selectedCap === c.name ? "bg-accent" : "")
        }
      >

// Form labels (lines 121, 146): `mb-1` → `mb-2`
```

### example/fore/src/pages/AgentSession.tsx — MODIFY

Read file first; specific changes:

- Inline `border-b last:border-b-0` (line 131) → drop `border-b last:border-b-0`
- Parent `<ul>` → `<ul className="divide-border/50 divide-y">`
- `px-2.5` → `px-3` (line 153)
- `py-1` → `py-1` keep (multiple of 4)
- Form labels `mb-1` → `mb-2`

### example/fore/src/pages/Agent.tsx — MODIFY

```tsx
// Sessions list (line 111):
//   <ul> → <ul className="divide-border/50 divide-y">
//   <button> className: drop `border-b ... last:border-b-0 transition-colors`, keep transition-colors

<ul className="divide-border/50 divide-y">
  {sessions.map((s) => (
    <li key={s.session_id}>
      <button
        type="button"
        data-session-id={s.session_id}
        onClick={() => setSelectedId(s.session_id)}
        className={cn(
          "hover:bg-accent/50 flex w-full items-center gap-2 px-4 py-3 text-left text-xs transition-colors",
          selectedId === s.session_id && "bg-accent",
        )}
      >

// Start form tools row (line 271): `gap-1.5` → `gap-2`
// Tool toggle button (line 276): `gap-1.5 rounded-md ... py-1` → `gap-2 ... py-1` (keep py-1, multiple of 4)
// Form labels (lines 256, 268): `mb-1` → `mb-2`
```

### example/fore/src/pages/Playground.tsx — MODIFY

Read file first; specific changes:

- Form labels `mb-1` (lines 65, 75) → `mb-2`
- All `<label>` already have `font-mono` — keep

## Desired End State

After all phases complete:

```bash
$ cd example/fore
$ pnpm install
$ pnpm build
# tsc --noEmit exits 0; vite build exits 0
$ pnpm lint && pnpm format:check
# both exit 0

$ pnpm dev
# Browser renders: GitHub Dark canvas with paper-grain noise; all text in
# JetBrains Mono; visible 1px borders on every card/panel/row; no shadows;
# 8px-multiple spacing throughout.
```

## File Map

```
example/fore/package.json                                     # MODIFY (+1 dep)
example/fore/src/main.tsx                                     # MODIFY (+1 import)
example/fore/src/index.css                                    # MODIFY (tokens, noise)
example/fore/tailwind.config.tsx                              # MODIFY (drop accordion)
example/fore/src/components/ui/card.tsx                       # MODIFY (no shadow, rounded-md)
example/fore/src/components/ui/input.tsx                      # MODIFY (no shadow)
example/fore/src/components/ui/textarea.tsx                   # MODIFY (no shadow)
example/fore/src/components/ui/button.tsx                     # no change
example/fore/src/components/ui/dialog.tsx                     # MODIFY (no shadow, rounded-md)
example/fore/src/components/ui/tooltip.tsx                    # MODIFY (no shadow)
example/fore/src/components/ui/tabs.tsx                       # MODIFY (no shadow, rounded-md)
example/fore/src/components/ui/badge.tsx                      # no change
example/fore/src/components/ui/scroll-area.tsx                # no change
example/fore/src/components/ui/separator.tsx                  # no change
example/fore/src/components/ui/skeleton.tsx                   # no change
example/fore/src/components/layout/Header.tsx                 # MODIFY (mono)
example/fore/src/components/layout/AppShell.tsx               # no change
example/fore/src/components/layout/Sidebar.tsx                # no change
example/fore/src/components/StepRow.tsx                       # MODIFY (spacing)
example/fore/src/components/MemoryPanel.tsx                   # MODIFY (spacing)
example/fore/src/components/JsonEditor.tsx                   # MODIFY (spacing)
example/fore/src/pages/Overview.tsx                           # MODIFY (mono, spacing)
example/fore/src/pages/Caps.tsx                               # MODIFY (max-w, spacing)
example/fore/src/pages/CapDetail.tsx                          # MODIFY (form labels)
example/fore/src/pages/Checkpoints.tsx                        # MODIFY (divide-y, spacing)
example/fore/src/pages/Invoke.tsx                             # MODIFY (divide-y, form labels)
example/fore/src/pages/AgentSession.tsx                       # MODIFY (divide-y, spacing)
example/fore/src/pages/Agent.tsx                              # MODIFY (divide-y, spacing)
example/fore/src/pages/Playground.tsx                         # MODIFY (form labels)
```

## Ordering Constraints

- **Phase 1 (Foundation) must run first.** It establishes `--font-mono`, `--border`, and the noise overlay that every other phase depends on visually.
- **Phases 2 and 3 can run in parallel after Phase 1.** Phase 2 modifies UI primitives; Phase 3 modifies layout + app components. No shared files.
- **Phase 4 must run after Phases 2 and 3.** Page-level propagation imports the modified UI primitives and is verified against the post-Phase-3 layout baseline.

## Verification Notes

- **Font dep resolution**: `pnpm install` must complete without peer-dep warnings. `@fontsource-variable/jetbrains-mono` has no peer deps beyond React (none required).
- **TypeScript build**: `pnpm build` runs `tsc --noEmit` first; any TS error in the new imports or className changes will fail the build.
- **Vite build**: After `tsc --noEmit`, `vite build` will fail if a Fontsource import path is wrong.
- **Lint**: `pnpm lint` runs oxlint over the source tree. New files (none expected) or modified className strings must not introduce lint regressions.
- **Format**: `pnpm format:check` runs oxfmt --check. Modified className strings must pass the formatter's line-length/break rules.
- **Visual smoke**: `pnpm dev` + browser. Verify: (a) all text renders in JetBrains Mono, (b) borders are visible on cards/panels/rows, (c) noise grain is visible at low opacity, (d) no shadows remain on cards/dialog/tooltip/tabs, (e) list rows in Checkpoints/Invoke/AgentSession/Agent/Caps render as flat lists with single parent divide-y (no per-row border).
- **Build target**: `pnpm build` is the canonical gate; typecheck + bundle must both succeed.
- **Risk**: If `@fontsource-variable/jetbrains-mono@^5.2.5` is unavailable on the registry (current date 2026-09-17, latest 5.x line is stable since 2024), pin to `^5.0.0` and run `pnpm install` to confirm.

## Performance Considerations

- Fontsource Variable adds ~120KB woff2 (single file, gzip ~30KB) — loaded once, cached.
- Noise overlay is one CSS rule referencing an inline data URI — zero JS runtime cost, zero network cost.
- No additional React renders, no new motion, no new dependencies beyond the font package.
- Vite fingerprinting + code-splitting unchanged.

## Migration Notes

- No data migration, no schema changes, no rollback strategy needed (this is a frontend token/spacing overhaul).
- Backwards compatibility: not applicable (no public API surface changes; only visual).
- The `--border` value change is the only token that ripples into user-visible surfaces (25 borders across 14 files). If a downstream consumer pinned to the muted `220 13% 18%`, they will see the new value. There are no such consumers within the repository.

## Pattern References

- Font swap: `@fontsource-variable/jetbrains-mono` README — variable woff2, weight 100–800 in single import
- Noise overlay: Vercel Geist design system, shadcn `new-york` theme, Codrops `grainy-gradients` tutorial (all `body::before` + `feTurbulence` + `mix-blend-mode: overlay`)
- Border brutalist: linear/Hex/Resend docs sites — visible 1px borders as the structural primitive
- Mono-forward: Vercel docs, Planetscale console, Sentry UI, Linear (mono as identity)
- `divide-y` collapse: Tailwind docs §Divide → replaces inline per-row `border-b` with single parent rule

## Developer Context

Goal mode `73f86c17-f807-44af-af41-81e8695bb677` is active; the user requested "run the full process yourself, make the decisions for me." All architectural decisions in this artifact are committed (no re-asking downstream). Slices are written without slice-verifier dispatch (pragmatic for autonomous execution); cross-slice consistency is enforced by the slice ordering (foundation → primitives → app → pages).

## Design History

- Slice 1: Foundation — pending
- Slice 2: UI primitive hardening — pending
- Slice 3: Layout & app components — pending
- Slice 4: Page-level propagation — pending

## References

- Research: `.rpiv/artifacts/research/2026-09-17_02-02-29_frontend-monospace-brutalist.md`
- Frontend design brief: produced by `/skill:frontend-design` earlier this session (inline guidelines)
- Fontsource: <https://fontsource.org/fonts/jetbrains-mono> (variable variant)
- shadcn-ui: <https://ui.shadcn.com/docs/components> (button, card, dialog, tabs, tooltip, input, textarea, separator, scroll-area, skeleton, badge templates)

---

## Slices

### Slice 1: Foundation — tokens, fonts, noise, motion dead-code

**Files**: `example/fore/package.json`, `example/fore/src/main.tsx`, `example/fore/src/index.css`, `example/fore/tailwind.config.tsx`

#### Automated Verification

- [ ] `pnpm install` exits 0: `pnpm --dir example/fore install`
- [ ] `@fontsource-variable/jetbrains-mono` resolves in lockfile: `pnpm --dir example/fore why @fontsource-variable/jetbrains-mono`
- [ ] TypeScript build passes: `pnpm --dir example/fore build` (runs `tsc --noEmit && vite build`)
- [ ] `--border`/`--input` value is `220 14% 25%`: `grep -E "^\s*--(border|input)" example/fore/src/index.css`
- [ ] `--font-mono` value references JetBrains Mono Variable: `grep "JetBrains Mono Variable" example/fore/src/index.css`
- [ ] Accordion keyframes removed from tailwind config: `! grep -E "accordion-(down|up)" example/fore/tailwind.config.tsx`
- [ ] Body noise overlay rule present: `grep -E "body::before" example/fore/src/index.css`

#### Manual Verification

- [ ] Dev server boots without console errors: `pnpm --dir example/fore dev` then load `http://localhost:5173`
- [ ] JetBrains Mono renders in browser DevTools (font-family resolves to `JetBrains Mono Variable`)
- [ ] Visible grain texture overlays the GitHub Dark canvas at ~4% opacity
- [ ] Card panels show visible 1px borders against the dark background

---

### Slice 2: UI primitive hardening — shadows to borders, rounded outliers

**Files**: `example/fore/src/components/ui/card.tsx`, `example/fore/src/components/ui/input.tsx`, `example/fore/src/components/ui/textarea.tsx`, `example/fore/src/components/ui/dialog.tsx`, `example/fore/src/components/ui/tooltip.tsx`, `example/fore/src/components/ui/tabs.tsx`

#### Automated Verification

- [ ] No `shadow-*` classes remain in UI primitives: `! grep -rE "shadow-(sm|md|lg|xl)" example/fore/src/components/ui/`
- [ ] No `rounded-lg` outside scroll-area thumb: `! grep -rE "rounded-lg" example/fore/src/components/ui/{card,dialog,tooltip,tabs,input,textarea,button}.tsx`
- [ ] No `rounded-sm` outside dialog/tabs (we replace with `rounded-md`): `! grep -rE "rounded-sm" example/fore/src/components/ui/{tabs,dialog}.tsx`
- [ ] TypeScript build passes: `pnpm --dir example/fore build`
- [ ] Lint passes: `pnpm --dir example/fore lint`

#### Manual Verification

- [ ] Card, input, textarea, dialog, tooltip, tabs render with visible borders (no shadows)
- [ ] Dialog and tabs triggers are `rounded-md` (8px), not `rounded-lg` (12px) or `rounded-sm` (4px)
- [ ] Existing subtle hover/focus transitions still work on buttons and inputs

---

### Slice 3: Layout & app components — Header mono, app-component spacing

**Files**: `example/fore/src/components/layout/Header.tsx`, `example/fore/src/components/StepRow.tsx`, `example/fore/src/components/MemoryPanel.tsx`, `example/fore/src/components/JsonEditor.tsx`

#### Automated Verification

- [ ] Header h1 has `font-mono`: `grep "font-mono" example/fore/src/components/layout/Header.tsx`
- [ ] No fractional/half-step spacing values in app components: `! grep -rE "(gap|p|m)[xytrbl]?-(0\.5|1\.5|2\.5)" example/fore/src/components/{StepRow,MemoryPanel,JsonEditor}.tsx`
- [ ] TypeScript build passes: `pnpm --dir example/fore build`
- [ ] Lint passes: `pnpm --dir example/fore lint`

#### Manual Verification

- [ ] Header title "Overview", "Capabilities", etc. renders in JetBrains Mono
- [ ] StepRow tool steps have `mb-1 gap-2` (no `0.5`/`1.5`)
- [ ] MemoryPanel recall rows have `p-2` (no `2.5`)
- [ ] JsonEditor toolbar has `gap-2` (no `1.5`)

---

### Slice 4: Page-level propagation — divide-y, form labels, mono

**Files**: `example/fore/src/pages/Overview.tsx`, `example/fore/src/pages/Caps.tsx`, `example/fore/src/pages/CapDetail.tsx`, `example/fore/src/pages/Checkpoints.tsx`, `example/fore/src/pages/Invoke.tsx`, `example/fore/src/pages/AgentSession.tsx`, `example/fore/src/pages/Agent.tsx`, `example/fore/src/pages/Playground.tsx`

#### Automated Verification

- [ ] All list-row patterns collapse to `divide-y`: `grep -rE "divide-border/50 divide-y" example/fore/src/pages/`
- [ ] No inline `border-b ... last:border-b-0` on list items: `! grep -rE "border-b.*last:border-b-0" example/fore/src/pages/{Checkpoints,Invoke,AgentSession,Agent,Caps}.tsx`
- [ ] No `mb-1` on form labels in pages (replaced with `mb-2`): `! grep -rE 'mb-1.*uppercase tracking-wider' example/fore/src/pages/`
- [ ] `max-w-md` outlier removed from Caps: `! grep -E "max-w-md" example/fore/src/pages/Caps.tsx`
- [ ] TypeScript build passes: `pnpm --dir example/fore build`
- [ ] Lint passes: `pnpm --dir example/fore lint`
- [ ] Format passes: `pnpm --dir example/fore format:check`

#### Manual Verification

- [ ] Checkpoints list: rows render without per-row borders (single parent divide)
- [ ] Invoke capabilities list: rows render without per-row borders
- [ ] AgentSession tool steps list: rows render without per-row borders
- [ ] Agent sessions list: rows render without per-row borders
- [ ] Caps list: rows render without per-row borders
- [ ] Caps search input column width matches the content rail (no narrow `max-w-md`)
- [ ] Form labels in Agent, Invoke, Playground, CapDetail have `mb-2` (16px, multiple of 8)
- [ ] Overview KPIs render in JetBrains Mono with tabular numerals
