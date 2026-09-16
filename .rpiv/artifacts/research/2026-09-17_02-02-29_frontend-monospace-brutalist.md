---
date: 2026-09-17T02:02:29+0800
author: anreonyr
commit: 3c8b0dc
branch: master
repository: odyssey
topic: "frontend monospace-forward brutalist raw — token/font/motion/spatial"
tags: [research, frontend, design-system, example-fore]
status: ready
last_updated: 2026-09-17T02:02:29+0800
last_updated_by: anreonyr
---

# Research: Frontend Monospace-Forward Brutalist Raw

## Summary

Implement the aesthetic brief produced by `/skill:frontend-design` for `example/fore/` (Vite + React + Tailwind + hand-rolled shadcn-style UI). Concrete changes: swap system mono to JetBrains Mono via Fontsource Variable; thicken `--border` (and `--input` in lockstep) for visible brutalist structure; add a `body::before` SVG `feTurbulence` noise overlay; drop dead-code accordion keyframes; flatten shadow/rounded outliers across UI primitives; collapse inline row separators to `divide-y`; round ~30 fractional spacing values to 8px multiples.

## Requirements

- Monospace-forward: every label/header/number uses a real mono (JetBrains Mono Variable), not the system stack
- Brutalist raw: visible 1px borders replace shadows; rounded outliers gone; structured grid baseline honored
- Subtle micro-interactions: keep `transition-colors`/`-opacity`/`-all`; drop the unused accordion keyframes
- Solid + noise/grain: paper-grain texture over the GitHub Dark body
- No new dependencies beyond `@fontsource-variable/jetbrains-mono` (one package, one woff2 file)
- Type-safe (`pnpm build` runs `tsc --noEmit && vite build`); passes lint/format

## Current State Analysis

### Codebase shape

- 43 source files across `example/fore/` (11 UI primitives, 3 layout, 3 app components, 8 pages, 1 `App.tsx`, 1 `main.tsx`, 1 `index.css`)
- Tailwind 3.4 with HSL-channel token system in `src/index.css:11-58`
- shadcn-style primitives hand-rolled in `src/components/ui/`
- Token discipline: every color is `hsl(var(--token) / <alpha-value>)` per `tailwind.config.tsx:1-5`

### Key Discoveries

**1. No webfont imports anywhere.** `src/index.css`, `src/main.tsx`, `index.html`, every `.tsx` under `src/`, `package.json` deps — zero `@font-face`, zero `@import`, zero `<link rel="stylesheet">`, zero `@fontsource/*`. Clean slate for a Fontsource add.

**2. Mono is already pervasive.** `font-mono` Tailwind utility used 53× across 16 files; `--font-mono` already feeds `tailwind.config.tsx:79` (`fontFamily.mono`); body default falls back to `var(--font-ui)` at `src/index.css:69`. The only "real" sans headline is `Header.tsx:40` (`text-sm font-semibold tracking-tight`) — design brief says convert to mono too.

**4. `--border` blast radius is wide but safe.** 22× `border-border` across 14 files; 3× `border-input` (separate token, identical value `220 13% 18%`) — both must move in lockstep. Also feeds WebKit scrollbar thumb (`src/index.css:106`), `Separator` (`components/ui/separator.tsx:17`), `ScrollArea` thumb (`components/ui/scroll-area.tsx:43`). No `divide-*`, no `ring-border`, no `focus:border-*` — no semantic dependency on the muted shade.

**5. Accordion keyframes are dead code.** `tailwind.config.tsx:73-82` defines `accordion-down`/`accordion-up` keyframes and matching `animation` entries. Zero `animate-accordion-*` references anywhere in `src/`. No Radix Accordion primitive in `src/components/ui/`. Safe to delete.

**6. Existing subtle motion is independent.** `transition-colors` ×9 sites, `transition-opacity` ×1, `transition-all` ×1, `duration-200` ×1 — all Tailwind defaults, all hover/focus/dialog micro-interactions. None reference the accordion keyframes. Keep as-is.

