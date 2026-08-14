# DeepSeek Harness Desktop 0.1.0-rc.5-0.4

## English

DeepSeek Harness Desktop is a Tauri/WebView shell for the existing `dsh web` interface.

The installer contains a trimmed Harness source tree without `node_modules`. On first launch, the app scans the host for Node / pnpm and an existing `~/.dsh` home, downloads only what is missing, starts the local Harness web host, and opens it in the desktop WebView.

### What's new

- First close asks whether to minimize to tray or quit, and remembers the choice. The tray can change it later.
- The shell writes spawnable `dsh` / `pnpm` commands and puts them on PATH so in-app `dsh`, plugins, MCP `npx`, and a new terminal can find the selected toolchain.
- On Windows, `dsh.exe` is a CLI trampoline: running it as `dsh` starts the Harness CLI instead of the GUI.
- Broken profile installs are repaired with `dsh plugin --profile <name> install` before the Host starts.
- File-in-use and access-denied errors during provision fall back to an existing runtime instead of aborting boot.
- Signed update checks run after the main window opens, so a slow update network no longer holds the splash.

Windows installation closes a running desktop process before replacing files and refreshes an existing desktop shortcut with the versioned DeepSeek fish icon.

### Included builds

- Windows x64 and x86 NSIS installers
- macOS Intel and Apple Silicon DMGs
- Linux x64 AppImage and deb packages

These artifacts are not operating-system code-signed or notarized. Windows SmartScreen, macOS Gatekeeper, or Linux desktop security prompts may require explicit approval.

## 中文

DeepSeek Harness Desktop 是现有 `dsh web` 界面的 Tauri/WebView 外壳。

安装包包含裁剪后的 Harness 源码树，不含 `node_modules`。首次启动会扫描本机 Node / pnpm 和已有 `~/.dsh` 主目录，只下载缺失部分，然后启动本地 Harness Web Host，并在桌面 WebView 中打开。

### 更新内容

- 第一次关闭窗口会询问最小化到托盘还是退出，并记住选择；之后可在托盘菜单更改。
- 外壳会写入可直接 spawn 的 `dsh` / `pnpm` 命令并加入 PATH，应用内的 `dsh`、插件、MCP 的 `npx` 以及新开的终端都能找到已选定的工具链。
- Windows 上的 `dsh.exe` 是 CLI 跳板：以 `dsh` 启动时运行 Harness CLI，而不是打开图形界面。
- Host 启动前会检查 profile 依赖，损坏的安装会先执行 `dsh plugin --profile <name> install`。
- 预配遇到文件占用或权限不足时，会回退到已有运行时，而不是直接启动失败。
- 签名更新检查改到主窗口打开之后进行，更新网络慢时不再拖住启动页。

Windows 安装会先关闭正在运行的桌面进程再替换文件，并用带版本号的 DeepSeek 鱼形图标刷新已有桌面快捷方式。

### 包含的构建

- Windows x64 和 x86 NSIS 安装包
- macOS Intel 和 Apple Silicon DMG
- Linux x64 AppImage 和 deb 包

这些产物没有操作系统代码签名，也未经过 notarization。Windows SmartScreen、macOS Gatekeeper 或 Linux 桌面安全提示可能要求用户明确批准。
