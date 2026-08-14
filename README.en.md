# DeepSeek Harness Desktop

[中文 README](README.md) · [Chinese pair](README.zh.md)

<p align="center">
  <img src="apps/desktop-tauri/app-icon.png" width="120" height="120" alt="DeepSeek Harness" />
</p>

<h1 align="center">DeepSeek Harness</h1>

<p align="center">Cross-platform desktop build · custom title bar · tray · signed auto-update · task-complete alerts</p>

<p align="center">
  <a href="https://github.com/Sakana-yuyu/deepseek-harness-desktop/releases"><img src="https://img.shields.io/github/v/release/Sakana-yuyu/deepseek-harness-desktop?display_name=tag&logo=github&label=Release" alt="Latest release" /></a>
  <a href="https://github.com/Sakana-yuyu/deepseek-harness-desktop/actions/workflows/desktop-release.yml"><img src="https://img.shields.io/github/actions/workflow/status/Sakana-yuyu/deepseek-harness-desktop/desktop-release.yml?logo=githubactions&label=Desktop" alt="Desktop build" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/Sakana-yuyu/deepseek-harness-desktop?logo=opensourceinitiative&label=License" alt="License" /></a>
  <a href="https://v2.tauri.app/"><img src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" alt="Tauri 2" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-native-DEA584?logo=rust&logoColor=white" alt="Rust" /></a>
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#features">Features</a> ·
  <a href="#interface">Interface</a> ·
  <a href="#community">Community</a>
</p>

![Main window](apps/desktop-tauri/screenshots/main.png)

> A thin Tauri shell around DeepSeek Harness `dsh web`: custom title bar, tray, signed updates, and a toast plus chime when a turn completes.

This repository is [Sakana-yuyu](https://github.com/Sakana-yuyu)'s desktop distribution of [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness). The GitHub landing page is Chinese by default.

## Why the desktop build

Upstream owns the agent loop, tools, and Web UI. This fork only adds desktop chrome: a frameless window, OS-aware min/max/close, tray, auto-update, and completion alerts. Framework code stays in `packages/`, so upstream syncs do not require package edits.

## Features

| Icon | Capability | Notes |
| :---: | --- | --- |
| <img src="apps/desktop-tauri/app-icon.png" width="22" height="22" alt="DeepSeek" /> | Native shell | Tauri 2 / Rust hosts the existing `dsh web` UI. |
| <img src="https://cdn.simpleicons.org/tauri/24C8DB" width="22" alt="Title bar" /> | Custom title bar | No native caption buttons. Right on Windows, left on macOS, WM layout on Linux. |
| <img src="https://cdn.simpleicons.org/apple/000000" width="22" alt="Tray" /> | System tray | First close asks minimize-to-tray vs quit and remembers the choice. Menu can change that, show the window, check updates, or quit. |
| <img src="https://cdn.simpleicons.org/github/111827" width="22" alt="Update" /> | Signed auto-update | After the main window opens, checks the `desktop-updater` channel and verifies signatures. Release notes are bilingual. |
| <img src="https://cdn.simpleicons.org/googlechat/34A853" width="22" alt="Notify" /> | Task-complete alert | A completed `turn/end` toasts and plays a chime when the window is unfocused. |
| <img src="https://cdn.simpleicons.org/nodedotjs/5FA04E" width="22" alt="Node" /> | Mirror provision | The installer ships source only. First launch scans host Node / pnpm and `~/.dsh`, then fetches from npmmirror only if missing. |
| <img src="https://cdn.simpleicons.org/rust/000000" width="22" alt="Overlay" /> | Overlay plugin | Host collaboration is a Cordis overlay via `dsh web --patch`, not a `packages/` edit. |

### Platforms

- Windows x64 and x86
- macOS Intel and Apple Silicon
- Linux x64 (AppImage, deb)

Download builds from [Releases](https://github.com/Sakana-yuyu/deepseek-harness-desktop/releases). Details: [desktop README](apps/desktop-tauri/README.md).

## Interface

![Task-complete notification](apps/desktop-tauri/screenshots/notify.png)

## Quick start

1. Open [GitHub Releases](https://github.com/Sakana-yuyu/deepseek-harness-desktop/releases).
2. Install **0.1.0-rc.5-0.5** for your platform.
3. First launch scans host Node / pnpm and an existing `~/.dsh` home, then downloads only what is missing.

## Community

- <img src="https://cdn.simpleicons.org/github/111827" width="16" alt="GitHub" /> [Repository](https://github.com/Sakana-yuyu/deepseek-harness-desktop)
- <img src="https://cdn.simpleicons.org/github/111827" width="16" alt="Releases" /> [Releases](https://github.com/Sakana-yuyu/deepseek-harness-desktop/releases)
- <img src="https://cdn.simpleicons.org/github/111827" width="16" alt="Upstream" /> [Upstream DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
- <img src="https://cdn.simpleicons.org/discourse/111827" width="16" alt="Linux.do" /> [Linux.do](https://linux.do/)
