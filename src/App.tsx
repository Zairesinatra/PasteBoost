import { useEffect, useMemo, useState } from 'react';
import {
  Braces,
  Check,
  Clipboard,
  Copy,
  Eraser,
  FileText,
  History,
  ImageIcon,
  Keyboard,
  Pin,
  PinOff,
  Plus,
  Scissors,
  Search,
  Settings,
  Sparkles,
  Trash2,
  X,
} from 'lucide-react';
import {
  captureCurrentClipboard,
  captureText,
  clearUnpinned,
  deleteItem,
  deleteItems,
  deleteSnippet,
  getSettings,
  listItems,
  listSnippets,
  pasteItem,
  pasteText,
  reportFrontendReady,
  saveSettings,
  saveSnippet,
  subscribeToPanelOpen,
  subscribeToSettings,
  subscribeToUpdates,
  toggleFavorite,
} from './platform';
import type { AppSettings, ClipboardItem, Snippet, ViewId } from './types';

const views: Array<{ id: ViewId; title: string; icon: typeof History }> = [
  { id: 'history', title: '剪贴历史', icon: History },
  { id: 'snippets', title: '文本片段', icon: FileText },
  { id: 'formatter', title: '格式化', icon: Braces },
  { id: 'settings', title: '设置', icon: Settings },
];

const initialSettings: AppSettings = {
  listeningEnabled: true,
  autostartEnabled: false,
  protectSensitive: true,
  hotkey: 'Ctrl+Shift+V',
  maxItems: 500,
  theme: 'light',
  language: 'zh-CN',
};

