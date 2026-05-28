export type ItemType = "text" | "json" | "url" | "email" | "code" | "image";

export interface ClipboardItem {
  id: number;
  content: string;
  itemType: ItemType;
  imageData?: string | null;
  isFavorite: boolean;
  createdAt: string;
  usedCount: number;
}

export interface Snippet {
  id: number;
  title: string;
  content: string;
  category: string;
  createdAt: string;
}

export interface AppSettings {
  listeningEnabled: boolean;
  autostartEnabled: boolean;
  protectSensitive: boolean;
  hotkey: string;
  maxItems: number;
  theme: "light" | "dark" | "system";
  language: "zh-CN" | "en-US";
}

export type ViewId = "history" | "snippets" | "formatter" | "settings";
