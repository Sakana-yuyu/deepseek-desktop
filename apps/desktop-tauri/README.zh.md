# DeepSeek Harness Desktop (Tauri)

[English](README.md) | 中文

这是现有 `dsh web` 界面的 Rust/WebView2 外壳。安装包携带 **Harness 源码**，不包含 `node_modules`；首次运行先扫描本机兼容的 Node / pnpm 和已有的 `~/.dsh` 主目录，只在扫描失败时从镜像拉取构建工具，再对安装包内的源码树执行 `pnpm install --prod`。

当前桌面发行版本：**0.1.0-rc.5-0.5**。

## 架构

| 层 | 安装包内容 | 首次运行 |
|---|---|---|
| **安装包** | Tauri 二进制、启动页、裁剪后的 monorepo 子集（`bundled/harness/`） | — |
| **构建环境** | — | 复用本机 Node 22.19+ 或 24+ 和 pnpm；没有时再从 npmmirror 下载 Node，并通过 npm 安装 pnpm |
| **依赖** | — | 在平台应用数据目录执行 `pnpm install --prod --no-frozen-lockfile`（裁剪包与 lockfile 不完全相同；移除 `CI`，避免 pnpm 强制冻结安装） |
| **Host** | — | `node apps/cli/lib/bin.js web --host 127.0.0.1`；启动时加载失败的插件会被禁用，然后重试 Host |
| **UI** | 本地 `shell.html` 标题栏 | 无边框窗口嵌入 `dsh web`；启动页是紧贴内容的透明窗口，中间一块无边框、四周淡出的毛玻璃，带官方鱼形标志、DeepSeek 字标和进度条；第一次关闭是与 web 客户端一致的页内浅色对话框；Windows 控件在右，macOS 在左，Linux 读取窗口管理器按钮布局 |
| **托盘** | 原生托盘图标 | 第一次关闭询问最小化到托盘还是退出，并写入 `desktop-settings.json`；托盘可改该偏好、显示窗口、检查更新或退出。退出会停止 Host 的 Node 进程树；最小化到托盘则保持运行 |
| **通知** | Overlay 插件 + 本机 POST | `turn/end` 且 `completed` 时，窗口不在前台则弹出系统通知并播放 `sounds/complete.wav` |
| **更新** | 内置更新公钥 | 检查稳定的 GitHub 更新 manifest，验证下载产物签名，安装并重启 |

安装包内的源码树包括：带已构建 `lib/` 的 `apps/cli`、带 `dist/` 的 `apps/web`、除 examples 与 test-support 外的 `packages/*/*`、`native/landlock-run`、`vendor/*`、`patches/` 和 lockfile。构建安装包时会移除 workspace 的 `devDependencies`，使 `--prod` 安装无需解析演示包。

### 镜像（可通过环境变量覆盖）

| 变量 | 默认值 |
|---|---|
| `DSH_NODE_MIRROR` | `https://npmmirror.com/mirrors/node` |
| `DSH_NPM_REGISTRY` | `https://registry.npmmirror.com` |

### 开发与生产模式

| 模式 | 环境变量 | 行为 |
|---|---|---|
| **本地开发** | `DSH_DESKTOP_LAUNCH=local` | 使用 monorepo checkout 和 `PATH` 中的 `node`/`pnpm`，跳过镜像下载 |
| **生产** | （默认） | 从安装包复制 `harness-source` 到应用数据目录，通过镜像安装，再启动应用 |

可写目录位于平台应用数据目录下：Windows 使用 `%APPDATA%\DeepSeek Harness`，macOS 使用 `~/Library/Application Support/ai.deepseek.dsh-desktop`，Linux 使用 Tauri 返回的平台数据目录。

- `harness-versions/<bundle-hash>/` — 按源码包隔离的源码，以及首次 `pnpm install` 后的 `node_modules`
- `runtime/` — Node、pnpm-global 和 manifest
- `dsh-home/` — 未发现已有 Harness 主目录时的回退会话数据
- `bin/` — 写入 Host PATH 的可 spawn `dsh.exe` / `dsh.cmd` / `pnpm.cmd`；用户 Path 缺失时也会加入
- `cache/` — 下载的 Node zip 或 tarball

