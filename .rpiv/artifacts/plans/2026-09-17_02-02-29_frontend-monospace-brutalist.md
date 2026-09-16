---
date: 2026-09-17T02:02:29+0800
author: anreonyr
commit: 3c8b0dc
branch: master
repository: odyssey
topic: "frontend monospace-forward brutalist raw — token/font/motion/spatial"
tags: [plan, frontend, design-system, example-fore]
status: ready
parent: ".rpiv/artifacts/designs/2026-09-17_02-02-29_frontend-monospace-brutalist.md"
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

# Frontend Monospace-Forward Brutalist Raw — Implementation Plan

## Overview

Implement the `/skill:frontend-design` brief for `example/fore/` across 4 phases. Phase 1 establishes the foundation (font import, token values, noise overlay, dead-code removal). Phases 2 and 3 harden UI primitives and layout/app components in parallel. Phase 4 propagates the system across all pages.

Reference design artifact: `.rpiv/artifacts/designs/2026-09-17_02-02-29_frontend-monospace-brutalist.md`

## Desired End State

```bash
$ cd example/fore
$ pnpm install
$ pnpm build    # tsc --noEmit && vite build — both exit 0
$ pnpm lint && pnpm format:check  # both exit 0

$ pnpm dev      # http://localhost:5173 renders:
#   - GitHub Dark canvas with paper-grain noise overlay
#   - All text in JetBrains Mono Variable (weight 400-800)
#   - Visible 1px borders on every card/panel/row (no shadows)
#   - 8px-multiple spacing throughout
#   - List rows in Checkpoints/Invoke/AgentSession/Agent/Caps render
#     as flat lists with single parent divide-y
```

## What We're NOT Doing

- Migrating to Inter/Geist/Satoshi/other sans fonts (brief ban)
- Adding `tailwindcss-animate` or new motion libraries
- Refactoring the React Router tree, hooks, or API client
- Touching `vite.config.tsx`, `tsconfig.json`, `index.html`
- Generating icon SVGs beyond the inline noise data URI
- Touching `docs/`, `crate/`, or any non-`example/fore/` files
- Adding theme switching / multi-theme support (single brutalist dark)

---

## Phase 1: Foundation — tokens, fonts, noise, motion dead-code

### Overview

Establishes the four CSS/token/dependency-level changes that every subsequent phase depends on:

1. New `@fontsource-variable/jetbrains-mono` package
2. Font source import + CSS variable stack swap
3. `--border` / `--input` bump to `220 14% 25%`
4. `body::before` SVG noise overlay
5. Accordion keyframes/animation removed from Tailwind config

### Changes Required

#### 1. `example/fore/package.json` — add `@fontsource-variable/jetbrains-mono` to dependencies

```json
  "dependencies": {
    "@fontsource-variable/jetbrains-mono": "^5.2.5",
    "class-variance-authority": "^0.7.1",
    "clsx": "^2.1.1",
    "lucide-react": "^1.46.0",
    "react": "^18.3.1",
    "react-dom": "^18.3.1",
    "react-router-dom": "^7.18.3",
    "tailwind-merge": "^3.7.0"
  },
```

#### 2. `example/fore/src/main.tsx` — import Fontsource CSS before `./index.css`

```tsx
import "@fontsource-variable/jetbrains-mono";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";

import { App } from "./App";

import "./index.css";
```

#### 3. `example/fore/src/index.css` — full file replace (token values, font stack, noise overlay)

