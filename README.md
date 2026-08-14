# DeepSeek Harness Desktop

English | [中文](README.zh.md)

This repository is [Sakana-yuyu](https://github.com/Sakana-yuyu)'s cross-platform desktop distribution of [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness), the open-source agent harness developed by [DeepSeek AI](https://deepseek.com).

The desktop application keeps the existing `dsh web` experience and wraps it in a lightweight Tauri/WebView shell. It downloads its private Node and pnpm runtime from configurable mirrors, so users do not need to prepare a Harness development environment.

## Download

Download the current **0.1.0-rc.5** prerelease from [GitHub Releases](https://github.com/Sakana-yuyu/deepseek-desktop/releases/tag/desktop-v0.1.0-rc.5).

- Windows x64 and x86: NSIS installer
- macOS Intel and Apple Silicon: DMG
- Linux x64: AppImage and deb

The first launch downloads Node and production dependencies. Later launches reuse the installed runtime. Desktop updates are signature-verified before installation; Windows upgrades close the existing application process before replacing files.

## Desktop design

- Rust/Tauri native shell instead of Electron
- Existing DeepSeek Harness Web UI and plugin architecture
- Compact installer with mirror-fetched runtime dependencies
- Isolated Harness source directories to avoid file-lock conflicts during upgrades
- Single-instance startup, hidden subprocess consoles, signed update manifests, and multilingual Windows installation
- One transparent DeepSeek fish icon across the executable, windows, taskbar, shortcuts, and installers

Implementation and build details live in the [desktop README](apps/desktop-tauri/README.md).

## Run

Install Node.js, then run the upstream npm package:

```sh
npx @deepseek-ai/dsh web
```

### Run from source

To build this fork from source:

```sh
git clone https://github.com/Sakana-yuyu/deepseek-desktop.git
cd deepseek-desktop
pnpm install
pnpm run build
pnpm --dir apps/desktop-tauri run build
```

## Contributing

Desktop issues and contributions belong in this fork. Harness framework contributions should follow the upstream [contribution guide](CONTRIBUTING.md), [development guide](docs/development.md), and [architecture documentation](docs/architecture.md).

## License

DeepSeek Harness and this desktop distribution use the [MIT license](LICENSE). Third-party dependencies and their licenses are disclosed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
