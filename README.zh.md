# DeepSeek Harness Desktop

[English](README.md) | 中文

本仓库是 [Sakana-yuyu](https://github.com/Sakana-yuyu) 维护的 [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) 跨平台桌面发行版。DeepSeek Harness 是由 [DeepSeek AI](https://deepseek.com) 开发的开源 agent harness（智能体框架）。

桌面应用保留现有 `dsh web` 使用体验，并通过轻量的 Tauri/WebView 原生外壳运行。应用从可配置镜像下载私有 Node 和 pnpm 运行时，用户无需预先搭建 Harness 开发环境。

## 下载

从 [GitHub Releases](https://github.com/Sakana-yuyu/deepseek-desktop/releases/tag/desktop-v0.1.0-rc.5) 下载当前 **0.1.0-rc.5** 预发布版本。

- Windows x64 和 x86：NSIS 安装包
- macOS Intel 和 Apple Silicon：DMG
- Linux x64：AppImage 和 deb

首次启动会下载 Node 和生产依赖，后续启动复用已安装的运行时。桌面更新在安装前验证签名；Windows 升级会先关闭已有应用进程，再替换文件。

## 桌面端设计

- 使用 Rust/Tauri 原生外壳，而不是 Electron
- 保留 DeepSeek Harness Web UI 和插件架构
- 使用紧凑安装包，并通过镜像拉取运行时依赖
- 按 Harness 源码包隔离目录，避免升级时发生文件占用冲突
- 支持单实例启动、隐藏子进程窗口、签名更新 manifest 和多语言 Windows 安装
- 可执行文件、窗口、任务栏、快捷方式和安装器统一使用透明背景的 DeepSeek 鱼形图标

实现与构建细节见[桌面端 README](apps/desktop-tauri/README.md)。

## 运行

安装 Node.js，然后运行上游 npm 包：

```sh
npx @deepseek-ai/dsh web
```

### 从源码运行

从源码构建本 fork：

```sh
git clone https://github.com/Sakana-yuyu/deepseek-desktop.git
cd deepseek-desktop
pnpm install
pnpm run build
pnpm --dir apps/desktop-tauri run build
```

## 参与贡献

桌面端问题与贡献请提交到本 fork。Harness 框架贡献请遵循上游的[贡献指南](CONTRIBUTING.md)、[开发指南](docs/development.md)和[架构文档](docs/architecture.md)。

## 许可证

DeepSeek Harness 和此桌面发行版均使用 [MIT 许可证](LICENSE)。第三方依赖及其许可证见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