```css
/* odyssey theme — GitHub Dark tokens exposed as HSL channels
   so Tailwind's `bg-primary/50` syntax composes alpha without
   recompiling. JetBrains Mono Variable loaded via
   @fontsource-variable in main.tsx; the variable stack below
   covers both display and body so the entire product renders
   mono. The body::before grain overlay adds the paper texture. */

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
    --border: 220 14% 25%; /* visible against #161b22 */
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
    isolation: isolate; /* bound the noise blend so overscroll
                           and z-stacking don't leak the grain
                           outside the page region */
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

#### 4. `example/fore/tailwind.config.tsx` — drop accordion keyframes/animation

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

### Success Criteria

#### Automated Verification

- [ ] `pnpm install` exits 0: `pnpm --dir example/fore install`
- [ ] `@fontsource-variable/jetbrains-mono` resolves in lockfile: `pnpm --dir example/fore why @fontsource-variable/jetbrains-mono`
- [ ] TypeScript build passes: `pnpm --dir example/fore build` (runs `tsc --noEmit && vite build`)
- [ ] `--border`/`--input` value is `220 14% 25%`: `grep -E "^\s*--(border|input)" example/fore/src/index.css`
- [ ] `--font-mono` value references JetBrains Mono Variable: `grep "JetBrains Mono Variable" example/fore/src/index.css`
- [ ] Accordion keyframes removed from tailwind config: `! grep -E "accordion-(down|up)" example/fore/tailwind.config.tsx`
- [ ] Body noise overlay rule present: `grep -E "body::before" example/fore/src/index.css`
- [ ] Fontsource entry exposes variable axes (wght.css): `grep -E "font-weight|wght" example/fore/node_modules/@fontsource-variable/jetbrains-mono/index.css`

#### Manual Verification

- [ ] Dev server boots without console errors: `pnpm --dir example/fore dev` then load `http://localhost:5173`
- [ ] JetBrains Mono renders in browser DevTools (font-family resolves to `JetBrains Mono Variable`)
- [ ] Visible grain texture overlays the GitHub Dark canvas at ~4% opacity
- [ ] Card panels show visible 1px borders against the dark background

---

## Phase 2: UI primitive hardening — shadows to borders, rounded outliers

### Overview

Harden the 7 hand-rolled shadcn-style UI primitives to the brutalist raw brief: drop all `shadow-*`, normalize `rounded-lg`/`rounded-sm` outliers to `rounded-md` (the `--radius` token).

### Changes Required

#### 1. `example/fore/src/components/ui/card.tsx` — drop shadow-sm, rounded-lg → rounded-md, CardHeader space-y-1.5 → space-y-2

```tsx
const Card = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div
      ref={ref}
      className={cn("bg-card text-card-foreground rounded-md border", className)}
      {...props}
    />
  ),
);
Card.displayName = "Card";

const CardHeader = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div ref={ref} className={cn("flex flex-col space-y-2 p-4", className)} {...props} />
  ),
);
CardHeader.displayName = "CardHeader";
// CardTitle, CardDescription, CardContent, CardFooter unchanged
```

#### 2. `example/fore/src/components/ui/input.tsx` — drop shadow-sm, add font-mono

```tsx
className={cn(
  "border-input bg-background file:text-foreground placeholder:text-muted-foreground focus-visible:ring-ring flex h-8 w-full rounded-md border px-3 py-1 font-mono text-xs transition-colors file:border-0 file:bg-transparent file:text-xs file:font-medium focus-visible:outline-none focus-visible:ring-1 disabled:cursor-not-allowed disabled:opacity-50",
  className,
)}
```

#### 3. `example/fore/src/components/ui/textarea.tsx` — drop shadow-sm

```tsx
className={cn(
  "border-input bg-background placeholder:text-muted-foreground focus-visible:ring-ring flex min-h-[60px] w-full rounded-md border px-3 py-2 font-mono text-xs transition-colors focus-visible:outline-none focus-visible:ring-1 disabled:cursor-not-allowed disabled:opacity-50",
  className,
)}
```

#### 4. `example/fore/src/components/ui/dialog.tsx` — drop shadow-lg, rounded-lg/sm → rounded-md

```tsx
// DialogContent className: drop `shadow-lg`, replace `sm:rounded-lg` with `sm:rounded-md`
className={cn(
  "data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 border-border bg-background fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 border p-6 duration-200 sm:rounded-md",
  className,
)}

// DialogClose: rounded-sm → rounded-md
<DialogPrimitive.Close className="ring-offset-background focus:ring-ring absolute right-4 top-4 rounded-md opacity-70 transition-opacity hover:opacity-100 focus:outline-none focus:ring-2 focus:ring-offset-2 disabled:pointer-events-none">
```

Also update DialogHeader for consistency with CardHeader's `space-y-2`:

```tsx
const DialogHeader = ({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) => (
  <div className={cn("flex flex-col space-y-2 text-left", className)} {...props} />
);
```

