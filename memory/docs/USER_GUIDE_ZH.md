# Cairn 使用手册

> Cairn 是一个 local-first 的个人记忆 / 上下文 OS。它把你随手写下的笔记结构化成可查询的实体，并以多种协议暴露给所有 AI agent（Claude、ChatGPT、Cursor、Gemini、豆包、Kimi、DeepSeek、通义、文心、Manus、Genspark 等）。
>
> 数据全部存在本机：`~/Library/Application Support/Cairn/memory.db`（macOS）。
>
> 最后更新：2026-06（对应 v0.1.0-alpha.3 之后的 main）。

---

## 目录

1. [系统要求](#1-系统要求)
2. [安装与启动](#2-安装与启动)
3. [初次配置](#3-初次配置)
4. [日常使用](#4-日常使用)
5. [接入各家 Agent](#5-接入各家-agent)
6. [浏览器扩展](#6-浏览器扩展)
7. [文件位置](#7-文件位置)
8. [常见问题排查](#8-常见问题排查)
9. [进阶：单写入者架构](#9-进阶统一通过-sse-bridge-单写入者)

---

## 1. 系统要求

| | |
|---|---|
| 操作系统 | macOS 13+（Apple Silicon 已验证）；Windows / Linux 有安装包但测试较少 |
| AI Provider | 任选一家：Vercel AI Gateway / OpenRouter / OpenAI / Anthropic / Gemini / **豆包 / Kimi / 智谱 GLM / 通义 / Minimax**（国内 provider 无需科学上网） |
| 可选 | Chrome / Edge / Brave（装扩展）、`cloudflared`（给 ChatGPT 网页版做公网 tunnel） |
| 仅源码构建需要 | Node 18+、pnpm、Rust stable、Xcode Command Line Tools |

API key **不再要求环境变量**——首次启动后在 **Settings → AI Providers** 里配置即可（见 §3.0）。`memory/.env` 仅在源码开发时作为兜底。

---

## 2. 安装与启动

### 方式 A：下载安装包（推荐）

从 [GitHub Releases](https://github.com/mutouwilson/cairn/releases) 下载：

| 平台 | 文件 |
|---|---|
| macOS (Apple Silicon) | `Cairn_x.y.z_aarch64.dmg` |
| Windows x64 | `Cairn_x.y.z_x64_en-US.msi` |
| Linux x64 | `Cairn_x.y.z_amd64.AppImage` |
| 浏览器扩展 | `cairn-chrome-extension-*.zip` |

macOS 安装包**未做 Apple 公证**，首次打开如果提示"已损坏/无法验证开发者"：

```bash
xattr -dr com.apple.quarantine /Applications/Cairn.app
```

或在 Finder 里右键 → 打开。

### 方式 B：源码构建（开发者）

```bash
git clone https://github.com/mutouwilson/cairn.git
cd cairn/memory
pnpm install
pnpm tauri:build          # 产物在 src-tauri/target/release/bundle/
```

### 启动后会做什么

- 打开 SQLite 数据库 + sqlite-vec 扩展（必要时先自动备份再跑迁移）
- 注册全局快捷键：`⌘⇧M` 空白快速捕获、`⌘⇧K` 剪贴板预填捕获
- 启动后台 worker：consolidation（每 15 分钟把零散 entity 归纳成主题；**每轮实时读取 Settings 里的 provider**，没配就跳过）、capture（轮询邮件/日历源）、import watcher（监听 `~/.claude` 等记忆文件变化）
- 启动本地 HTTP API（`127.0.0.1:7716`，给浏览器扩展和第三方用）
- 如果上次启用过，则自动启动：选区悬浮窗（macOS 划词）、Remote MCP bridge（`127.0.0.1:7717`）

---

## 3. 初次配置

### 3.0 配置 AI Provider（第一件事）

1. 打开 Cairn → **Settings → AI Providers**
2. **Extraction**（笔记 → 实体抽取）选一家 provider、填 API key、选模型
3. **Embedding**（向量检索）同理；不配也能用（退化为 BM25 全文检索）
4. 点 **Test** 验证连通 → **Save**

改动即时生效，不需要重启。consolidation / 抽取 / 检索全部走这里的配置。

### 3.1 切换界面语言

**Settings → Language** → `English` / `简体中文`。跟随系统语言自动检测，可手动覆盖。

### 3.2 开启选区悬浮窗（划词保存，macOS）

1. **Settings → Selection popover** → 点 toggle
2. 首次会要求 **系统设置 → 隐私与安全 → 辅助功能** 里勾选 Cairn
3. 授权后回 Settings 再点一次 toggle → 变绿
4. 任意 app 划词 → 选区下方浮出 `Save / Note / ×` 药丸；点 × 或点旁边会自动收起，不会把 Cairn 主窗口带出来

> **覆盖范围**：原生 Cocoa 应用通过 AX 直读；Chrome 系列首次划词激活 AX 后直读；Sublime / Electron 等走剪贴板回退（合成 ⌘C → 还原剪贴板）。

### 3.3 装浏览器扩展

1. 从 [Releases](https://github.com/mutouwilson/cairn/releases) 下载 `cairn-chrome-extension-*.zip`，解压到一个**长期保留**的目录（Chrome 从磁盘加载）。源码构建则 `cd memory/tools/browser-ext && pnpm install && pnpm build`，产物在 `dist/`
2. `chrome://extensions/` → 开 **开发者模式** → **加载已解压的扩展程序** → 选解压目录（或 `dist/`）
3. 点工具栏 Cairn 图标 → 底部显示 `connected` 即成功（前提：Cairn.app 在运行）
4. 如果你给 Cairn 设置了 `CAIRN_API_TOKEN`，在扩展 **选项** 页把同一个 token 填进去

### 3.4 开启 Remote MCP bridge（外部 agent 接入，可选）

**Settings → Remote MCP bridge** → toggle → 显示 `http://127.0.0.1:7717/sse`。

- 本地 stdio agent（Claude Desktop / Cursor）**不需要** bridge
- 只有 ChatGPT 网页版这类要求公网 SSE 的才需要（还要加 tunnel，见 §5.3）

---

## 4. 日常使用

### 4.1 写入方式

| 触发 | 适用场景 |
|---|---|
| 主页 Capture 输入框 | 主动写笔记，⌘+Enter 保存 |
| 全局快捷键 `⌘⇧M` | 任何 app 里弹出 quick-capture 浮窗 |
| 全局快捷键 `⌘⇧K` | 剪贴板内容预填进 quick-capture |
| 选区悬浮窗（macOS） | 任何 app 划词 → `Save` / `Note` / `×` |
| 浏览器扩展右键 | 任何网页 → **Save selection to Cairn** / **Save this page to Cairn** |
| 网页里选中文字 | 支持的 AI 站点上选中即弹出保存浮层 |
| Email 源（IMAP） | Settings 配 IMAP，邮件自动入库 |
| 日历源（ICS） | Settings 配订阅 URL，事件自动入库 |

### 4.2 检索与注入

- **Search 页**：混合检索（BM25 + 向量 RRF 融合），底部显示 diagnostics
- **扩展被动召回**：在支持的 AI 站点输入框打字，命中记忆时自动浮出 pill；强匹配自动展开拾取器 → `1/2/3` 勾选、`Enter` 注入、`Esc` 关闭（中文输入法组字期间按键不会被劫持）
- **扩展 `@cairn` 主动召回**：输入框打 `@cairn 关键词` 精确搜索后注入
- **地址栏**：输 `cairn 关键词` 直接搜（omnibox）
- **快捷键 `⌘⇧L`**：打开扩展搜索弹窗

注入会以 `<cairn-context>` 块前置到你的消息，**实体和完整笔记原文都可以勾选**。

### 4.3 导入已有记忆（Import 页）

自动扫描这些用户级记忆文件并导入：

- `~/.claude/CLAUDE.md` + `~/.claude/projects/*/memory/*.md`（Claude Code）
- `~/.cursor/skills-cursor/*/SKILL.md` + `~/.cursor/plans/*.md`（Cursor）
- `~/.codex/memories/*.md`（Codex）

文件变化会被 watcher 自动重新导入。每行的状态与操作：

| 状态 | 含义 | 操作 |
|---|---|---|
| `NEW` / `SYNCED` | 待导入 / 已同步 | 行尾「跳过」按钮 → 永久不同步；「重新同步」强制重导 |
| `SKIPPED` | 你标记过不同步 | 勾选复选框 → 恢复同步 |
| `UNLINKED` | 导入过但笔记被你删了 | 勾选(可「全选待恢复」)→ Apply 批量找回；或行内「重新同步」单个找回；也可「跳过」永久忽略 |

也支持反向导出：**Export to Claude CLAUDE.md** 把 Cairn 里的高频实体写回 `~/.claude/CLAUDE.md` 的受管块。

### 4.4 主题归纳（Themes 页）

后台每 15 分钟自动把反复出现的实体（人物、偏好域、目标）归纳成语义主题；也可点 **Run consolidation** 手动触发——运行中按钮显示已耗时，结果实时刷新。

### 4.5 重新抽取 / 删除

每条 note 悬停出现 ↻（重抽）和 🗑（删除）。重抽 = 清掉该 note 产生的实体后重跑抽取；删除 = 连同孤立实体一起清（被其他 note 支撑的保留;手动编辑过的 sticky 实体也保留）。

---

## 5. 接入各家 Agent

> **Settings → Hosted MCP Connectors** 里有 Claude Desktop / Cursor / ChatGPT 的**现成配置片段**，可直接复制，并显示各 agent 最近调用时间。

### 5.1 Claude Desktop（推荐：SSE 路径）

前置：Settings 里 Remote MCP bridge 已启用。编辑 `~/Library/Application Support/Claude/claude_desktop_config.json`：

```json
{
  "mcpServers": {
    "cairn": { "url": "http://127.0.0.1:7717/sse" }
  }
}
```

完全重启 Claude Desktop。这样 **Cairn.app 是唯一 SQLite 写者**，永无锁冲突。

> **stdio 方式（备选）**：让 Claude Desktop 自己 spawn 安装版自带的二进制：
>
> ```json
> {
>   "mcpServers": {
>     "cairn": {
>       "command": "/Applications/Cairn.app/Contents/MacOS/cairn-mcp",
>       "env": { "CAIRN_AGENT_ID": "claude-desktop" }
>     }
>   }
> }
> ```
>
> 缺点：与 Cairn.app 并发写同一 DB，偶发 SQLite BUSY（有 4 次退避兜底）。仅在不想常驻 Cairn.app 时用。

### 5.2 Cursor / Windsurf

同上，stdio 或 SSE 均可；配置文件位置不同、格式一致（Settings 里有 Cursor 片段可复制）。

### 5.3 ChatGPT 网页端（需要公网 tunnel）

```bash
cloudflared tunnel --no-autoupdate --protocol http2 --url http://localhost:7717
```

拿到 `https://<随机词>.trycloudflare.com` 后，在 ChatGPT **设置 → 应用 → 新应用**：URL 填 `https://<随机词>.trycloudflare.com/sse`，身份验证选**未授权**。

> 国内网络：`*.trycloudflare.com` 本地 curl 会被 DNS 污染，但 ChatGPT 服务器在境外能解析——只要它那边创建成功即可。不行就换 `npx localtunnel --port 7717`。

### 5.4 Gemini 网页、豆包、Kimi、DeepSeek、通义、文心、Manus、Genspark

不支持 MCP，全部走**浏览器扩展**：被动召回 pill 自动浮现，或 `@cairn` 主动注入；右键保存照常可用。

### 5.5 任意第三方 / 自建 agent

走本地 HTTP API（**端口 7716**；若设置了 `CAIRN_API_TOKEN` 需带 `Authorization: Bearer …`）：

```bash
curl http://127.0.0.1:7716/api/status
curl -X POST http://127.0.0.1:7716/api/capture \
     -H "content-type: application/json" \
     -d '{"text":"…","source":"my-agent"}'
curl "http://127.0.0.1:7716/api/search?q=投资人&limit=5"
curl "http://127.0.0.1:7716/api/recent?limit=20"
curl "http://127.0.0.1:7716/api/themes?limit=20"
```

---

## 6. 浏览器扩展

### 6.1 主要功能

| 入口 | 行为 |
|---|---|
| **被动召回** | 输入框打字 → 命中记忆自动浮 pill；强匹配自动展开拾取器：`1/2/3` 勾选、`Enter` 注入、`Esc` 关闭。中文输入法组字键不受影响 |
| 输入框打 `@cairn <query>` | 主动搜索 → 选中后以 `<cairn-context>` 块前置注入（实体 + 完整笔记原文均可选） |
| 右键 **Save selection to Cairn** | 任意网页选区直接入库 |
| 右键 **Save this page to Cairn** | 抓当前页可见文字 + URL + 标题 |
| 右键 **Search Cairn for: "…"** | 用选区文本搜索 |
| 工具栏图标 popup | 快速捕获 + 实时搜索 |
| 快捷键 `⌘⇧L` / `Ctrl⇧L` | 打开搜索弹窗 |
| 地址栏 `cairn <query>` | omnibox 直接搜索 |

### 6.2 设置（右键图标 → 选项）

- **Cairn URL** —— 默认 `http://127.0.0.1:7716`（旧版 7717 会自动迁移）
- **API token** —— Cairn 用 `CAIRN_API_TOKEN` 启动时填同一个值；默认留空
- **Passive recall** —— 被动召回开关（默认开）
- **Slash prefix** —— 默认 `@cairn`
- **Inject as prefix** —— 开：`<cairn-context>` 块注入消息前；关：只插入行内引用
- **Enabled sites** —— 按站点开关

### 6.3 默认支持的站点

`chatgpt.com` / `chat.openai.com` / `claude.ai` / `gemini.google.com` / `www.doubao.com` / `kimi.com` / `chat.deepseek.com` / `yiyan.baidu.com` / `tongyi.aliyun.com` / `mira.day` / `manus.im` / `genspark.ai`

其它站点：右键菜单照常工作，只是没有输入框注入。

### 6.4 加新站点适配器

`memory/tools/browser-ext/src/content/sites/` 新增一个 `.ts` 实现 `SiteAdapter`，在 `sites/index.ts` 注册，`manifest.config.ts` 的 `CHAT_HOSTS` 加域名，`src/lib/settings.ts` 的 `DEFAULTS.enabled_sites` 和 `src/options/options.ts` 的 `KNOWN_SITES` 各加一条，`pnpm build` → chrome://extensions/ 点 ↻。

---

## 7. 文件位置

| 路径 | 内容 |
|---|---|
| `~/Library/Application Support/Cairn/memory.db` | 主数据库（SQLite + WAL） |
| `~/Library/Application Support/Cairn/providers.json` | AI Provider 配置（含 API key，勿外传） |
| `~/Library/Application Support/Cairn/backups/` | 迁移前自动备份 |
| `~/Library/Application Support/Cairn/selection_popover.json` | 选区悬浮窗开关 |
| `~/Library/Application Support/Cairn/mcp_bridge.json` | Remote MCP bridge 开关 |
| `~/Library/Application Support/Cairn/api_endpoint.json` | 本地 API 端口/token 信息 |
| Keychain `so.cairn.app` | Ed25519 审计签名密钥 |
| `/Applications/Cairn.app/Contents/MacOS/cairn-mcp` | 安装版自带的独立 MCP 二进制 |
| `memory/tools/browser-ext/dist/` | 扩展构建产物（源码构建时） |

---

## 8. 常见问题排查

### 扩展显示 unreachable / save failed

- Cairn.app 是否在运行（扩展连的是它的 `127.0.0.1:7716`）
- `chrome://extensions/` 看 Cairn 的"服务工作进程"是否 Running，点一下唤醒
- 设置过 `CAIRN_API_TOKEN` 的话，扩展选项里的 token 是否一致
- 注意：HTTP API（7716）**不依赖** Remote MCP bridge（7717），bridge 没开不影响扩展

### Run consolidation 报错或没主题

- 报 "consolidation needs an AI provider" → 去 **Settings → AI Providers** 配置（不再需要环境变量）
- Provider 选了 Anthropic 直连 → 它没有 OpenAI 兼容 chat 端点，换 Gateway / OpenRouter 等
- 没主题产出是正常的：需要同一人物/偏好域/目标积累 ≥3 条相关实体才会形成主题

### "保存失败 / failed"

- 真 failed：`/audit` 看 `note/insert` 条目
- 抽取出错但 note 已保留：显示绿色 `saved`，audit 留 `extract/raw_fallback`
- DB locked：已有 4 次指数退避兜底；若 Claude Desktop 用 stdio 方式并发写，切到 SSE 路径（§5.1）

### 选区悬浮窗不出现

- Settings 里 toggle 是否开、辅助功能权限是否给了
- Chrome 系列第一次划词只是激活 AX，再划一次
- Sublime / Electron：需要拖拽 >4px 或双击才触发剪贴板回退

### ChatGPT 连不上 tunnel

- `cloudflared` 日志出现 `Registered tunnel connection` 才算注册成功
- `Failed to dial a quic connection` → 加 `--protocol http2`
- 本地 curl tunnel 域名失败不代表 ChatGPT 连不上（见 §5.3 国内网络说明）

### 重新初始化（清掉所有数据）

```bash
pkill -f "Cairn|cairn-mcp"
rm -rf ~/Library/Application\ Support/Cairn
security delete-generic-password -s "so.cairn.app" -a "audit-signing-key" || true
# 重新打开 Cairn.app
```

---

## 9. 进阶：统一通过 SSE bridge 单写入者

推荐架构：**Cairn.app 是唯一 SQLite 写入进程**，所有 agent 通过两个本地端口访问：

```
   ┌──────────────────┐
   │ Claude Desktop   │──┐
   │ Cursor           │──┤  MCP（SSE :7717，或 stdio spawn）
   │ Windsurf         │──┘
   ┌──────────────────┐         ┌─────────────────────────┐
   │ ChatGPT (web)    │── tunnel │ Cairn.app               │
   └──────────────────┘    │     │ ├─ MCP SSE bridge :7717 │
   ┌──────────────────┐    └────│ ├─ REST /api/*   :7716  │
   │ 浏览器扩展        │─────────│ ├─ Tauri IPC            │
   │ (12+ AI 站点)     │         │ ├─ 选区 AX / 捕获源      │
   └──────────────────┘         │ └─ Consolidation worker  │
                                │       ↓                  │
                                │   SQLite（单写者）        │
                                └─────────────────────────┘
```

所有读写串到同一个连接池，Claude 看到的记忆、ChatGPT 看到的记忆、扩展存进去的记忆是同一份，且每次访问都进 Ed25519 签名审计链（`/audit` 可验证）。

---

## 10. Slogan & 哲学

> **Mark where you've been. Read by every agent.**

- 写入：你说什么、看到什么、想到什么 → 一条接一条入库
- 结构化：LLM 后台抽取成 Person / Event / Preference / Belief / Goal / Asset / Skill / Location 八种实体 + 关系
- 主题化：consolidation worker 把多次提到的实体归纳成高层主题
- 检索：BM25 + 向量 RRF 融合 + 重要度/新近度重排
- 暴露：MCP（stdio / SSE）+ HTTP REST + 浏览器扩展，覆盖任何 agent
- 审计：每一次 agent 调用都有 Ed25519 签名链记录，verify, don't trust
- 主权：local-first、SQLite、可加密、可导出、随时拔网线就跑
