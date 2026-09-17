# DESIGN.md — `example/fore` 前端方向指南

> 本文件是后续 AI 接手 `example/fore/` 前端工作时的事实之源。除非显式 override，所有 UI 改动必须遵守这里的决策。

---

## 调性 (Tone)

**粗犷 / 原始**。结构裸露、对比生硬、`220 14% 25%` 的加粗边框（源码注释称为 "brutalist structure"）作为承重元素；`#0d1117` 画布 + JetBrains Mono Variable 全场表态。不做美化、不道歉。

**承重决策**：

- 边框不止于分隔，还是结构语言 —— 外轮廓、粗接缝、终端会话框感
- 间距允许紧但拒绝空
- 装饰仅服务氛围（如颗粒层），不承担信息

---

## 颜色 (Color)

**尊重现有 token 系统**。不要新增颜色；调色板通过 `hsl(var(--token) / <alpha-value>)` 消费。

来源：`example/fore/src/index.css:13-60`

| 角色     | Token                | HSL 通道 → 实际值                            |
| -------- | -------------------- | -------------------------------------------- |
| 画布     | `--background`       | `220 22% 9%` → `#0d1117`                     |
| 正文     | `--foreground`       | `217 18% 80%` → `#c9d1d9`                    |
| 卡片     | `--card`             | `220 16% 12%` → `#161b22`                    |
| 弹层     | `--popover`          | `220 16% 12%` → `#161b22`                    |
| 弱化背景 | `--muted`            | `220 13% 18%` → `#21262d`                    |
| 弱化文字 | `--muted-foreground` | `215 11% 65%` → `#8b949e`                    |
| 主色     | `--primary`          | `212 92% 68%` → `#58a6ff`                    |
| 次色     | `--secondary`        | `220 13% 18%` → `#21262d`                    |
| 强调背景 | `--accent`           | `220 13% 18%` → `#21262d`                    |
| 成功     | `--success`          | `137 56% 50%` → `#3fb950`                    |
| 警告     | `--warning`          | `41 70% 47%` → `#d29922`                     |
| 错误     | `--destructive`      | `358 75% 60%` → `#f85149`                    |
| 边框     | `--border`           | `220 14% 25%`（加粗，"brutalist structure"） |
| 输入框边 | `--input`            | `220 14% 25%`                                |
| 焦点环   | `--ring`             | `212 92% 68%` → `#58a6ff`                    |
| 半径基   | `--radius`           | `0.5rem`                                     |

**规则**：

- 需要重新主题化就只改 `:root`。`tailwind.config.tsx:9-75` 已把 token 接入 Tailwind；改 token 时不需要同步改 config，颜色通过 CSS 变量直接生效
- 不要硬编码十六进制 —— 永远走 `hsl(var(--token))`
- 不要新增颜色 token；如果现有不够用，先评估是否真的需要

---

## 字体 (Typography)

**尊重现有单字体承诺**。JetBrains Mono Variable 一统天下（显示 + 正文 + 代码）。

来源：

- 字体导入：`example/fore/src/main.tsx:5`（`@fontsource-variable/jetbrains-mono`）
- 字体变量：`example/fore/src/index.css:58-59`（`--font-ui` / `--font-mono`）
- 字体包：`example/fore/package.json:16`（`@fontsource-variable/jetbrains-mono ^5.2.5`）

**默认阶梯**：

- 正文：13px / line-height 1.5（在 `index.css:80` 附近设置）
- 代码 / `code` / `pre` / `kbd` / `samp`：12px（`index.css:91-99`）
- 字号阶梯：通过 Tailwind `text-*` 调整；不要为单一组件引入定制 size
- 字重：通过 Tailwind `font-bold` 等标准 utility；不要新增自定义 weight

**禁止**：

- 引入展示衬线（IBM Plex Serif、Bodoni Moda、Fraunces、Newsreader 等）
- 引入无衬线配对（Inter、Geist、Space Grotesk、Satoshi 等）
- 使用 `system-ui`、`Arial`、`Helvetica`、`sans-serif` 作为显示字体
- 差异化请在 **字号、字重、ASCII 装饰** 内部玩，不要破坏单字体边界

---

## 动作 (Motion)

**大胆的入场编排** —— 一次性、定时、分阶段的页面加载入场揭示。整个页面只有一个连贯的表演；零散悬停微交互清零。

