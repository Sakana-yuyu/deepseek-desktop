# DeepSeek Harness Desktop

[English](README.md) | 中文

<p align="center">
  <img src="apps/desktop-tauri/app-icon.png" width="120" height="120" alt="DeepSeek Harness" />
</p>

<h1 align="center">DeepSeek Harness</h1>

<p align="center">跨平台桌面发行版 · 自定义标题栏 · 托盘 · 签名自动更新 · 任务完成提醒</p>

<p align="center">
  <a href="https://github.com/Sakana-yuyu/deepseek-harness-desktop/releases"><img src="https://img.shields.io/github/v/release/Sakana-yuyu/deepseek-harness-desktop?display_name=tag&logo=github&label=Release" alt="Latest release" /></a>
  <a href="https://github.com/Sakana-yuyu/deepseek-harness-desktop/actions/workflows/desktop-release.yml"><img src="https://img.shields.io/github/actions/workflow/status/Sakana-yuyu/deepseek-harness-desktop/desktop-release.yml?logo=githubactions&label=Desktop" alt="Desktop build" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/Sakana-yuyu/deepseek-harness-desktop?logo=opensourceinitiative&label=License" alt="License" /></a>
  <a href="https://v2.tauri.app/"><img src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" alt="Tauri 2" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-native-DEA584?logo=rust&logoColor=white" alt="Rust" /></a>
  <a href="README.en.md"><img src="https://img.shields.io/badge/docs-English-9aa3b5" alt="English" /></a>
</p>

<p align="center">
  <a href="#快速开始">快速开始</a> ·
  <a href="#功能概览">功能概览</a> ·
  <a href="#界面">界面</a> ·
  <a href="#社区与反馈">社区与反馈</a>
</p>

![主窗口](apps/desktop-tauri/screenshots/main.png)

> 把 DeepSeek Harness 的 `dsh web` 装进轻量 Tauri 壳：自己画标题栏，托盘常驻，签名更新，任务完成会弹通知、会响。

