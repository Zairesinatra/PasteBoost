import { defaultSettings, demoItems, demoSnippets } from "./data";
import type { AppSettings, ClipboardItem, ItemType, Snippet } from "./types";

const ITEMS_KEY = "pasteboost.items";
const SNIPPETS_KEY = "pasteboost.snippets";
const SETTINGS_KEY = "pasteboost.settings";

const isDesktop = "__TAURI_INTERNALS__" in window;

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const api = await import("@tauri-apps/api/core");
  return api.invoke<T>(command, args);
}

export async function reportFrontendReady(): Promise<void> {
  if (isDesktop) await invoke("frontend_ready");
}

function readLocal<T>(key: string, fallback: T): T {
  const value = localStorage.getItem(key);
  if (value) return JSON.parse(value) as T;
  localStorage.setItem(key, JSON.stringify(fallback));
  return fallback;
}

function storeLocal<T>(key: string, value: T): T {
  localStorage.setItem(key, JSON.stringify(value));
  return value;
}

export function classifyText(content: string): ItemType {
  const trimmed = content.trim();
  try {
    JSON.parse(trimmed);
    return "json";
  } catch {
    if (/^https?:\/\/\S+$/i.test(trimmed)) return "url";
    if (/^[\w.+-]+@[\w.-]+\.[a-z]{2,}$/i.test(trimmed)) return "email";
    if (/(select |const |function |<\w+|import |insert |update )/i.test(trimmed)) return "code";
    return "text";
  }
}

export async function listItems(query = ""): Promise<ClipboardItem[]> {
  if (isDesktop) return invoke("list_items", { query });
  const items = readLocal(ITEMS_KEY, demoItems);
  const lowered = query.trim().toLowerCase();
  return items.filter((item) => !lowered || item.content.toLowerCase().includes(lowered));
}

export async function captureText(content: string): Promise<void> {
  const clean = content.trim();
  if (!clean) return;
  if (isDesktop) {
    await invoke("capture_text", { content: clean });
    return;
  }
  const items = readLocal(ITEMS_KEY, demoItems).filter((item) => item.content !== clean);
  items.unshift({
    id: Date.now(),
    content: clean,
    itemType: classifyText(clean),
    isFavorite: false,
    createdAt: new Date().toISOString(),
    usedCount: 0,
  });
  storeLocal(ITEMS_KEY, items);
}

export async function captureCurrentClipboard(): Promise<void> {
  if (isDesktop) {
    await invoke("capture_current_clipboard");
    return;
  }
  if (navigator.clipboard.read) {
    const clipboardItems = await navigator.clipboard.read();
    const imageType = clipboardItems[0]?.types.find((type) => type.startsWith("image/"));
    if (imageType) {
      const blob = await clipboardItems[0].getType(imageType);
      const imageData = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.addEventListener("load", () => resolve(reader.result as string));
        reader.addEventListener("error", () => reject(reader.error));
        reader.readAsDataURL(blob);
      });
      const items = readLocal(ITEMS_KEY, demoItems).filter((item) => item.imageData !== imageData);
      items.unshift({
        id: Date.now(),
        content: `[Image] ${Math.round(blob.size / 1024)} KB`,
        imageData,
        itemType: "image",
        isFavorite: false,
        createdAt: new Date().toISOString(),
        usedCount: 0,
      });
      storeLocal(ITEMS_KEY, items);
      return;
    }
  }
  const text = await navigator.clipboard.readText();
  await captureText(text);
}

export async function toggleFavorite(id: number): Promise<void> {
  if (isDesktop) {
    await invoke("toggle_favorite", { id });
    return;
  }
  const items = readLocal(ITEMS_KEY, demoItems).map((item) =>
    item.id === id ? { ...item, isFavorite: !item.isFavorite } : item,
  );
  storeLocal(ITEMS_KEY, items);
}

export async function deleteItem(id: number): Promise<void> {
  if (isDesktop) {
    await invoke("delete_item", { id });
    return;
  }
  storeLocal(
    ITEMS_KEY,
    readLocal(ITEMS_KEY, demoItems).filter((item) => item.id !== id),
  );
}

export async function deleteItems(ids: number[]): Promise<void> {
  if (isDesktop) {
    await invoke("delete_items", { ids });
    return;
  }
  const selected = new Set(ids);
  storeLocal(
    ITEMS_KEY,
    readLocal(ITEMS_KEY, demoItems).filter((item) => !selected.has(item.id)),
  );
}

export async function clearUnpinned(): Promise<void> {
  if (isDesktop) {
    await invoke("clear_unpinned");
    return;
  }
  storeLocal(
    ITEMS_KEY,
    readLocal(ITEMS_KEY, demoItems).filter((item) => item.isFavorite),
  );
}

export async function pasteText(content: string, autoPaste = false): Promise<void> {
  if (isDesktop && autoPaste) {
    await invoke("paste_text", { content });
    return;
  }
  if (isDesktop) {
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(content);
  } else {
    await navigator.clipboard.writeText(content);
  }
}

export async function pasteItem(item: ClipboardItem, autoPaste = false): Promise<void> {
  if (item.itemType !== "image" || !item.imageData) {
    await pasteText(item.content, autoPaste);
    return;
  }
  if (isDesktop) {
    await invoke("paste_image", { id: item.id, autoPaste });
    return;
  }
  const blob = await fetch(item.imageData).then((response) => response.blob());
  await navigator.clipboard.write([new globalThis.ClipboardItem({ [blob.type]: blob })]);
}

export async function listSnippets(): Promise<Snippet[]> {
  if (isDesktop) return invoke("list_snippets");
  return readLocal(SNIPPETS_KEY, demoSnippets);
}

export async function saveSnippet(snippet: Omit<Snippet, "id" | "createdAt">): Promise<void> {
  if (isDesktop) {
    await invoke("save_snippet", { ...snippet });
    return;
  }
  const snippets = readLocal(SNIPPETS_KEY, demoSnippets);
  snippets.unshift({ ...snippet, id: Date.now(), createdAt: new Date().toISOString() });
  storeLocal(SNIPPETS_KEY, snippets);
}

export async function deleteSnippet(id: number): Promise<void> {
  if (isDesktop) {
    await invoke("delete_snippet", { id });
    return;
  }
  storeLocal(
    SNIPPETS_KEY,
    readLocal(SNIPPETS_KEY, demoSnippets).filter((snippet) => snippet.id !== id),
  );
}

export async function getSettings(): Promise<AppSettings> {
  if (isDesktop) return invoke("get_settings");
  return { ...defaultSettings, ...readLocal(SETTINGS_KEY, defaultSettings) };
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  if (isDesktop) {
    await invoke("save_settings", { settings });
    return;
  }
  storeLocal(SETTINGS_KEY, settings);
}

export async function subscribeToUpdates(onUpdate: () => void): Promise<() => void> {
  if (!isDesktop) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen("clipboard-updated", onUpdate);
}

export async function subscribeToSettings(onUpdate: (settings: AppSettings) => void): Promise<() => void> {
  if (!isDesktop) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen<AppSettings>("settings-updated", (event) => onUpdate(event.payload));
}

export async function subscribeToPanelOpen(onOpen: () => void): Promise<() => void> {
  if (!isDesktop) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  return listen("panel-opened", onOpen);
}