**当前基线**：`src/index.css` 内只有 `transition-opacity` 被引用，没有任何 `@keyframes` 或 `--ease-*` token。新增动作需要从这里长出来。

**入场编排约定**：

- 元素按其语义顺序进入（如：源 → 表格 → 行 → 字段）
- `animation-delay` 按 index 错开（约 40-80ms 步长），CSS-only 即可
- 单次入场曲线：`ease-out` 或自定义 cubic-bezier；不要 `bounce` 或 `elastic`
- 完成后保持稳定状态 —— 不要无限循环、闪烁、抖动
- 必须支持 `prefers-reduced-motion` —— 该媒体查询下退化为即时呈现

**反面案例**：

- 滚动入场（`IntersectionObserver` + `fade-in`）—— SaaS 默认，必须避开
- 悬停缩放、阴影浮起 —— 与粗犷调性冲突
- 加载旋转图标 —— 用入场编排取代
- 任何超过 600ms 的入场 —— 慢即拖沓

**已落地实现**（`example/fore/src/index.css` 末尾 `@layer base` 块）：

| Token                | 值                                         |
| -------------------- | ------------------------------------------ |
| `--stagger-base`     | `40ms`                                     |
| `--stagger-step`     | `70ms`                                     |
| `--stagger-duration` | `320ms`                                    |
| `--stagger-ease`     | `cubic-bezier(0.16, 1, 0.3, 1)` (expo-out) |

**契约**：

- 父级 `[data-stagger]` —— 设上述 4 个 CSS 变量，子级继承
- 子级 `[data-stagger-item]` —— 设 `--stagger-index: N`，`animation-delay = base + N * step`
- `@keyframes rise-in`：translateY(8px) + opacity 0 → translateY(0) + opacity 1

**应用范围**（`AppShell.tsx` 是唯一 orchestrator）：

| 子项    | `--stagger-index` | 启动时刻 | 完成时刻 |
| ------- | ----------------- | -------- | -------- |
| Sidebar | `1`               | T+110ms  | T+430ms  |
| Header  | `2`               | T+180ms  | T+500ms  |
| main    | `3`               | T+250ms  | T+570ms  |

总入场 ≤ 600ms 上限。

**降级路径**：`@media (prefers-reduced-motion: reduce)` 块内 `[data-stagger-item]` 设 `animation: none` + `opacity: 1` + `transform: none`，元素立即可见，无动画。

**未来扩展约定**：当某页需要内部二级编排（如 ChatComposer 单独登场），新 `[data-stagger]` 子树应继承外层 token，但用更短的 `--stagger-step`（如 50ms）和 `--stagger-duration`（如 240ms）以不破 600ms 上限。

---

## 空间 (Spatial)

**规整网格、对称** —— 可预测的对齐通过排版、色彩、动作贏得质感，而非布局戏剧。

- shadcn 原语原样使用（`Card`、`Dialog`、`Tabs`、`ScrollArea`、`Tooltip`）—— `src/components/ui/`
- 间距走 Tailwind 默认（无自定义 `--spacing-*` token）
- `borderRadius` 走 `var(--radius)` 派生（base `0.5rem`，lg/md/sm 通过 `tailwind.config.tsx:53-75` 派生）—— 不要单独给某个组件写 `rounded-md`
- 对齐默认 `items-stretch` / `justify-start`，除非内容要求居中
- 不用 CSS Grid 破对称；偶有需要时走负 margin 或 `transform: translate`

**禁止**：

- `max-w-7xl mx-auto` 套每个区块 —— 与对称承诺冲突（除非该区块本身就是内容容器）
- 元素互相重叠、负 margin 渗透 —— 留给粗犷调性做边界表达，不要在结构层面使用
- 三列 hero + 三张卡片 + 居中 CTA —— 默认 SaaS 套路
- 不规则网格（如 Pinterest 式、Bento 风格）—— 与对称承诺冲突

---

## 背景 (Backgrounds)

**尊重现有颗粒层**。4% SVG `feTurbulence` 颗粒覆盖层在 `body::before`（`src/index.css:106-115`），`mix-blend-mode: overlay`。