本仓库是 [Sakana-yuyu](https://github.com/Sakana-yuyu) 维护的 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) 跨平台桌面发行版。DeepSeek Harness 是由 [DeepSeek AI](https://deepseek.com) 开发的开源 agent harness。英文说明见 [README.en.md](README.en.md)。

## 为什么使用桌面版

上游负责 agent 循环、工具和 Web UI。这个发行版只补桌面该有的东西：无边框窗口、按操作系统放置的最小化/最大化/关闭、托盘、自动更新、完成提醒。框架代码仍在 `packages/`，同步上游时不用改那些包。

## 功能概览

| 图标 | 能力 | 说明 |
| :---: | --- | --- |
| <img src="apps/desktop-tauri/app-icon.png" width="22" height="22" alt="DeepSeek" /> | 原生桌面壳 | Tauri 2 / Rust 窗口承载现有 `dsh web`，不把业务搬进 Electron。 |
| <img src="https://cdn.simpleicons.org/tauri/24C8DB" width="22" alt="Title bar" /> | 自定义标题栏 | 取消系统原生按钮。Windows 在右，macOS 在左，Linux 读窗口管理器布局。 |
| <img src="https://cdn.simpleicons.org/apple/000000" width="22" alt="Tray" /> | 系统托盘 | 第一次关闭询问最小化到托盘还是退出，并记住选择。菜单可改该偏好、显示窗口、检查更新或退出。 |
| <img src="https://cdn.simpleicons.org/github/111827" width="22" alt="Update" /> | 签名自动更新 | 主窗口打开后检查 `desktop-updater` 通道，校验签名后再安装。发布说明为中英双语。 |
| <img src="https://cdn.simpleicons.org/googlechat/34A853" width="22" alt="Notify" /> | 任务完成提醒 | 一轮 `turn/end` 完成且窗口不在前台时，弹出系统通知并播放完成音。 |
| <img src="https://cdn.simpleicons.org/nodedotjs/5FA04E" width="22" alt="Node" /> | 镜像预配 | 安装包只带源码。首次启动先扫描本机 Node / pnpm 和 `~/.dsh`，只在缺失时从 npmmirror 拉取。 |
| <img src="https://cdn.simpleicons.org/rust/000000" width="22" alt="Overlay" /> | Overlay 插件 | 和 Host 协作的功能写成 Cordis overlay，经 `dsh web --patch` 植入，不改 `packages/`。 |

### 支持的平台

- Windows x64、Windows x86
- macOS Intel、macOS Apple Silicon
- Linux x64（AppImage、deb）

发布包由 GitHub Actions 构建，前往 [Releases](https://github.com/Sakana-yuyu/deepseek-harness-desktop/releases) 下载对应平台版本。

## 界面

自定义标题栏取消系统原生最大化、最小化和关闭按钮。第一次关闭询问最小化到托盘还是退出，并写入 `desktop-settings.json`。

![任务完成通知](apps/desktop-tauri/screenshots/notify.png)

任务完成后，若主窗口不在前台，会弹出系统通知并播放完成音。

## 快速开始

### 1. 下载并安装

1. 打开 [GitHub Releases](https://github.com/Sakana-yuyu/deepseek-harness-desktop/releases)。
2. 下载当前 **0.1.0-rc.8-0.1** 对应平台的安装包。
3. Windows 如果出现 SmartScreen 提示，确认来源后选择“更多信息”并继续运行。
4. 首次启动先扫描本机 Node / pnpm 和已有 `~/.dsh` 对话与密钥，只在缺失时下载运行时和生产依赖，后续启动复用已匹配的环境。

桌面更新在安装前验证签名；Windows 升级会先关闭已有应用进程，再替换文件。已安装的 rc.5 之后会自动发现更高版本。

### 2. 同步上游时只动壳

Harness 框架代码在 `packages/`、`apps/cli`、`apps/web`。桌面能力全部留在 `apps/desktop-tauri/`。同步上游时拉取框架目录即可，不要把桌面改动写进 `packages/`。

实现与构建细节见[桌面端 README](apps/desktop-tauri/README.md)。

<a id="run"></a>

## 运行

安装 Node.js，然后运行上游 npm 包：

```sh
npx @deepseek-ai/dsh web
```

<a id="run-from-source"></a>

### 从源码运行

从源码构建本 fork：

```sh
git clone https://github.com/Sakana-yuyu/deepseek-harness-desktop.git
cd deepseek-harness-desktop
pnpm install
pnpm run build
pnpm --dir apps/desktop-tauri run build
```

## 社区与反馈

- <img src="https://cdn.simpleicons.org/github/111827" width="16" alt="GitHub" /> [GitHub 仓库](https://github.com/Sakana-yuyu/deepseek-harness-desktop)
- <img src="https://cdn.simpleicons.org/github/111827" width="16" alt="Releases" /> [GitHub Releases](https://github.com/Sakana-yuyu/deepseek-harness-desktop/releases)
- <img src="https://cdn.simpleicons.org/github/111827" width="16" alt="Upstream" /> [上游 DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
- <img src="https://cdn.simpleicons.org/discourse/111827" width="16" alt="Linux.do" /> [Linux.do 社区](https://linux.do/)

桌面端问题请提到本 fork。Harness 框架问题请走上游仓库。

## 参与贡献

桌面端问题与贡献请提交到本 fork。Harness 框架贡献请遵循上游的[贡献指南](CONTRIBUTING.md)、[开发指南](docs/development.md)和[架构文档](docs/architecture.md)。

<p align="center">
  <a href="https://github.com/Sakana-yuyu"><img src="https://avatars.githubusercontent.com/Sakana-yuyu?s=80" width="48" height="48" alt="Sakana-yuyu" title="Sakana-yuyu" /></a>
</p>

## 许可证

DeepSeek Harness 和此桌面发行版均使用 [MIT 许可证](LICENSE)。第三方依赖及其许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
