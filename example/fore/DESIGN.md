# DESIGN.md — `example/fore` 前端方向指南

> 本文件是后续 AI 接手 `example/fore/` 前端工作时的事实之源。除非显式 override，所有 UI 改动必须遵守这里的决策。

## 调性 (Tone)

**现代极简**。外部用户操作的 agent 控制面板 —— 接近 Vercel / Linear / Anthropic Console 的视觉密度。Inter sans body + JetBrains Mono code/ID + soft 1px borders + 克制暗色 token + shadcn 默认密度。

- 结构优先于装饰：border 是分组语言，不是承重元素
- 间距紧凑但不拥挤
- 装饰只服务功能性 affordance，不做氛围

## 颜色 (Color)

**HSL 通道 token 系统**。颜色通过 `:root { --token }` 暴露为 HSL 通道三元组（`<channel> <channel> <channel>`），Tailwind 通过 `<alpha-value>` 在 `bg-primary/50` 等位置消费 alpha。

来源：`example/fore/src/index.css:11-58`

| 角色     | Token                | HSL 通道 → 实际值                 |
| -------- | -------------------- | --------------------------------- |
| 画布     | `--background`       | `220 14% 9%` → `#0d1117`          |
| 正文     | `--foreground`       | `220 13% 92%`                     |
| 卡片     | `--card`             | `220 13% 11%` → `#161b22`         |
| 弹层     | `--popover`          | `220 13% 11%`                     |
| 升起     | `--surface-2`        | `220 12% 14%` → raised hover rows |
| 弱化背景 | `--muted`            | `220 12% 14%`                     |
| 弱化文字 | `--muted-foreground` | `215 10% 60%`                     |
| 主色     | `--primary`          | `220 90% 64%` → soft sky          |
| 次色     | `--secondary`        | `220 12% 14%`                     |
| 强调背景 | `--accent`           | `220 12% 14%`                     |
| 成功     | `--success`          | `150 50% 50%`                     |
| 警告     | `--warning`          | `38 85% 58%`                      |
| 错误     | `--destructive`      | `0 72% 58%`                       |
| 边框     | `--border`           | `220 13% 19%` — 1px soft          |
| 输入框边 | `--input`            | `220 13% 19%`                     |
| 焦点环   | `--ring`             | `220 90% 64% / 0.5`               |
| 半径基   | `--radius`           | `0.5rem`                          |

## 字体 (Typography)

**Sans body + mono code 双字体策略**。Inter 走主体文字；JetBrains Mono Variable 走代码块、ID、时间戳、JSON。

来源：

- Inter 导入：`example/fore/src/main.tsx:5`
- JetBrains Mono 导入：`example/fore/src/main.tsx:6`
- 字体变量：`example/fore/src/index.css:55-58`

**默认阶梯**：

- 正文：14px / line-height 1.5（`index.css:80` 附近）
- 代码 / `code` / `pre` / `kbd` / `samp`：13px
- 字号阶梯：通过 Tailwind `text-sm` / `text-xs` / `text-base` 等标准 utility；不要为单一组件引入定制 size
- 字重：通过 Tailwind `font-medium` / `font-semibold`；不要新增自定义 weight

**禁止**：

- 引入展示衬线
- 引入无衬线配对（保持 Inter 一家）
- 使用 `system-ui`、`Arial`、`Helvetica`、`sans-serif` 作为显示字体
- mono 用作正文（除代码、ID、时间戳、JSON 外的位置禁止 `font-mono`）

## 动作 (Motion)

**轻量编排入场** —— 现代极简保留一条统一的入场曲线（不是 brutalist 那种粗编排），但更紧凑、更克制。

**当前基线**：`src/index.css` 内 `@keyframes rise-in` 与 `[data-stagger]` token。已落地。

| Token                | 值                                         |
| -------------------- | ------------------------------------------ |
| `--stagger-base`     | `60ms`                                     |
| `--stagger-step`     | `50ms`                                     |
| `--stagger-duration` | `280ms`                                    |
| `--stagger-ease`     | `cubic-bezier(0.16, 1, 0.3, 1)` (expo-out) |

**应用范围**（`AppShell.tsx` 是唯一 orchestrator）：

