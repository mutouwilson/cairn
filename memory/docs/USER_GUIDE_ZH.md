# Cairn 使用手册

> Cairn 是一个 local-first 的个人记忆 / 上下文 OS。它把你随手写下的笔记结构化成可查询的实体，并以多种协议暴露给所有 AI agent（Claude、ChatGPT、Cursor、Gemini、豆包、Kimi、DeepSeek、通义、文心 等）。
>
> 数据全部存在本机：`~/Library/Application Support/Cairn/memory.db`（macOS）。

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

---

## 1. 系统要求

| | |
|---|---|
| 操作系统 | macOS 13+（Apple Silicon 已验证；Intel 应该可行但未测） |
| 工具链 | Node 18+、pnpm、Rust（rustup 默认 stable）、Xcode Command Line Tools |
| 可选 | `cloudflared`（公网 tunnel，给 ChatGPT 等远程 agent 用）、Chrome / Edge / Brave（装扩展） |
| 网络 | 需要能访问 Vercel AI Gateway（默认走 `https://gateway.ai.vercel.ai`）；国内需要科学上网 |

环境变量你必须准备一个：

```bash
AI_GATEWAY_API_KEY="vck_…"   # 从 https://vercel.com/dashboard 拿
```

放进 `memory/.env`（已经被 `.gitignore`）。

---

## 2. 安装与启动

### 第一次拉代码后

```bash
cd memory

# 1) 前端依赖
pnpm install

# 2) Rust 一次性编译（debug 版即可，会被打包进 Cairn.app）
pnpm tauri build --debug
```

成功后会生成：
- `src-tauri/target/debug/bundle/macos/Cairn.app` —— GUI 主程序
- `src-tauri/target/debug/cairn-mcp` —— 独立 MCP 服务器

### 启动 Cairn 主程序（GUI）

```bash
open /Users/wangxu/Documents/dream/cairn/memory/src-tauri/target/debug/bundle/macos/Cairn.app
```

或者直接双击 Finder 里的 `Cairn.app`。

启动后会做以下事情：

- 打开 SQLite 数据库 + sqlite-vec 扩展
- 注册全局快捷键：`⌘⇧M` 空白快速捕获、`⌘⇧K` 剪贴板预填捕获
- 启动后台 worker：consolidation（每 15 分钟把零散 entity 合并成主题）、capture（轮询邮件/日历源）
- 如果上次启用过，则**自动启动**：
  - 选区悬浮窗（macOS 划词触发）
  - Remote MCP bridge（监听 `127.0.0.1:7717`）

### 重新构建（修改代码后）

```bash
# 改了 Rust 代码 / TS UI 代码
touch src-tauri/target/debug/migrate   # 绕过一个 tauri bundler bug
pnpm tauri build --debug

# 改了独立 MCP（Claude Desktop / Cursor 用的那个）
cargo build --release --bin cairn-mcp
```

---

## 3. 初次配置

### 3.1 开启选区悬浮窗（划词保存）

1. 启动 Cairn.app → 顶栏点 **Settings**
2. 找到 **Selection popover** 区块 → 点右上角的 `disabled`
3. macOS 第一次会弹"Cairn 想使用辅助功能"对话框（如果没弹自动打开系统设置）→ 在 **系统设置 → 隐私与安全 → 辅助功能** 里打开 Cairn 开关
4. 回 `/settings` 再点一次 toggle → 状态条变绿、底部出现"Accessibility granted"
5. 在任意 app 划词试试：3 个动作的小药丸应该浮在选区下方

> **覆盖范围**：原生 Cocoa 应用（TextEdit、Safari、Mail、Notes、Terminal、DingTalk、Sublime、IDE 等）通过 AX 直读；Chrome 系列首次激活 AX 后也能直读；Sublime / Electron 等用剪贴板回退（合成 ⌘C 复制 → 还原原剪贴板）。

### 3.2 开启 Remote MCP bridge（让外部 agent 接入）

1. `/settings` → **Remote MCP bridge** 区块 → 点 `disabled`
2. 状态条变绿 + 显示本地 URL `http://127.0.0.1:7717/sse`
3. 点 **copy** 按钮复制

