# PasteBoost

面向开发者与办公场景的轻量剪贴板增强器，基于 Tauri 2、React 与 SQLite。

## 当前功能

- 文本与图片剪贴历史：自动分类、图片预览、搜索、置顶、删除与去重。
- 快速操作：复制、快速粘贴、多选后按分隔符合并粘贴。
- 文本片段：保存常用话术、代码或 SQL 模板。
- 格式化工具：JSON 格式化/压缩、去空行与大小写转换。
- 桌面能力：托盘常驻、全局快捷键、开机自启、暂停监听。
- 本地保护：SQLite 持久化与基础敏感内容过滤。

## 运行方式

安装 Node 依赖：

```powershell
pnpm install
```

仅查看界面与交互（使用浏览器本地演示数据）：

```powershell
pnpm dev
```

运行完整桌面应用需要先安装 Rust stable 工具链，然后执行：

```powershell
pnpm tauri dev
```

网页预览通过 `localStorage` 模拟历史和片段操作；Tauri 应用运行时会使用系统剪贴板与应用数据目录中的 SQLite 数据库。

## 目录

- `src/`：React 界面、文本操作以及网页/Tauri 数据桥接。
- `src-tauri/src/lib.rs`：SQLite、剪贴板监听、托盘及快速粘贴后端逻辑。
- `src-tauri/icons/`：应用和托盘图标资源。