**7. Spatial layout is ~85% aligned with brief.** `max-w-6xl px-8 py-6` baseline at `AppShell.tsx:26`. One outlier: `max-w-md` at `pages/Caps.tsx:86` (search input column). 6 `shadow-*` usages, all in `components/ui/*` (clean blast radius). `rounded-full` only at `scroll-area.tsx:43` (defensible — scrollbar thumb). 5 inline `border-b` list-row separators (Checkpoints, Invoke, AgentSession, Agent, Caps) → can collapse to `divide-y`. ~30 fractional/half-step spacing values (heaviest: `mb-1` ×6 in form labels, `gap-1` ×7 in chip rows, `py-1` ×5 in button/badge padding).

**8. No noise/grain/pattern/svg-filter exists.** `body::before` with inline SVG `feTurbulence` data URI is the clean precedent (used by Vercel Geist, shadcn `new-york`, Codrops `grainy-gradients`). Pure-CSS, no JSX render, no binary asset. Add in `@layer base` block alongside existing body rules at `src/index.css:66-80`.

## File Inventory

```
example/fore/
├── index.html                      # no font link, no changes needed
├── package.json                    # +1 dep: @fontsource-variable/jetbrains-mono
├── tailwind.config.tsx             # MODIFY — drop accordion keyframes/animations
├── vite.config.tsx                 # no changes
├── tsconfig.json                   # no changes
├── src/
│   ├── main.tsx                    # MODIFY — import fontsource CSS
│   ├── App.tsx                     # no changes
│   ├── index.css                   # MODIFY — token values, noise overlay, font stack
│   ├── lib/utils.tsx               # no changes
│   ├── components/
│   │   ├── layout/
│   │   │   ├── AppShell.tsx        # no changes (baseline)
│   │   │   ├── Header.tsx          # MODIFY — h1 mono
│   │   │   └── Sidebar.tsx         # no changes (already mono)
│   │   ├── ui/
│   │   │   ├── badge.tsx           # no changes (already mono)
│   │   │   ├── button.tsx          # MODIFY — drop shadow-sm, drop rounded-md→border
│   │   │   ├── card.tsx            # MODIFY — drop shadow-sm, rounded-lg→md
│   │   │   ├── dialog.tsx          # MODIFY — drop shadow-lg, rounded-lg→md, rounded-sm→md
│   │   │   ├── input.tsx           # MODIFY — drop shadow-sm
│   │   │   ├── scroll-area.tsx     # no changes (rounded-full on thumb is utility)
│   │   │   ├── separator.tsx       # no changes (already 1px)
│   │   │   ├── skeleton.tsx        # no changes
│   │   │   ├── tabs.tsx            # MODIFY — drop data-[state=active]:shadow, rounded-sm→md
│   │   │   ├── textarea.tsx        # MODIFY — drop shadow-sm
│   │   │   └── tooltip.tsx         # MODIFY — drop shadow-md
│   │   ├── JsonEditor.tsx          # MODIFY — spacing fix (gap-1.5→gap-2)
│   │   ├── MemoryPanel.tsx         # MODIFY — p-2.5→p-2, mono labels
│   │   └── StepRow.tsx             # MODIFY — mb-0.5/gap-1.5→mb-1/gap-2
│   └── pages/
│       ├── Overview.tsx            # MODIFY — h2 mono, mt-0.5→mt-1
│       ├── Caps.tsx                # MODIFY — max-w-md→max-w-2xl, gap-1.5→gap-2, mono labels
│       ├── CapDetail.tsx           # MODIFY — form label mb-1→mb-2, mono propagation
│       ├── Checkpoints.tsx         # MODIFY — border-b list→divide-y, py-2.5→py-3, mt-0.5→mt-1
│       ├── Invoke.tsx              # MODIFY — border-b list→divide-y, form labels mb-1→mb-2
│       ├── AgentSession.tsx        # MODIFY — border-b list→divide-y, px-2.5→px-3
│       ├── Agent.tsx               # MODIFY — border-b list→divide-y, py-2.5→py-3, gap-1.5→gap-2
│       └── Playground.tsx          # MODIFY — mb-1→mb-2 form labels
```

## Integration Points

