// Per-site adapter registry. We dispatch by `hostname` (plus a fallback for
// www-prefixed variants).

import type { SiteAdapter } from "../types";
import { chatgptAdapter } from "./chatgpt";
import { claudeAdapter } from "./claude";
import { geminiAdapter } from "./gemini";
import { doubaoAdapter } from "./doubao";
import { kimiAdapter } from "./kimi";
import { deepseekAdapter } from "./deepseek";
import { tongyiAdapter } from "./tongyi";
import { miraAdapter } from "./mira";
import { manusAdapter } from "./manus";
import { gensparkAdapter } from "./genspark";

const REGISTRY: Record<string, SiteAdapter> = {
  "chatgpt.com": chatgptAdapter,
  "www.chatgpt.com": chatgptAdapter,
  "chat.openai.com": chatgptAdapter,
  "claude.ai": claudeAdapter,
  "gemini.google.com": geminiAdapter,
  "www.doubao.com": doubaoAdapter,
  "kimi.com": kimiAdapter,
  "www.kimi.com": kimiAdapter,
  "chat.deepseek.com": deepseekAdapter,
  "yiyan.baidu.com": doubaoAdapter, // structurally similar enough for v1
  "tongyi.aliyun.com": tongyiAdapter,
  "mira.day": miraAdapter,
  "www.mira.day": miraAdapter,
  "manus.im": manusAdapter,
  "www.manus.im": manusAdapter,
  "genspark.ai": gensparkAdapter,
  "www.genspark.ai": gensparkAdapter,
};

export function adapterForHost(hostname: string): SiteAdapter | null {
  return REGISTRY[hostname] ?? null;
}