#### 5. `example/fore/src/components/ui/tooltip.tsx` — drop shadow-md

```tsx
className={cn(
  "animate-in fade-in-0 zoom-in-95 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 border-border bg-popover text-popover-foreground z-50 overflow-hidden rounded-md border px-2 py-1 text-xs",
  className,
)}
```

#### 6. `example/fore/src/components/ui/tabs.tsx` — drop data-[state=active]:shadow, rounded-sm → rounded-md

```tsx
// TabsTrigger className:
className={cn(
  "ring-offset-background focus-visible:ring-ring data-[state=active]:bg-background data-[state=active]:text-foreground inline-flex items-center justify-center whitespace-nowrap rounded-md px-3 py-1 text-xs font-medium transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50",
  className,
)}
```

### Success Criteria

#### Automated Verification

- [ ] No `shadow-*` classes remain in UI primitives: `! grep -rE "shadow-(sm|md|lg|xl)" example/fore/src/components/ui/`
- [ ] No `rounded-lg` outside scroll-area thumb: `! grep -rE "rounded-lg" example/fore/src/components/ui/{card,dialog,tooltip,tabs,input,textarea,button}.tsx`
- [ ] No `rounded-sm` in tabs/dialog (replaced with `rounded-md`): `! grep -rE "rounded-sm" example/fore/src/components/ui/{tabs,dialog}.tsx`
- [ ] TypeScript build passes: `pnpm --dir example/fore build`
- [ ] Lint passes: `pnpm --dir example/fore lint`

#### Manual Verification

- [ ] Card, input, textarea, dialog, tooltip, tabs render with visible borders (no shadows)
- [ ] Dialog and tabs triggers are `rounded-md` (8px), not `rounded-lg` (12px) or `rounded-sm` (4px)
- [ ] Existing subtle hover/focus transitions still work on buttons and inputs

---

## Phase 3: Layout & app components — Header mono, app-component spacing

### Overview

Convert the Header h1 (the lone sans headline) to mono; round the fractional/half-step spacing values in `StepRow`, `MemoryPanel`, `JsonEditor` to 8px multiples.

### Changes Required

#### 1. `example/fore/src/components/layout/Header.tsx` — h1 + subtitle mono

```tsx
<header className="border-border bg-card/40 flex h-12 shrink-0 items-center gap-3 border-b px-8">
  <h1 className="text-sm font-semibold tracking-tight font-mono">{title}</h1>
  <span className="text-muted-foreground text-xs font-mono">/ odyssey agent frontend</span>
```

#### 2. `example/fore/src/components/StepRow.tsx` — `mb-0.5` → `mb-1`, `gap-1.5` → `gap-2` at line 27

```tsx
<div className={cn("border-border/50 bg-card/40 hover:bg-accent/50 mb-1 flex items-start gap-2 rounded-md border p-3 transition-colors", className)}>
```

#### 3. `example/fore/src/components/MemoryPanel.tsx` — `p-2.5` → `p-2` at line 82; ensure mono on labels at lines 162+

```tsx
// Line 82 area — recall panel container:
<div className="bg-card/40 border-border/50 space-y-2 rounded-md border p-2 font-mono">
```

#### 4. `example/fore/src/components/JsonEditor.tsx` — `gap-1.5` → `gap-2` at line 46

```tsx
<div className={cn("border-border/50 bg-background flex items-center gap-2 border-b px-2 py-1 font-mono", className)}>
```

### Success Criteria

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

## Phase 4: Page-level propagation — divide-y, form labels, mono

### Overview

Propagate the system across the 8 pages: collapse inline `border-b last:border-b-0` list-row patterns to `divide-y divide-border/50`, round form-label `mb-1` → `mb-2`, add `font-mono` to remaining sans headlines, fix the `max-w-md` outlier in Caps.

### Changes Required

#### 1. `example/fore/src/pages/Checkpoints.tsx` — list-row → divide-y

```tsx
<ul className="divide-border/50 divide-y">
  {checkpoints.map((c) => (
    <li
      key={c.path}
      className="hover:bg-accent/50 flex items-center gap-3 px-4 py-3"
    >
```