- **Token system (`src/index.css:11-58`)** — single source of truth for color/font/radius; Tailwind config token-agnostic per `tailwind.config.tsx:1-5` comment. Editing `:root` propagates everywhere.
- **Tailwind config (`tailwind.config.tsx:79`)** — `fontFamily.mono: ["var(--font-mono)"]` already routes the CSS variable to the utility. No config change needed for the font swap; the variable value change does it.
- **Fontsource integration (`src/main.tsx:11`)** — already imports `./index.css`; add `import "@fontsource-variable/jetbrains-mono";` as a separate line. Vite fingerprints and inlines via the package's CSS — zero `@font-face` declarations needed.
- **AppShell (`AppShell.tsx:21`)** — root chrome: `<div className="bg-background text-foreground flex h-screen w-full overflow-hidden">`. Any fixed-position noise overlay on `<body>` covers the viewport above this div (which sits inside the body's stacking context).
- **`Separator` primitive (`components/ui/separator.tsx:18`)** — already `h-[1px] w-full bg-border`; brief's pattern matches. No change.
- **Layout baseline (`AppShell.tsx:26`)** — `mx-auto h-full w-full max-w-6xl px-8 py-6`. All inner pages render into this rail.

## Architecture Insights

- **HSL channel triplet pattern** is the load-bearing design decision. Every color is `hsl(var(--token) / <alpha-value>)` so Tailwind's `/N` opacity modifier composes without recompiling. This is why the system survives re-themes by editing only `src/index.css`.
- **shadcn-style primitives are hand-rolled, not generated.** No `npx shadcn-ui` dep. Editing them is a normal `git diff`, not a regeneration. This makes the brutalist pass tractable — each shadow/rounded edit is a one-line change.
- **mono adoption is already 53× out of 14 files.** The "mono everywhere" brief is mostly already in place; the work is closing the remaining sans gap (Header h1) and propagating forward where new code drops in.
- **`--input` and `--border` are pinned to identical values today.** This is a footgun, not a feature. Brief wants them bumped together; that's the right move — both at the same value preserves the current coupling.

## Precedents & Lessons

- **No git history precedent** — this is the first design system overhaul for `example/fore/`. No similar past changes to compare. (`precedent-locator` skipped — repo has 0 commits touching these files beyond the original scaffolding per `git log`.)
- **Vercel Geist grain layer** uses the same `body::before` + `feTurbulence` data URI pattern as recommended by the noise-overlay research. Low-opacity `mix-blend-mode: overlay` is the consensus.
- **JetBrains Mono Variable** is the standard pick for dev-tool mono-forward projects; single woff2, weight axis 100–800, ~120KB. Not in the "distinctive but overused" trap (Geist Mono is).

## Open Questions

None — every dimension is settled by the brief and codebase audit.

## Developer Context

Q: Run the full pipeline autonomously, decisions made by the model?
A: Yes (per goal mode `73f86c17-f807-44af-af41-81e8695bb677`). All architectural decisions in this research are committed; downstream stages will not re-ask.

## References

- Frontend design brief (inline, this session): tone Brutalist/raw, color GitHub Dark (existing), typography monospace-forward (JetBrains Mono), motion subtle micro-interactions, spatial structured grid symmetric, backgrounds solid + noise/grain, differentiation typography as art
- `example/fore/src/index.css:11-58` — token definitions
- `example/fore/tailwind.config.tsx:1-89` — Tailwind config + accordion dead code at `:73-82`
- `example/fore/src/components/layout/AppShell.tsx:21-26` — layout baseline
- `example/fore/package.json` — scripts (`build` runs `tsc --noEmit && vite build`); no current `@fontsource/*`
- Research finding files (this artifact's evidence):
  - Font audit: 53× `font-mono`, 0× `font-sans`, 3 direct `font-family` refs all in `index.css`
  - Border audit: 22× `border-border`, 3× `border-input`, 0× `divide-*`, 0× `ring-border`
  - Motion audit: 0× `animate-accordion-*`, no Accordion primitive, 9× `transition-colors`, 1× `transition-opacity`, 1× `transition-all`
  - Spatial audit: 1× `max-w-md` outlier, 6× `shadow-*` in UI primitives, ~30 fractional spacing values
  - Noise audit: 0× existing noise/grain/svg-filter; recommend `body::before` SVG data URI