这只是开了**本地端口**。外部 agent（ChatGPT、Gemini Web 等）要访问还得过一层公网 tunnel，见 [第 5 节](#5-接入各家-agent)。

Claude Desktop / Cursor / Windsurf 这种走 **stdio** 的本地 agent **不需要 bridge**，直接 spawn 独立 `cairn-mcp` 二进制即可。

### 3.3 装浏览器扩展（覆盖所有 web AI）

```bash
cd tools/browser-ext
pnpm install
pnpm build
```

在 Chrome / Edge / Brave 里：

1. 打开 `chrome://extensions/`
2. 右上角开 **开发者模式**
3. 左上 **加载已解压的扩展程序** → 选 `tools/browser-ext/dist`
4. 工具栏出现 Cairn 图标 → 点开 → 弹窗底部应显示 `connected · v0.1.0`

---

## 4. 日常使用

### 4.1 三种基本写入方式

| 触发 | 适用场景 |
|---|---|
| 主页 Capture 输入框 | 主动写笔记，⌘+Enter 保存 |
| 全局快捷键 `⌘⇧M` | 任何 app 里弹出一个轻量 quick-capture 浮窗（手动输入） |
| 全局快捷键 `⌘⇧K` | 把当前剪贴板内容预填进 quick-capture 浮窗 |
| 选区悬浮窗（macOS） | 任何 app 划词 → 弹窗 → `Save` / `+ Note` / `×` |
| 浏览器扩展右键 | 任何网页 → 右键 → **Save selection to Cairn** / **Save this page to Cairn** |
| 浏览器扩展 AI 回复 Save 按钮 | ChatGPT / Claude.ai / Gemini 等 web chat 的每条 AI 回复下方一键存档 |
| Email 源（IMAP 轮询） | `/settings` 配 IMAP，邮件自动入库 |
| 日历源（ICS 轮询） | `/settings` 配日历订阅 URL，事件自动入库 |

### 4.2 检索

- **`/search` 页**：混合检索（BM25 + 向量 RRF 融合），底部显示 diagnostics（命中数 / 跳过原因）
- **浏览器扩展 popup**：实时搜索，输入即搜
- **浏览器扩展 `@cairn` 命令**：在 ChatGPT 输入框打 `@cairn 我的投资人 id` → 下拉显示匹配项 → 选中后以 `<cairn-context>` 块前置注入到 prompt

### 4.3 重新抽取

每条已保存 note 右上角悬停会出现 ↻ 图标（重抽）和 🗑 图标（删除）。

- **重抽**：清掉这条 note 产生的 entity / relation（被其他 note 也引用的 entity 保留），重跑 LLM 抽取。换了模型 / 觉得抽错了用这个。
- **删除**：连同孤立的 entity 一起清掉。跨 note 支撑的 entity 保留。

---

## 5. 接入各家 Agent

### 5.1 Claude Desktop（推荐：SSE 路径）

**强烈推荐**让 Claude Desktop 连进程内 SSE bridge，不再独立 spawn cairn-mcp 二进制。这样**只有 Cairn.app 一个 SQLite 写者**，永远没有锁冲突，且 Claude 看到的记忆和 GUI、扩展看到的完全一致。

前置条件：`/settings → Remote MCP bridge` 已经启用（变绿，`http://127.0.0.1:7717/sse`）。

编辑 `~/Library/Application Support/Claude/claude_desktop_config.json`：

```json
{
  "mcpServers": {
    "cairn": {
      "url": "http://127.0.0.1:7717/sse"
    }
  }
}
```

完全重启 Claude Desktop（菜单栏 → Quit → 再打开）。在对话里问"看看 cairn 里我最近的笔记"应该能调到 `search_memory` 等工具。

> **旧的 stdio 方式（不推荐）**：如果你坚持让 Claude Desktop 自己 spawn 一个 cairn-mcp 进程：
>
> ```json
> {
>   "mcpServers": {
>     "cairn": {
>       "command": "/Users/wangxu/Documents/dream/cairn/memory/src-tauri/target/release/cairn-mcp",
>       "args": [],
>       "env": {
>         "AI_GATEWAY_API_KEY": "vck_…",
>         "CAIRN_DATA_DIR": "/Users/wangxu/Library/Application Support/Cairn",
>         "CAIRN_AGENT_ID": "claude-desktop",
>         "CAIRN_LOG": "info"
>       }
>     }
>   }
> }
> ```
>
> 这样 cairn-mcp 和 Cairn.app 会并发写同一个 DB，**抽取写入会偶尔撞 SQLite BUSY_SNAPSHOT**（应用层有 4 次指数退避兜底，多数能恢复，但比 SSE 方案脆弱）。仅在你不想跑 Cairn.app 时才用。

### 5.2 Cursor / Windsurf

同 Claude Desktop —— 它们也是 stdio MCP，配置文件位置不同但格式一致。

### 5.3 ChatGPT 网页端（需要 tunnel）

ChatGPT 只接受 **公网 HTTPS** 的 SSE MCP 服务器，所以本地 Cairn 必须经过 tunnel：

```bash
# 启 tunnel（推荐 cloudflared；HTTP/2 模式避免 UDP 被防火墙拦）
cloudflared tunnel --no-autoupdate --protocol http2 --url http://localhost:7717
```

日志里会出现一行 `https://<随机词>.trycloudflare.com`。把它复制下来。

在 ChatGPT 里：**设置 → 应用 → 新应用** →
- 名称：`cairn`
- MCP 服务器 URL：`https://<随机词>.trycloudflare.com/sse`
- 身份验证：**未授权**
- 勾"我了解并希望继续" → **创建**

> **国内网络注意**：`*.trycloudflare.com` 在国内被 DNS 污染，你 curl 它会失败；但 ChatGPT 服务器在境外能正确解析，所以**问题不大**——只要 ChatGPT 那边能创建成功就行。如果 ChatGPT 也连不上，换 `localtunnel`（`npx localtunnel --port 7717`）。

### 5.4 Gemini Code Assist / VS Code Copilot

支持 MCP，但 SSE 配置和 ChatGPT 类似（远程 URL + 未授权）。

### 5.5 Gemini 网页、豆包、Kimi、DeepSeek、通义、文心

**不支持 MCP**，全部走浏览器扩展。装好扩展后，输入框打 `@cairn` 注入，AI 回复下方 `💾 Save` 一键存档。

### 5.6 任意第三方 / 自建 agent

走本地 HTTP API：

```bash
curl http://127.0.0.1:7717/api/status
curl -X POST http://127.0.0.1:7717/api/capture \
     -H "content-type: application/json" \
     -d '{"text":"…","source":"my-agent","metadata":{"url":"…"}}'
curl "http://127.0.0.1:7717/api/search?q=投资人&limit=5"
curl "http://127.0.0.1:7717/api/recent?limit=20"
curl "http://127.0.0.1:7717/api/themes?limit=20"
```

---

## 6. 浏览器扩展

### 6.1 主要功能

| 入口 | 行为 |
|---|---|
| 输入框打 `@cairn <query>` | 弹出搜索拾取器；选中后以 `<cairn-context>` 块前置注入 |
| 每条 AI 回复下方 `💾 Save` | 一键存档，自动带上 agent / conversation_id / url / title 元数据 |
| 右键菜单 **Save selection to Cairn** | 任意网页选区 → 直接存 |
| 右键菜单 **Save this page to Cairn** | 抓当前页前 ~2KB 可见文字 + URL + 标题 |
| 右键菜单 **Search Cairn for: "…"** | 用选区文本搜索，结果在通知里 |
| 工具栏图标 popup | 快速捕获 + 实时搜索 |
| 快捷键 `⌘⇧L` | 打开侧栏 |

### 6.2 设置

右键 Cairn 图标 → **选项**：

- **Cairn URL** —— 默认 `http://127.0.0.1:7717`
- **Slash prefix** —— 默认 `@cairn`
- **Inject as prefix** —— 开：以 `<cairn-context>` 块注入到消息**前**；关：把匹配项的 summary 附在你消息**后**
- **Enabled sites** —— 单独开关每个站点

### 6.3 默认支持的站点

`chatgpt.com` / `chat.openai.com` / `claude.ai` / `gemini.google.com` / `www.doubao.com` / `kimi.com` / `chat.deepseek.com` / `yiyan.baidu.com` / `tongyi.aliyun.com`

其它站点：右键菜单照常工作，只是没有输入框注入和 AI 回复 Save 按钮。

### 6.4 加新站点适配器

在 `tools/browser-ext/src/content/sites/` 新增一个 `.ts`，实现 `SiteAdapter` 接口，注册到 `sites/index.ts`，在 `manifest.config.ts` 的 `host_permissions` 加站点 URL，`pnpm build` → 在 chrome://extensions/ 点 ↻ reload。

---

## 7. 文件位置

| 路径 | 内容 |
|---|---|
| `~/Library/Application Support/Cairn/memory.db` | 主数据库（SQLite + WAL） |
| `~/Library/Application Support/Cairn/selection_popover.json` | 选区悬浮窗开关持久化 |
| `~/Library/Application Support/Cairn/mcp_bridge.json` | Remote MCP bridge 开关持久化 |
| `~/Library/Application Support/Cairn/adapters/` | 个性化 embedding 适配器（Phase 4d） |
| `~/Library/Keychains/login.keychain-db` 里的 `so.cairn.app` | Ed25519 审计签名密钥（macOS Keychain） |
| `memory/.env` | `AI_GATEWAY_API_KEY=…`（不要 commit） |
| `memory/src-tauri/target/debug/bundle/macos/Cairn.app` | 调试版 GUI |
| `memory/src-tauri/target/release/cairn-mcp` | release 版独立 MCP 二进制 |
| `memory/tools/browser-ext/dist/` | 浏览器扩展构建产物 |

---

## 8. 常见问题排查

### "保存失败 / failed"

- **真 failed**：`/audit` 看 `note/insert` 那条，DB 出错（少见）
- **抽取出错但 note 已保留**：现在新版会显示绿色 `saved`，audit log 留 `extract/raw_fallback`。如果还显示 `failed` 说明是旧版本，重新构建一次
- **DB locked**：`Db::save_extracted` 已经做了 4 次指数退避（60→120→240→480ms）兜底 `SQLITE_BUSY_SNAPSHOT (code 517)`。如果还是失败，最大原因是 Cairn.app 和独立 `cairn-mcp` 同时写。**最干净方案**是把 Claude Desktop 也切到 SSE 路径（见 §5.1）

### 重抽不生效 / 点 ↻ 没反应

- 检查 `tail -F /tmp/cairn-app.log | grep reprocess` 看是否有 `reprocess: cleared previous extraction` 出现，没有的话 IPC 没到。可能是浏览器扩展或前端连不上 Cairn.app（同 `unreachable` 检查）
- 有 cleared 但没有后续 `re-extracting`：抽取异步任务没 spawn，看是否有 panic（少见）
- 有 cleared + re-extracting 但 entity 没出来：DB 写锁，看重试日志 `transient SQLite lock; retrying`，4 次都失败的话切到 SSE 单写者方案

### 选区悬浮窗不出现

```bash
tail -F /tmp/cairn-app.log | grep -E "selection|AX"
```

- 没有 `selection detector started` → `/settings` 里 toggle 没开
- 有 `started` 但划词没反应 → 看是不是 Chrome 系列，第一次划词只是激活 AX，再划一次
- Sublime / Electron 应用 → 必须保留 drag distance > 4px 或双击，否则不会触发剪贴板回退（避免误触）

### ChatGPT 连不上 tunnel

- 看 `cloudflared` 日志：`Registered tunnel connection` 表示注册成功
- 出现 `Failed to dial a quic connection error` → 用 `--protocol http2`（UDP 被防火墙拦）
- 出现 `Unauthorized: Tunnel not found` → 旧 tunnel 过期，重启 cloudflared 拿新 URL
- 本地 curl `*.trycloudflare.com` 失败 → 你被 GFW DNS 污染，但 ChatGPT 服务端不在国内，**问题在它端能不能解析**，不在你能不能 curl

### Claude Desktop spawn `cairn-mcp` 一直 timeout

- 看是不是 release 版老旧：`cargo build --release --bin cairn-mcp` 重建
- macOS Keychain 弹框你没点 → cairn-mcp 卡在 2 秒兜底再 fallback 到 DB 存储；看 `tail -F /tmp/cairn-mcp.log`
- 在系统设置 → 隐私 → 辅助功能里再点一次 cairn-mcp

### 扩展显示 "save failed"

- `chrome://extensions/` 看 Cairn 卡片里的"服务工作进程"是否 Running
- 点 Cairn 图标看 popup 底部 `connected · v0.1.0`；如果是 `unreachable` 说明 GUI 没开或 bridge 没启
- 检查 Cairn `/settings` → Remote MCP bridge 是 enabled

### 重新初始化（清掉所有数据）

```bash
# 1) 退出 Cairn.app + Claude Desktop
pkill -f "Cairn.app|cairn-mcp"

# 2) 清掉数据目录
rm -rf ~/Library/Application\ Support/Cairn

# 3) 清掉 Keychain 里的 cairn 签名密钥（可选）
security delete-generic-password -s "so.cairn.app" -a "audit-signing-key" || true

# 4) 重新启动 Cairn.app
```

---

## 9. 进阶：统一通过 SSE bridge 单写入者

推荐的最终架构是 **Cairn.app 是唯一的 SQLite 写入进程**，所有 agent（不管本地的 Claude Desktop / Cursor / Windsurf，还是远程的 ChatGPT / Gemini / 浏览器扩展）全部通过 SSE bridge（`http://127.0.0.1:7717/sse`）和本地 HTTP API（`/api/*`）访问：

```
   ┌──────────────────┐
   │ Claude Desktop   │──┐
   │ Cursor           │──┤   stdio MCP → 改成 SSE URL
   │ Windsurf         │──┤
   └──────────────────┘  │
                          │
   ┌──────────────────┐  │      ┌────────────────────┐
   │ ChatGPT (web)    │──┼──────│ Cairn.app          │
   │ Gemini Code …    │──┤      │ ├─ SSE bridge      │
   └──────────────────┘  │      │ ├─ REST /api/*     │
                          │      │ ├─ Tauri IPC      │
   ┌──────────────────┐  │      │ ├─ Selection AX   │
   │ 浏览器扩展        │──┘      │ ├─ Capture sources │
   │ (Doubao / Kimi…) │         │ └─ Consolidation  │
   └──────────────────┘         │     ↓              │
                                │   SQLite (单写者)   │
                                └────────────────────┘
```

唯一的 SQLite 写入者 = Cairn.app 的内嵌 axum。所有读写都串到同一个连接池上，**永远不会锁冲突**。Claude Desktop 看到的记忆、ChatGPT 看到的记忆、浏览器扩展存进去的记忆，都是同一份。

如果实在不想常驻 Cairn.app，可以临时用独立的 `cairn-mcp --transport sse --port 7717`，HTTP/SSE 接口一样。但抽取不会进行（独立 mcp 没有 extractor）—— 它只是只读 server。

---

## 10. Slogan & 哲学

> **Mark where you've been. Read by every agent.**

- 写入：你说什么、看到什么、想到什么 → 一条接一条入库
- 结构化：LLM 在后台抽取成 Person / Event / Preference / Belief / Goal / Asset / Skill / Location 八种实体 + 关系
- 主题化：consolidation worker 把多次提到的 entity 合并成高层 theme
- 检索：BM25 + 向量 RRF 融合 + 个性化重排
- 暴露：MCP（stdio / SSE）+ HTTP REST + 浏览器扩展，覆盖任何 agent
- 审计：每一次 agent 调用都有 Ed25519 签名链记录，`/audit` 可查
- 主权：local-first、SQLite、可加密、可导出（签名 bundle）、随时拔网线就跑