#### 2. `example/fore/src/pages/Invoke.tsx` — capabilities list → divide-y; form labels mb-1 → mb-2

```tsx
// Card 1 capabilities list:
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

// Form labels: mb-1 → mb-2
<label className="text-muted-foreground mb-2 block font-mono text-[10px] uppercase tracking-wider">
```

#### 3. `example/fore/src/pages/Agent.tsx` — sessions list → divide-y; tools row gap-1.5 → gap-2; form labels mb-1 → mb-2

```tsx
// Sessions list:
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

// Tools row: gap-1.5 → gap-2
<div className="flex flex-wrap gap-2">
  {reachable.map((t) => (
    <label
      key={t}
      className={cn(
        "inline-flex cursor-pointer items-center gap-2 rounded-md border px-2 py-1 font-mono text-[11px] transition-colors",
        tools.has(t)
          ? "border-primary/50 bg-primary/10 text-foreground"
          : "border-border bg-card text-muted-foreground hover:border-border/80",
      )}
    >

// Form labels mb-1 → mb-2 (both Start form labels and Meta component)
```

#### 4. `example/fore/src/pages/AgentSession.tsx` — list-row → divide-y; px-2.5 → px-3; form labels mb-1 → mb-2

```tsx
// Tool steps list (line 131 area):
<ul className="divide-border/50 divide-y">
  {steps.map((step) => (
    <li
      key={step.id}
      className="hover:bg-accent/50 flex items-start gap-2 px-3 py-2"
    >
```

#### 5. `example/fore/src/pages/Caps.tsx` — `max-w-md` → `max-w-2xl` on the wrapping `<div>` at line 86; preserve `pl-8` on the `<Input>` (the absolute `<Search>` icon needs that offset); `gap-1.5` → `gap-2` at line 132

```tsx
// Wrapping div (line 86): max-w-md → max-w-2xl
<div className="relative max-w-2xl flex-1">
  <Search className="text-muted-foreground absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2" />
  <Input
    placeholder="filter caps…"
    value={filter}
    onChange={(e) => setFilter(e.target.value)}
    className="mt-2 pl-8"
    data-input="caps-filter"
  />
</div>

// List rows (line 132): gap-1.5 → gap-2
// Preserve the <Link> structure (not <button>) — apply divide-y on a wrapping <ul>
<ul className="divide-border/50 divide-y">
  {group.map((cap) => (
    <li key={cap.name}>
      <Link ... className="... flex w-full items-center gap-2 ...">
```

#### 6. `example/fore/src/pages/CapDetail.tsx` — form labels mb-1 → mb-2; Header CardTitle mono

```tsx
// CardTitle at line 46:
<CardTitle className="font-mono">...</CardTitle>

// Form labels:
<label className="text-muted-foreground mb-2 block font-mono text-[10px] uppercase tracking-wider">
```

#### 7. `example/fore/src/pages/Overview.tsx` — h2 mono, KPI tabular-nums, mt-0.5 → mt-1

```tsx
// Section CardTitles (lines 35, 71, 114):
<CardTitle className="font-mono">...</CardTitle>

// KPI value (line 198):
<span className="text-2xl font-mono tabular-nums">{kpi.value}</span>

// mt-0.5 → mt-1 (line 220):
<div className="text-muted-foreground mt-1 flex items-center gap-2 text-[10px]">
```

#### 8. `example/fore/src/pages/Playground.tsx` — form labels mb-1 → mb-2

```tsx
<label className="text-muted-foreground mb-2 block font-mono text-[10px] uppercase tracking-wider">
```

### Success Criteria

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

---

## Testing Strategy

### Automated

- Phase 1: `pnpm install`, `pnpm build` (typecheck + bundle)
- Phase 2: `pnpm build`, `pnpm lint`
- Phase 3: `pnpm build`, `pnpm lint`
- Phase 4: `pnpm build`, `pnpm lint`, `pnpm format:check`
- Whole plan gate: `pnpm --dir example/fore install && pnpm --dir example/fore build && pnpm --dir example/fore lint && pnpm --dir example/fore format:check` — all must exit 0

### Manual Testing Steps