function timeAgo(date: string, language: AppSettings['language']) {
  const minutes = Math.floor((Date.now() - new Date(date).getTime()) / 60000);
  if (language === 'en-US') {
    if (minutes < 1) return 'Just now';
    if (minutes < 60) return `${minutes} min ago`;
    if (minutes < 1440) return `${Math.floor(minutes / 60)} hr ago`;
    return `${Math.floor(minutes / 1440)} d ago`;
  }
  if (minutes < 1) return '刚刚';
  if (minutes < 60) return `${minutes} 分钟前`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)} 小时前`;
  return `${Math.floor(minutes / 1440)} 天前`;
}

function App() {
  const [view, setView] = useState<ViewId>('history');
  const [items, setItems] = useState<ClipboardItem[]>([]);
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [settings, setSettings] = useState(initialSettings);
  const [search, setSearch] = useState('');
  const [selected, setSelected] = useState<number[]>([]);
  const [separator, setSeparator] = useState('\n');
  const [toast, setToast] = useState('');
  const [captureDraft, setCaptureDraft] = useState('');
  const zh = settings.language === 'zh-CN';

  const refreshItems = async () => {
    try {
      setItems(await listItems(search));
    } catch {
      notify('无法读取剪贴历史');
    }
  };
  const refreshSnippets = async () => {
    try {
      setSnippets(await listSnippets());
    } catch {
      notify('无法读取文本片段');
    }
  };

  useEffect(() => {
    void reportFrontendReady();
    void getSettings()
      .then(setSettings)
      .catch(() => notify('无法读取设置，已使用默认配置'));
    void refreshSnippets();
  }, []);

  useEffect(() => {
    void refreshItems();
  }, [search]);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    void subscribeToUpdates(refreshItems).then((fn) => {
      dispose = fn;
    });
    return () => dispose?.();
  }, [search]);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    void subscribeToSettings(setSettings).then((fn) => {
      dispose = fn;
    });
    return () => dispose?.();
  }, []);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    void subscribeToPanelOpen(() => {
      setView('history');
      void refreshItems();
    }).then((fn) => {
      dispose = fn;
    });
    return () => dispose?.();
  }, [search]);

  useEffect(() => {
    document.documentElement.dataset.theme = settings.theme;
    document.documentElement.lang = settings.language;
  }, [settings.theme, settings.language]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(''), 2100);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const selectedItems = useMemo(
    () => selected.map((id) => items.find((item) => item.id === id)).filter(Boolean) as ClipboardItem[],
    [items, selected],
  );
  const orderedItems = useMemo(() => [...items].sort((a, b) => Number(b.isFavorite) - Number(a.isFavorite)), [items]);

  const notify = (message: string) => setToast(message);

  async function handlePaste(item: ClipboardItem) {
    await pasteItem(item, true);
    notify('已写入剪贴板并准备粘贴');
  }

  async function handleCopy(item: ClipboardItem) {
    await pasteItem(item);
    notify('已复制');
  }

  async function handleCapture() {
    try {
      if (captureDraft.trim()) {
        await captureText(captureDraft);
        setCaptureDraft('');
      } else {
        await captureCurrentClipboard();
      }
      await refreshItems();
      notify('已添加到历史记录');
    } catch {
      notify('请允许剪贴板访问，或手动输入文本');
    }
  }

  async function handleMerge(copyOnly: boolean) {
    if (selectedItems.some((item) => item.itemType === 'image')) {
      notify(zh ? '图片不可参与合并，请仅选择文本记录' : 'Images cannot be merged with text');
      return;
    }
    const text = selectedItems.map((item) => item.content).join(separator);
    await pasteText(text, !copyOnly);
    notify(copyOnly ? '合并内容已复制' : '合并内容已准备粘贴');
    setSelected([]);
  }

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <div className="brand-mark">
            <Clipboard size={21} />
          </div>
          <div>
            <strong>PasteBoost</strong>
            <span>{zh ? '轻量剪贴助手' : 'Clipboard assistant'}</span>
          </div>
        </div>
        <nav className="nav">
          {views.map(({ id, title, icon: Icon }) => (
            <button className={view === id ? 'nav-item active' : 'nav-item'} onClick={() => setView(id)} key={id}>
              <Icon size={18} />
              {zh
                ? title
                : { history: 'History', snippets: 'Snippets', formatter: 'Formatter', settings: 'Settings' }[id]}
            </button>
          ))}
        </nav>
        <section className="status-card">
          <div className={settings.listeningEnabled ? 'status-dot online' : 'status-dot'} />
          <div>
            <strong>{settings.listeningEnabled ? (zh ? '监听中' : 'Listening') : zh ? '监听已暂停' : 'Paused'}</strong>
            <span>
              {settings.hotkey} {zh ? '打开历史面板' : 'opens history'}
            </span>
          </div>
        </section>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">PASTEBOOST</p>
            <h1>
              {zh
                ? views.find((item) => item.id === view)?.title
                : { history: 'History', snippets: 'Snippets', formatter: 'Formatter', settings: 'Settings' }[view]}
            </h1>
          </div>
          {view === 'history' && (
            <label className="search-box">
              <Search size={17} />
              <input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={zh ? '搜索复制过的内容...' : 'Search clipboard history...'}
              />
            </label>
          )}
        </header>

        {view === 'history' && (
          <HistoryView
            items={orderedItems}
            selected={selected}
            separator={separator}
            captureDraft={captureDraft}
            setCaptureDraft={setCaptureDraft}
            setSeparator={setSeparator}
            onCapture={handleCapture}
            onToggleSelect={(id) =>
              setSelected((current) =>
                current.includes(id) ? current.filter((value) => value !== id) : [...current, id],
              )
            }
            onFavorite={async (id) => {
              await toggleFavorite(id);
              await refreshItems();
            }}
            onDelete={async (id) => {
              await deleteItem(id);
              setSelected((current) => current.filter((value) => value !== id));
              await refreshItems();
            }}
            onClear={async () => {
              await clearUnpinned();
              await refreshItems();
              notify('已清除未置顶记录');
            }}
            onCopy={handleCopy}
            onPaste={handlePaste}
            onMerge={handleMerge}
            language={settings.language}
            onDeleteSelected={async () => {
              await deleteItems(selected);
              setSelected([]);
              await refreshItems();
              notify(zh ? '已删除选中记录' : 'Selected items deleted');
            }}
          />
        )}
        {view === 'snippets' && (
          <SnippetsView
            snippets={snippets}
            onCopy={(content) => void pasteText(content).then(() => notify('已复制'))}
            language={settings.language}
            onDelete={async (id) => {
              await deleteSnippet(id);
              await refreshSnippets();
            }}
            onCreate={async (draft) => {
              await saveSnippet(draft);
              await refreshSnippets();
              notify('片段已保存');
            }}
          />
        )}
        {view === 'formatter' && (
          <FormatterView onCopy={(content) => void pasteText(content).then(() => notify('已复制'))} language={settings.language} />
        )}
        {view === 'settings' && (
          <SettingsView
            settings={settings}
            language={settings.language}
            onChange={async (next) => {
              try {
                await saveSettings(next);
                setSettings(next);
                notify(next.language === 'zh-CN' ? '设置已保存' : 'Settings saved');
              } catch {
                notify(
                  settings.language === 'zh-CN'
                    ? '保存失败，请检查快捷键或开机启动设置'
                    : 'Save failed. Check the shortcut or startup setting.',
                );
              }
            }}
          />
        )}
      </main>
      {toast && (
        <div className="toast">
          <Check size={16} />
          {toast}
        </div>
      )}
    </div>
  );
}

interface HistoryProps {
  items: ClipboardItem[];
  selected: number[];
  separator: string;
  captureDraft: string;
  setCaptureDraft: (text: string) => void;
  setSeparator: (text: string) => void;
  onCapture: () => void;
  onToggleSelect: (id: number) => void;
  onFavorite: (id: number) => void;
  onDelete: (id: number) => void;
  onClear: () => void;
  onCopy: (item: ClipboardItem) => void;
  onPaste: (item: ClipboardItem) => void;
  onMerge: (copyOnly: boolean) => void;
  onDeleteSelected: () => void;
  language: AppSettings['language'];
}

function HistoryView(props: HistoryProps) {
  const zh = props.language === 'zh-CN';
  const containsImage = props.items
    .filter((item) => props.selected.includes(item.id))
    .some((item) => item.itemType === 'image');
  return (
    <section className="history-layout">
      <div className="content-column">
        {props.selected.length > 0 && (
          <div className="merge-bar">
            <strong>{zh ? `已选 ${props.selected.length} 项` : `${props.selected.length} selected`}</strong>
            <label>
              {zh ? '分隔符' : 'Separator'}
              <select value={props.separator} onChange={(event) => props.setSeparator(event.target.value)}>
                <option value={'\n'}>{zh ? '换行' : 'New line'}</option>
                <option value=" ">{zh ? '空格' : 'Space'}</option>
                <option value=", ">{zh ? '逗号' : 'Comma'}</option>
                <option value=" | ">{zh ? '竖线' : 'Pipe'}</option>
              </select>
            </label>
            <button className="danger-button" onClick={props.onDeleteSelected}>
              <Trash2 size={15} />
              {zh ? '批量删除' : 'Delete'}
            </button>
            <button className="secondary" disabled={containsImage} onClick={() => props.onMerge(true)}>
              {zh ? '合并复制' : 'Merge copy'}
            </button>
            <button className="primary" disabled={containsImage} onClick={() => props.onMerge(false)}>
              {zh ? '合并粘贴' : 'Merge paste'}
            </button>
          </div>
        )}
        <div className="section-row">
          <span>{zh ? `${props.items.length} 条记录` : `${props.items.length} items`}</span>
          <button className="quiet" onClick={props.onClear}>
            <Eraser size={15} />
            {zh ? '清除未置顶' : 'Clear unpinned'}
          </button>
        </div>
        <div className="item-list">
          {props.items.map((item) => (
            <article className={props.selected.includes(item.id) ? 'clip-item selected' : 'clip-item'} key={item.id}>
              <button
                aria-label={zh ? '选择' : 'Select'}
                className={props.selected.includes(item.id) ? 'checkbox checked' : 'checkbox'}
                onClick={() => props.onToggleSelect(item.id)}
              >
                {props.selected.includes(item.id) && <Check size={17} strokeWidth={3} />}
              </button>
              <div className="item-body" onDoubleClick={() => props.onPaste(item)}>
                <div className="item-meta">
                  <span className={`type ${item.itemType}`}>
                    {item.itemType === 'image' && <ImageIcon size={11} />}
                    {item.itemType.toUpperCase()}
                  </span>
                  <time>{timeAgo(item.createdAt, props.language)}</time>
                  {item.usedCount > 0 && (
                    <span>{zh ? `使用 ${item.usedCount} 次` : `Used ${item.usedCount} times`}</span>
                  )}
                </div>
                {item.itemType === 'image' && item.imageData ? (
                  <img className="clip-image" src={item.imageData} alt={zh ? '剪贴板图片预览' : 'Clipboard image preview'} />
                ) : (
                  <p>{item.content}</p>
                )}
              </div>
              <div className="item-actions">
                <button title={zh ? '复制' : 'Copy'} onClick={() => props.onCopy(item)}>
                  <Copy size={16} />
                </button>
                <button title={zh ? '快速粘贴' : 'Quick paste'} onClick={() => props.onPaste(item)}>
                  <Scissors size={16} />
                </button>
                <button title={zh ? '置顶' : 'Pin'} onClick={() => props.onFavorite(item.id)}>
                  {item.isFavorite ? <PinOff size={16} /> : <Pin size={16} />}
                </button>
                <button title={zh ? '删除' : 'Delete'} onClick={() => props.onDelete(item.id)}>
                  <Trash2 size={16} />
                </button>
              </div>
            </article>
          ))}
          {props.items.length === 0 && (
            <div className="empty">{zh ? '没有匹配的剪贴记录' : 'No matching clipboard records'}</div>
          )}
        </div>
      </div>
      <aside className="capture-panel">
        <div className="panel-title">
          <Plus size={16} />
          {zh ? '添加内容' : 'Add content'}
        </div>
        <textarea
          placeholder={
            zh
              ? '粘贴或输入文本，也可直接捕获系统剪贴板中的文本或图片...'
              : 'Paste or type text, or capture text or an image from the clipboard...'
          }
          value={props.captureDraft}
          onChange={(event) => props.setCaptureDraft(event.target.value)}
        />
        <button className="primary wide" onClick={props.onCapture}>
          {props.captureDraft.trim()
            ? zh
              ? '存入历史'
              : 'Save to history'
            : zh
              ? '捕获当前剪贴板'
              : 'Capture clipboard'}
        </button>
        <div className="tip">
          <Keyboard size={15} />
          {zh ? '双击记录即可快速粘贴' : 'Double-click an item to paste'}
        </div>
      </aside>
    </section>
  );
}

function SnippetsView({
  snippets,
  onCopy,
  onDelete,
  onCreate,
  language,
}: {
  snippets: Snippet[];
  onCopy: (text: string) => void;
  onDelete: (id: number) => void;
  onCreate: (draft: Omit<Snippet, 'id' | 'createdAt'>) => void;
  language: AppSettings['language'];
}) {
  const zh = language === 'zh-CN';
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState({ title: '', category: '常用', content: '' });
  return (
    <section className="page-card">
      <div className="page-actions">
        <p>
          {zh
            ? '保存常用话术、代码和模板，一次点击即可复制。'
            : 'Save phrases, code and templates for one-click copying.'}
        </p>
        <button className="primary" onClick={() => setEditing(true)}>
          <Plus size={16} />
          {zh ? '新建片段' : 'New snippet'}
        </button>
      </div>
      {editing && (
        <form
          className="snippet-editor"
          onSubmit={(event) => {
            event.preventDefault();
            if (!draft.title.trim() || !draft.content.trim()) return;
            onCreate(draft);
            setDraft({ title: '', category: '常用', content: '' });
            setEditing(false);
          }}
        >
          <input
            placeholder={zh ? '标题' : 'Title'}
            value={draft.title}
            onChange={(event) => setDraft({ ...draft, title: event.target.value })}
          />
          <input
            placeholder={zh ? '分类' : 'Category'}
            value={draft.category}
            onChange={(event) => setDraft({ ...draft, category: event.target.value })}
          />
          <textarea
            placeholder={zh ? '片段内容' : 'Snippet content'}
            value={draft.content}
            onChange={(event) => setDraft({ ...draft, content: event.target.value })}
          />
          <button type="button" className="quiet" onClick={() => setEditing(false)}>
            <X size={15} />
            {zh ? '取消' : 'Cancel'}
          </button>
          <button className="primary" type="submit">
            {zh ? '保存' : 'Save'}
          </button>
        </form>
      )}
      <div className="snippet-grid">
        {snippets.map((snippet) => (
          <article className="snippet" key={snippet.id}>
            <span>{snippet.category}</span>
            <h3>{snippet.title}</h3>
            <p>{snippet.content}</p>
            <div>
              <button className="secondary" onClick={() => onCopy(snippet.content)}>
                <Copy size={15} />
                {zh ? '复制' : 'Copy'}
              </button>
              <button className="icon-delete" onClick={() => onDelete(snippet.id)}>
                <Trash2 size={16} />
              </button>
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function removeTrailingCommas(text: string) {
  return text.replace(/,(\s*[}\]])/g, '$1');
}

function normalizeObjectLiteral(text: string) {
  const withoutTrailingCommas = removeTrailingCommas(text.trim());
  const quotedKeys = withoutTrailingCommas.replace(/([{,]\s*)([A-Za-z_$][\w$-]*)(\s*:)/g, '$1"$2"$3');
  const normalizedQuotes = quotedKeys.replace(/'([^'\\]*(?:\\.[^'\\]*)*)'/g, (_match, value: string) =>
    JSON.stringify(value.replace(/\\'/g, "'")),
  );
  return JSON.stringify(JSON.parse(normalizedQuotes), null, 2);
}

function FormatterView({ onCopy, language }: { onCopy: (text: string) => void; language: AppSettings['language'] }) {
  const [source, setSource] = useState("{ name: 'PasteBoost', enabled: true, tags: ['desktop',], }");
  const [output, setOutput] = useState('');
  const zh = language === 'zh-CN';

  function transform(action: string) {
    try {
      switch (action) {
        case 'pretty':
          setOutput(JSON.stringify(JSON.parse(source), null, 2));
          break;
        case 'minify':
          setOutput(JSON.stringify(JSON.parse(source)));
          break;
        case 'trim':
          setOutput(
            source
              .split('\n')
              .map((line) => line.trim())
              .filter(Boolean)
              .join('\n'),
          );
          break;
        case 'commas':
          setOutput(removeTrailingCommas(source));
          break;
        case 'objectJson':
          setOutput(normalizeObjectLiteral(source));
          break;
        case 'upper':
          setOutput(source.toUpperCase());
          break;
        case 'lower':
          setOutput(source.toLowerCase());
          break;
      }
    } catch {
      setOutput(zh ? '转换失败，请检查对象或 JSON 输入内容。' : 'Conversion failed. Check the object or JSON input.');
    }
  }

  return (
    <section className="formatter">
      <div className="transform-actions">
        <button onClick={() => transform('pretty')}>
          <Sparkles size={16} />
          {zh ? 'JSON 格式化' : 'Pretty JSON'}
        </button>
        <button onClick={() => transform('minify')}>{zh ? 'JSON 压缩' : 'Minify JSON'}</button>
        <button onClick={() => transform('commas')}>{zh ? '移除多余逗号' : 'Remove extra commas'}</button>
        <button onClick={() => transform('objectJson')}>{zh ? '宽松对象转 JSON' : 'Object to JSON'}</button>
        <button onClick={() => transform('trim')}>{zh ? '去空行 / Trim' : 'Trim lines'}</button>
        <button onClick={() => transform('upper')}>{zh ? '大写' : 'Uppercase'}</button>
        <button onClick={() => transform('lower')}>{zh ? '小写' : 'Lowercase'}</button>
      </div>
      <div className="editor-grid">
        <label>
          {zh ? '输入' : 'Input'}
          <textarea
            aria-label={zh ? '格式化输入' : 'Formatter input'}
            value={source}
            onChange={(event) => setSource(event.target.value)}
          />
        </label>
        <label>
          {zh ? '结果' : 'Output'}
          <textarea
            aria-label={zh ? '格式化结果' : 'Formatter output'}
            value={output}
            readOnly
            placeholder={zh ? '选择一个操作生成结果' : 'Choose an action to generate output'}
          />
        </label>
      </div>
      <button className="primary result-copy" disabled={!output} onClick={() => onCopy(output)}>
        <Copy size={16} />
        {zh ? '复制结果' : 'Copy output'}
      </button>
    </section>
  );
}

function SettingsView({
  settings,
  onChange,
  language,
}: {
  settings: AppSettings;
  onChange: (next: AppSettings) => Promise<void>;
  language: AppSettings['language'];
}) {
  const [draft, setDraft] = useState(settings);
  const [saving, setSaving] = useState(false);
  const zh = language === 'zh-CN';

  useEffect(() => {
    setDraft(settings);
  }, [settings]);

  const toggles = [
    {
      key: 'listeningEnabled' as const,
      title: zh ? '剪贴板监听' : 'Clipboard monitoring',
      description: zh ? '自动记录新复制的文本内容' : 'Automatically record newly copied text',
    },
    {
      key: 'autostartEnabled' as const,
      title: zh ? '开机自动启动' : 'Launch on startup',
      description: zh ? '登录 Windows 后常驻托盘' : 'Stay in the tray after signing in',
    },
    {
      key: 'protectSensitive' as const,
      title: zh ? '敏感文本保护' : 'Sensitive text protection',
      description: zh ? '不保留疑似密码或验证码内容' : 'Exclude suspected passwords and codes',
    },
  ];
  const changed = JSON.stringify(draft) !== JSON.stringify(settings);
  const validHotkey =
    /^(Ctrl|Control|Alt|Shift|Super|Meta)(\+(Ctrl|Control|Alt|Shift|Super|Meta))*\+[A-Za-z0-9]$/i.test(
      draft.hotkey.trim(),
    );
  return (
    <section className="settings-card">
      {toggles.map((toggle) => (
        <div className="setting-row" key={toggle.key}>
          <div>
            <strong>{toggle.title}</strong>
            <p>{toggle.description}</p>
          </div>
          <button
            className={draft[toggle.key] ? 'switch enabled' : 'switch'}
            aria-label={toggle.title}
            aria-pressed={draft[toggle.key]}
            onClick={() => setDraft({ ...draft, [toggle.key]: !draft[toggle.key] })}
          >
            <span />
          </button>
        </div>
      ))}
      <div className="setting-row fields">
        <div>
          <strong>{zh ? '呼出快捷键' : 'Global shortcut'}</strong>
          <p>{zh ? '在任何应用中打开剪贴历史面板' : 'Open history from any application'}</p>
        </div>
        <div className="hotkey-field">
          <input
            aria-label={zh ? '呼出快捷键' : 'Global shortcut'}
            value={draft.hotkey}
            onChange={(event) => setDraft({ ...draft, hotkey: event.target.value })}
          />
          {!validHotkey && (
            <small>{zh ? '请使用如 Ctrl+Shift+V 的组合键' : 'Use a combination like Ctrl+Shift+V'}</small>
          )}
        </div>
      </div>
      <div className="setting-row fields">
        <div>
          <strong>{zh ? '最大保留条数' : 'History limit'}</strong>
          <p>{zh ? '超出后自动移除更早记录' : 'Remove older records beyond this limit'}</p>
        </div>
        <input
          aria-label={zh ? '最大保留条数' : 'History limit'}
          type="number"
          min={10}
          max={5000}
          value={draft.maxItems}
          onChange={(event) => setDraft({ ...draft, maxItems: Number(event.target.value) })}
        />
      </div>
      <div className="setting-row fields">
        <div>
          <strong>{zh ? '主题' : 'Theme'}</strong>
          <p>{zh ? '选择舒适的界面明暗模式' : 'Choose a comfortable color mode'}</p>
        </div>
        <select
          aria-label={zh ? '主题' : 'Theme'}
          value={draft.theme}
          onChange={(event) => setDraft({ ...draft, theme: event.target.value as AppSettings['theme'] })}
        >
          <option value="light">{zh ? '浅色' : 'Light'}</option>
          <option value="dark">{zh ? '深色' : 'Dark'}</option>
          <option value="system">{zh ? '跟随系统' : 'System'}</option>
        </select>
      </div>
      <div className="setting-row fields">
        <div>
          <strong>{zh ? '语言' : 'Language'}</strong>
          <p>{zh ? '保存后应用界面语言' : 'Applied after saving settings'}</p>
        </div>
        <select
          aria-label={zh ? '语言' : 'Language'}
          value={draft.language}
          onChange={(event) => setDraft({ ...draft, language: event.target.value as AppSettings['language'] })}
        >
          <option value="zh-CN">简体中文</option>
          <option value="en-US">English</option>
        </select>
      </div>
      <div className="settings-actions">
        {changed && <span>{zh ? '有尚未保存的修改' : 'Unsaved changes'}</span>}
        <button className="secondary" disabled={!changed || saving} onClick={() => setDraft(settings)}>
          {zh ? '取消修改' : 'Cancel'}
        </button>
        <button
          className="primary"
          disabled={!changed || saving || !validHotkey}
          onClick={async () => {
            setSaving(true);
            try {
              await onChange({ ...draft, hotkey: draft.hotkey.trim() });
            } finally {
              setSaving(false);
            }
          }}
        >
          {saving ? (zh ? '保存中...' : 'Saving...') : zh ? '保存设置' : 'Save settings'}
        </button>
      </div>
    </section>
  );
}

export default App;