- 不要移除颗粒层 —— 它是氛围的一部分
- 不要叠加渐变 mesh、几何图案、SVG 装饰在内容之上
- 卡片背景允许 `var(--card)`，与画布有 `220 16% 12%` vs `220 22% 9%` 的层级差
- 不要用 `backdrop-blur` 模糊背景 —— 与粗犷调性冲突

---

## 差异化 (Differentiation)

**入场编排本身即是签名** —— 让人一周后还能回忆的工件。粗犷调性 + 全等宽 + 颗粒 + 对称网格的组合很多；唯有"页面加载即表演"的执行让这套组合不可复制。

**执行准则**：把每个新页面的入场当作一部短片编排 ——

- 第一帧谁出现
- 第二个出现的间隔
- 第三个是否重叠前一个的尾部
- 最后一个何时收尾
- 是否所有元素同向出场（从顶到底 vs 从左到右）

这一编排应当：

- 可被 `prefers-reduced-motion` 关闭
- 默认开启
- 整页统一（不要每个区块用不同曲线）

---

## 永不生成 (NEVER Generate)

- **默认字体**：Inter、Roboto、Arial、`system-ui` 作为显示字体；同时避开 Space Grotesk、Geist、Satoshi（无衬线），Fraunces、Cormorant、EB Garamond（编辑衬线）
- **烂俗配色**：白底紫蓝渐变、`#3B82F6` 一族的 "SaaS 蓝"、均匀分布的粉彩
- **可预测布局**：居中卡片堆、首屏 + 三列、流水线导航、每个区块都套 `max-w-7xl mx-auto`
- **流水线组件**：千篇一律的 `rounded-xl shadow-md` 卡片、通用幽灵按钮
- **通用动作**：每滚一次都淡入、雷同的弹跳缓动、零散无编排的微交互
- **惰性交互面**：看起来可点击的元素必须真的可点击 —— 真 `<a href>` 或 `<button>`，别用样式化的 `<div>`/`<span>`；React/Vue：真 `<Link>` 或 `@click` 处理器；视觉可供性承诺行为，要么兑现，要么去掉

---

## 当前例外与未来工作

- ~~入场编排的 `@keyframes` 尚未落地~~ —— 已落地（见"动作"章节"已落地实现"段）
- `components.json`（shadcn registry）未提交 —— 当前 11 个 UI 原语是手写的，符合 shadcn 习惯但不进 shadcn 流水线
- 没有 `.storybook/` —— 组件开发走 `src/components/ui/` 下手动验证
- 没有 `--spacing-*`、`--shadow-*` token —— Tailwind 默认够用；动作 token 已落地于 `[data-stagger]` 上下文（见"动作"章节）

---

## 引用索引

| 维度          | 主源                                                   | 备注                                                                                                                    |
| ------------- | ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- |
| 颜色 token    | `example/fore/src/index.css:13-60`                     | `:root { --background, --primary, --border, ... }`                                                                      |
| 边框说明      | `example/fore/src/index.css:48`                        | 注释：`(was 18%, now 25%)` —— brutalist structure                                                                       |
| 字体变量      | `example/fore/src/index.css:58-59`                     | `--font-ui`, `--font-mono`                                                                                              |
| 字体导入      | `example/fore/src/main.tsx:5`                          | `@fontsource-variable/jetbrains-mono`                                                                                   |
| 颗粒层        | `example/fore/src/index.css:106-115`                   | `body::before` + `feTurbulence`                                                                                         |
| Tailwind 接入 | `example/fore/tailwind.config.tsx:9-75`                | 颜色映射 HSL 通道                                                                                                       |
| 半径派生      | `example/fore/tailwind.config.tsx:53-75`               | `lg/md/sm` 从 `--radius` 派生                                                                                           |
| 字体包        | `example/fore/package.json:16`                         | `@fontsource-variable/jetbrains-mono ^5.2.5`                                                                            |
| UI 原语       | `example/fore/src/components/ui/`                      | 11 个手写 shadcn 风格组件                                                                                               |
| `cn()` 工具   | `example/fore/src/lib/utils.tsx:11-13`                 | `twMerge(clsx(inputs))`                                                                                                 |
| 特性组件      | `example/fore/src/components/{layout/}` + 6 个 feature | `AppShell`、`Header`、`Sidebar`、`ChatComposer`、`JsonEditor`、`MemoryPanel`、`StatusPill`、`StepRow`、`StreamEventRow` |