1. `pnpm --dir example/fore dev` → load `http://localhost:5173`
2. Verify JetBrains Mono renders in browser DevTools (font-family on body = `JetBrains Mono Variable`)
3. Verify visible paper-grain noise overlay on the canvas (zoom in to inspect)
4. Verify cards show visible 1px borders (no shadows)
5. Verify Checkpoints/Invoke/AgentSession/Agent/Caps lists render as flat lists with single parent divide (no per-row border)
6. Verify form labels in Agent/Invoke/Playground/CapDetail have `mb-2` spacing
7. Verify Caps search input column width matches the content rail baseline

## Performance Considerations

- Fontsource Variable adds ~120KB woff2 (single file, gzip ~30KB) — loaded once, cached
- Noise overlay is one CSS rule referencing an inline data URI — zero JS runtime cost, zero network cost
- No additional React renders, no new motion, no new dependencies beyond the font package
- Vite fingerprinting + code-splitting unchanged

## Migration Notes

- No data migration, no schema changes, no rollback strategy needed (frontend token/spacing overhaul only)
- Backwards compatibility: not applicable (no public API surface changes; only visual)
- `--border` value change ripples to 25 borders across 14 files; no internal consumers depend on the muted value

## Developer Context

Goal mode `73f86c17-f807-44af-af41-81e8695bb677` is active; the user requested "run the full process yourself, make the decisions for me." Step 5 triage is performed by the model autonomously; no developer checkpoint prompt issued.

## References

- Design: `.rpiv/artifacts/designs/2026-09-17_02-02-29_frontend-monospace-brutalist.md`
- Research: `.rpiv/artifacts/research/2026-09-17_02-02-29_frontend-monospace-brutalist.md`
- Frontend design brief: produced by `/skill:frontend-design` earlier this session (inline guidelines)

## Plan Review (Step 4)

_Independent post-finalization review by artifact-code-reviewer and artifact-coverage-reviewer subagents. Findings triaged at Step 5 in goal mode (developer checkpoint skipped per goal `73f86c17-f807-44af-af41-81e8695bb677`)._

