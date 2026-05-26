import type { AppSettings, ClipboardItem, Snippet } from "./types";

const now = Date.now();

export const demoItems: ClipboardItem[] = [
  {
    id: 1,
    content: '{ "name": "PasteBoost", "version": "0.1.0", "ready": true }',
    itemType: "json",
    isFavorite: true,
    createdAt: new Date(now - 1000 * 60 * 3).toISOString(),
    usedCount: 8,
  },
  {
    id: 2,
    content: "https://v2.tauri.app/plugin/clipboard/",
    itemType: "url",
    isFavorite: false,
    createdAt: new Date(now - 1000 * 60 * 26).toISOString(),
    usedCount: 3,
  },
  {
    id: 3,
    content: "SELECT id, content FROM clipboard_items ORDER BY created_at DESC;",
    itemType: "code",
    isFavorite: false,
    createdAt: new Date(now - 1000 * 60 * 90).toISOString(),
    usedCount: 1,
  },
  {
    id: 4,
    content: "product@pasteboost.app",
    itemType: "email",
    isFavorite: false,
    createdAt: new Date(now - 1000 * 60 * 190).toISOString(),
    usedCount: 0,
  },
];

export const demoSnippets: Snippet[] = [
  {
    id: 1,
    title: "PR 回复",
    category: "工作",
    content: "已收到反馈，我会修正后重新提交 review。",
    createdAt: new Date(now - 86400000).toISOString(),
  },
  {
    id: 2,
    title: "分页 SQL",
    category: "SQL",
    content: "LIMIT {{pageSize}} OFFSET {{offset}}",
    createdAt: new Date(now - 172800000).toISOString(),
  },
  {
    id: 3,
    title: "联系邮箱",
    category: "个人",
    content: "hello@example.com",
    createdAt: new Date(now - 3600000).toISOString(),
  },
];

export const defaultSettings: AppSettings = {
  listeningEnabled: true,
  autostartEnabled: false,
  protectSensitive: true,
  hotkey: "Ctrl+Shift+V",
  maxItems: 500,
  theme: "light",
  language: "zh-CN",
};
