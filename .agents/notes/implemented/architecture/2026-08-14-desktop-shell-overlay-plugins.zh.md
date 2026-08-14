# Agent Note: 桌面壳标题栏与 overlay 插件

Status: implemented

[English](2026-08-14-desktop-shell-overlay-plugins.md) | 中文

## 问题

桌面 fork 必须提供窗口标题栏、托盘、签名更新和任务完成提醒，同时不能修改上游 Harness 包。后续同步应原样拉取 `packages/`、`apps/cli` 和 `apps/web`。必须观察 Host session 事件的功能不能只放在 WebView 里，因为那会改已发布的 web 客户端。

## 决策

**原生标题栏留在 `apps/desktop-tauri`。** 主窗口无边框。本地 `shell.html` 标题栏承载鱼形标志、标题和最小化/最大化/关闭。Windows 把这些按钮放在右侧；macOS 放在左侧；Linux 解析窗口管理器按钮布局（`gsettings` / XFCE，可用 `DSH_DESKTOP_BUTTON_LAYOUT` 覆盖），并允许左右分置。关闭会隐藏窗口；进程留在托盘，直到选择退出。

**与 Host 的协作是 overlay 插件，不是改包。** 外壳把 `overlay/desktop-notify/index.mjs` 复制到 `$DSH_HOME/desktop-overlay`，写入插件 `name` 为 `file://` URL 的 `--patch` 列表，再启动 `dsh web --patch <该文件>`。Windows 盘符路径（如 `C:/...`）不是合法 ESM specifier——Node 会把 `C:` 当成 URL scheme——因此 overlay 必须写成 `file:///C:/...`（空格做百分号编码）。插件监听 `session/event` 中 `turn/end` 且 `reason.kind === 'completed'`，并向 `DSH_DESKTOP_NOTIFY_URL` 给出的本机通知 URL 发送 POST。Rust 监听端只在主窗口不在前台时弹出系统通知并播放 `sounds/complete.wav`。

**更新仍使用带签名的 Tauri updater。** 启动时仍在预配前检查。托盘「检查更新」按需运行同一套签名检查。

这是对[跨平台桌面源码预配](../feature/2026-08-14-cross-platform-desktop-source-provisioning.md)的延伸，并不把桌面行为移进 `packages/`。

## 曾考虑的替代方案

**改 `apps/web` 或某个 `packages/*` 插件。** 不采用：每次同步上游都要重做或丢失桌面行为。overlay 改用文档中的 `--patch` 层。

**直接写 `$DSH_HOME/cordis.patch.yml`。** 不采用：该文件是用户的 home 级 patch 层。生成独立的 `--patch` 文件，把 home 文件留给用户。

**把标题栏注入 React DOM。** 不采用：这会让标题栏耦合 web 客户端标记，且仍无法拥有托盘、更新或操作系统通知。

**标题栏关闭按钮直接退出。** 不采用：编码会话不应因误点关闭而结束；进程生命周期由托盘负责。

## 后果

上游框架目录不再包含仅桌面使用的插件行。缺少 overlay 文件时 Host 启动会明确失败。用户已有的 home `cordis.patch.yml` 保持不动。窗口在前台时的 turn 不弹通知、不播放完成音。读不到按钮布局的 Linux 主机使用 Windows 风格的右侧控件。`apps/desktop-tauri/screenshots/` 中的截图用于说明外壳外观，不是从实况会话录制。