| 子项    | `--stagger-index` | 启动时刻 | 完成时刻 |
| ------- | ----------------- | -------- | -------- |
| Sidebar | `1`               | T+110ms  | T+390ms  |
| Header  | `2`               | T+160ms  | T+440ms  |
| main    | `3`               | T+210ms  | T+490ms  |

总入场 ≤ 500ms。

**降级路径**：`prefers-reduced-motion` 媒体查询关闭动画。

## 空间 (Spatial)

**shadcn 默认密度** —— 标准控制面板的紧凑度。

- Button：h-9（36px）default / h-8（32px）sm / h-7（28px）xs
- Input：h-9 default / h-8 sm
- Card padding：p-6（24px）default / p-4（16px）sm
- 间距走 Tailwind 默认（4px 单位）：`space-1 = 4`、`space-2 = 8`、`space-4 = 16`、`space-6 = 24`
- Sidebar 宽度：240px（shadcn 默认）/ 256（Linear）— 选 240
- Page max-width：1280px（content）/ 1440px（outer）— 用 Tailwind `max-w-screen-xl` 或自定义
- `borderRadius` 走 `var(--radius)` 派生（base `0.5rem`，lg/md/sm 派生）—— 不要单独给某个组件写 `rounded-md`

**禁止**：

- 边框超过 1px（除了聚焦环）
- 阴影超过 `shadow-sm`（现代极简基本不用阴影）
- 元素互相重叠、负 margin 渗透

## 背景 (Backgrounds)

**纯净画布**。不加颗粒层、不加渐变 mesh、不加几何装饰。`--background` 即唯一底色。

- 不要叠加 `body::before` 装饰
- 不要用 `backdrop-blur`
- 卡片背景允许 `var(--card)`，与画布有 `220 13% 11%` vs `220 14% 9%` 的层级差

## 组件原语

**shadcn-ui CLI 维护**。`src/components/ui/` 下 11 个原语通过 `npx shadcn-ui add` 重新生成；`components.json` 锁定路径前缀和 style。

新增原语时优先走 `npx shadcn-ui add <name>` 而非手写。

## 永不生成 (NEVER Generate)

- **装饰性 mono 正文**：除代码、ID、JSON、时间戳外的位置禁用 `font-mono`
- **UPPERCASE tracking-widest 标签**：现代极简 label 是普通字号 + font-medium，不用全大写 + tracking-widest 做装饰
- **bracket / ASCII 装饰**：`[IDLE]`、`× error`、`≡ goal`、`■ final` 等 ASCII art 不出现
- **paper grain 颗粒层**：不要 `body::before` SVG `feTurbulence` 装饰
- **border-2 / 2px 边框**：现代极简只用 1px；超过 1px 的边框会让卡片喧宾夺主
- **box-shadow-lg / shadow-md**：极少用；优先用背景层级（`--card` vs `--background`）表达层级
- **sans-serif 显示字体引入**：保持 Inter 一家
- **可预测布局**：居中卡片堆、首屏 + 三列、流水线导航、每个区块都套 `max-w-7xl mx-auto`

## 当前例外与未来工作

- `/explore` 整合了过去的 `/caps`、`/caps/:name`、`/invoke`、`/playground` 路由 —— 通过 Tabs 内部导航
- `Checkpoints` 移入 `/agent/sessions/:id` 页面 —— 不再有顶级 `/checkpoints`
- `useCheckpoints` hook 在 slice 9 决定保留（Overview 卡片）还是删除
- `useStream` hook 已确认为死代码，slice 9 删除
- 后端改动（`/api/checkpoints` 移除、agent cap 形状调整）不在本 artifact 范围

## 引用索引

| 维度          | 主源                                                   |
| ------------- | ------------------------------------------------------ |
| 颜色 token    | `example/fore/src/index.css:11-58`                     |
| 字体变量      | `example/fore/src/index.css:55-58`                     |
| 字体导入      | `example/fore/src/main.tsx:5-6`                        |
| Tailwind 接入 | `example/fore/tailwind.config.tsx`                     |
| 路径别名      | `example/fore/tsconfig.json:25-26` + `components.json` |
| UI 原语       | `example/fore/src/components/ui/`                      |
| `cn()` 工具   | `example/fore/src/lib/utils.ts`                        |
| 特性组件      | `example/fore/src/components/{layout,*}.tsx`           |