首次启动会先扫描进程 `PATH`（Windows 上再加上用户/系统里的持久 Path）和常见安装位置，查找满足 `^22.19 || >=24` 的 Node 和可用的 pnpm，再决定是否从镜像下载。若 `$DSH_HOME`（进程环境，或 Windows 上的用户/系统环境）或 `~/.dsh` 已包含会话、凭据、`.env`、profile 或 settings，则采用该主目录，并把隔离 `dsh-home/` 中缺失的文件复制进去。随后写入可被 spawn 的 `dsh` / `pnpm` shim（`dsh.exe` 是以 CLI 跳板运行的桌面二进制），并把已选定的 Node / pnpm 目录（以及发现 PATH 上缺少 `git` 或 `bash` 时的 Git `cmd`/`bin`）前置到 Host PATH，使应用内的 `spawn('dsh')`、`dsh plugin`、MCP 的 `npx` 以及 agent 的 `bash`/`git` 查找能够解析。Windows 不写入无扩展名的 `dsh` 文件。父进程没有可见控制台时，Windows 上的 Host 与 CLI 子进程不会再弹出控制台窗口。用户 Path 通过注册表加入 shim 目录；仅当对应命令仍不是可 spawn 文件时，才加入 Node 或 pnpm 目录。扫描失败时，预配器仍会为 Windows x64/x86、macOS x64/arm64 和 Linux x64/arm64 下载 Node。zip 与 tar.gz 解压会拒绝预期 Node 根目录以外的条目。Unix 压缩包保留可执行权限。私有安装的 pnpm 由已选定的 Node 二进制执行；扫描到主机 pnpm 时则直接调用。按源码包隔离的 Harness 目录允许更新版本预配新源码，而不删除旧 Host 正在使用的文件；兼容的 Node 和 pnpm 会在源码更新之间复用。原生外壳只允许一个应用实例，重复启动时聚焦已有窗口。Release 构建会在主窗口打开后再检查更新；更新网络或 manifest 失败只写入日志，不拖住启动页。运行时 manifest 已就绪时跳过主机工具链扫描，并用文件大小比对 Node，不再对 `node.exe` 做 SHA256。窗口标题栏、托盘、更新、系统通知和完成音都留在这个 Rust crate。与 Host 的协作是复制到 `$DSH_HOME/desktop-overlay` 的 overlay 插件，通过 `dsh web --patch` 加载，不修改 `packages/`。

## 构建

在仓库根目录执行（需要已构建的 CLI 与 Web dist）：

```powershell
pnpm run build
cd apps/desktop-tauri
pnpm install
$env:TAURI_SIGNING_PRIVATE_KEY=(Get-Content "$HOME\.tauri\deepseek-desktop-updater.key" -Raw)
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD=(Get-Content "$HOME\.tauri\deepseek-desktop-updater.key.password" -Raw)
pnpm run build:win
```

安装包输出：`src-tauri/target/release/bundle/nsis/DeepSeek Harness_0.1.0-rc.5-0.5_x64-setup.exe`

NSIS 安装包包含**英语**、**简体中文**和**繁体中文**。安装语言自动跟随操作系统 locale，不显示语言选择器；不支持的 locale 使用英语。复制文件前，安装器会静默关闭 `dsh-desktop.exe` 及其子进程树。安装后，安装器使用独立的版本化 ICO 资源重建已有桌面快捷方式，并通知 Explorer 清除陈旧的图标缓存记录。

## 发布

推送 `desktop-v*` tag 会运行[桌面发布工作流](../../.github/workflows/desktop-release.yml)。该工作流构建 Windows x64/x86 NSIS 安装包、macOS Intel/Apple Silicon DMG 和 Linux x64 AppImage/deb，并在所有矩阵任务成功后发布一个 GitHub Release。工作流为每个更新产物生成签名，用双语[发布说明](release-notes.md)生成 Tauri `latest.json`，再替换稳定 `desktop-updater` Release 通道中的 manifest。手动触发可重新构建现有 tag。

Release 资产名称包含操作系统和架构。更新签名用于向已安装应用验证产物，但可执行文件没有操作系统代码签名，也未经过 notarization，因此 Windows SmartScreen、macOS Gatekeeper 或 Linux 桌面安全提示可能要求用户明确批准。

Windows、macOS 和 Linux 图标均从 `app-icon.svg` 生成；其中使用与 `packages/client/ui-primitives/src/FishLogo.tsx` 相同的透明背景黑色鱼形路径。Tauri bundle、NSIS 安装器与卸载器、启动窗口、主窗口标题栏、任务栏、Dock 和 Linux desktop entry 均使用这套图标。Windows 安装还携带文件名含版本的 ICO 文件，避免快捷方式图标查询复用旧的可执行文件路径缓存键。

只生成源码包（不运行 Tauri）：

```powershell
node scripts/bundle-harness-source.mjs
```

## 运行

**开发模式（monorepo checkout）：**

```powershell
# repo root: pnpm run build  (once)
cd apps/desktop-tauri
$env:DSH_DESKTOP_LAUNCH='local'
pnpm run dev
```

**已安装应用：**运行 NSIS 安装包；首次启动会在启动页扫描本机环境、匹配已有 `~/.dsh` 主目录，并只安装缺失的工具或依赖，完成后打开 Web UI。

## 脚本

| 脚本 | 用途 |
|---|---|
| `scripts/bundle-harness-source.mjs` | 裁剪并复制 monorepo 子集到 `bundled/harness/` |
| `scripts/prepare-dist.mjs` | 生成启动页 dist 与源码包（Tauri `beforeBuildCommand`） |
| `scripts/serve-dist.mjs` | 为 `tauri dev` 启动静态启动页服务器 |
| `overlay/desktop-notify/` | Cordis overlay：把已完成的 turn 发到原生通知端口 |