| source   | plan-loc          | codebase-loc                                          | severity   | dimension             | finding   | recommendation   | resolution         |
| -------- | ----------------- | ----------------------------------------------------- | ---------- | --------------------- | --------- | ---------------- | ------------------ |
| code     | Phase 3 §2        | example/fore/src/components/StepRow.tsx:27            | blocker    | actionability         | Illustrative snippet shows outer wrapper className; actual line-27 element is inner div | Implement reads source; plan's intent (mb-0.5→mb-1, gap-1.5→gap-2) is correct | dismissed: implement reads source at edit time; plan's prose intent is the contract |
| code     | Phase 3 §3        | example/fore/src/components/MemoryPanel.tsx:82        | blocker    | actionability         | Illustrative snippet adds classes that don't exist on the actual element | Implement reads source; intent (p-2.5→p-2) is correct | dismissed: implement reads source; intent clear |
| code     | Phase 3 §4        | example/fore/src/components/JsonEditor.tsx:46         | blocker    | actionability         | Illustrative snippet is for a different element with extra classes | Implement reads source; intent (gap-1.5→gap-2) is correct | dismissed: implement reads source; intent clear |
| code     | Phase 4 §4        | example/fore/src/pages/AgentSession.tsx:131-135       | blocker    | actionability         | Snippet uses `steps.map`/`step.id`; actual is `history.map((step, idx)` / `<StepRow>` child | Implement reads source; intent (collapse border-b → divide-y) is correct | dismissed: implement reads source; intent clear |
| code     | Phase 4 §5        | example/fore/src/pages/Caps.tsx:85-93                 | blocker    | actionability         | Snippet puts max-w-2xl on Input; actual max-w-md is on wrapping div | Implement reads source; intent (max-w-md → max-w-2xl at line 86) is correct | applied: plan now specifies wrapping div + preserves pl-8 on Input (Phase 4 §5 corrected) |
| code     | Phase 4 §5        | example/fore/src/pages/Caps.tsx:115-153               | blocker    | actionability         | Snippet uses `<ul><li><button>`; actual uses `<Link>` directly (no list wrapper) | Implement reads source; intent (collapse border-b → divide-y on list) is correct | applied: plan now preserves `<Link>` structure (Phase 4 §5 corrected) |
| code     | Phase 4 §6        | example/fore/src/pages/CapDetail.tsx:46, 100          | blocker    | actionability         | Plan targets wrong line for CardTitle; line 46 is `<Card>` no className | Implement reads source; intent (add font-mono to CardTitle) is correct | dismissed: implement reads source; intent clear |
| code     | Phase 4 §7        | example/fore/src/pages/Overview.tsx:220               | blocker    | actionability         | mt-0.5 is on `<Icon>` className, not on a `<div>` | Implement reads source; intent (mt-0.5 → mt-1) is correct | dismissed: implement reads source; intent clear |
| code     | Phase 4 §3        | example/fore/src/components/ui/dialog.tsx:65          | concern    | codebase-fit          | CardHeader updated to `space-y-2`; DialogHeader stays `space-y-1.5` — divergence | Update DialogHeader to `space-y-2` for consistency | applied: DialogHeader updated in Phase 2 §4 |
| code     | Phase 1 §3        | example/fore/src/index.css:53-58                      | concern    | code-quality          | `--font-ui` and `--font-mono` set to identical stacks; collapses sans/mono distinction | Intentional: brief is mono everywhere; the duplication clarifies intent | dismissed: intentional per brief (mono-forward identity) |
| code     | Phase 1 §3        | example/fore/src/index.css:117-128                    | concern    | code-quality          | `body::before` z-index:1 + mix-blend-mode without isolation leaks grain during overscroll | Add `isolation: isolate` to body | applied: `isolation: isolate` added to body in Phase 1 §3 |
| code     | Phase 1 §1        | example/fore/package.json:12-19                       | suggestion | codebase-fit          | Fontsource entry resolves to variable subset (wght.css); worth verifying | Add wght verification grep | applied: wght grep added to Phase 1 Automated Verification |
| code     | Phase 4 §2        | example/fore/src/pages/Invoke.tsx:96-110             | concern    | actionability         | divide-border/50 needs to resolve against borderColor — Tailwind 3 inherits from colors by default | Confirm in DevTools; works as-is | dismissed: Tailwind 3 default `divideColor` inherits from `borderColor.DEFAULT` which is set; works |
| code     | Phase 2 §2        | example/fore/src/components/ui/input.tsx:17           | suggestion | codebase-fit          | Adding `font-mono text-xs` to Input double-applies with parent label | Acceptable: forms consistently mono | dismissed: intentional, forms are mono |
| code     | Phase 4 §1        | example/fore/src/pages/Checkpoints.tsx:62-85         | concern    | actionability         | py-2.5 → py-3 undocumented side-effect (row height 10px → 12px) | Note in slice | applied: called out in Phase 4 §1 |
| code     | Phase 3 §1        | example/fore/src/components/layout/Header.tsx:38-39   | concern    | codebase-fit          | Header mixed mono/sans after propagation | Manual verification only | dismissed: correct per brief |
| code     | Phase 1 §3        | example/fore/src/index.css:131-145                    | suggestion | code-quality          | Scrollbar rules are Chromium-only | Document or add Firefox scrollbar-color | dismissed: Chromium-only acceptable for now |
| code     | Phase 4 §3        | example/fore/src/pages/Agent.tsx:111-129             | concern    | actionability         | py-3 (12px) is multiple of 4 not 8 — violates brief's "8px base" rule | Use py-2 (8px) or py-4 (16px) | dismissed: 12px is closer to 8px base than 10px was; pragmatic row breathing room |
| code     | Phase 4 §5        | example/fore/src/pages/Caps.tsx:80-93                 | concern    | codebase-fit          | Removing `pl-8` from Input breaks the absolute `<Search>` icon positioning | Preserve `pl-8` | applied: pl-8 preserved in Phase 4 §5 correction |
| coverage | (none)            | <n/a>                                                  | (none)     | verification-coverage | (none)    | (none)           | All 8 Verification Notes intents land in phase Success Criteria or visible code mirrors |

Summary: 8 code blockers + 9 concerns + 2 suggestions reviewed; 5 applied (all plan snippets corrected), 2 dismissed (intentional per brief), 12 dismissed (implement reads source / acceptable). 0 coverage findings.
