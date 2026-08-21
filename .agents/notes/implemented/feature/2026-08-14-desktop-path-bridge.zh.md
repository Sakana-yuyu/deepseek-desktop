# Agent Note: 桌面端 Host CLI 的 PATH 桥接

Status: implemented

[English](2026-08-14-desktop-path-bridge.md) | 中文

## 问题

桌面端 Host 以 `node apps/cli/lib/bin.js web` 启动，却没有把已选定的工具链放进 PATH。产品代码随后按裸名称查找：插件目录会 `spawn('dsh')`，`dsh plugin` 会 `spawnSync('pnpm')`，bash-local 运行 `bash -c`，MCP stdio 示例使用 `npx`，agent 还会调用 `dsh` 或 `git`。机器上只有私有运行时、或 Git for Windows 不在 PATH 上时，这些查找会失败，即使文件已经存在。

Windows 上这些查找比用户终端更严。Node 在 CVE-2024-27980 加固之后，无 shell 的 `spawn('dsh')` 不会运行 `.cmd`；同一目录里无扩展名的 `dsh` 文件会被优先选中并以 `ENOENT` 失败。`spawnSync('pnpm')` 走 `PATHEXT`，因此 PowerShell 的 `pnpm.ps1` 不是命中。用 GUI 进程再拉起 PowerShell 去写用户 Path 也经常静默失败，新开的终端同样看不到 `dsh`。

## 决策

**外壳写入可被 spawn 的 `dsh` / `pnpm` shim，并把所有必需 CLI 目录前置到 Host PATH。** `%APPDATA%\DeepSeek Harness\bin`（或平台应用数据目录下的 `bin`）位于该 PATH 最前，并写入：

- `dsh.cmd`（以及 Unix 上的 `dsh` 脚本），先把选定 Node 和 pnpm 目录前置到 `PATH`，再 exec 已选定的 Node 与 `apps/cli/lib/bin.js`
- `dsh-launch.json`，记录该 Node、CLI 入口、`$DSH_HOME` 和同一份 `pathPrepend` 目录
- `dsh.exe` — 桌面二进制的硬链接或副本，仅在缺失、大小不同、或比该二进制更旧时刷新；当 `argv0` 为 `dsh` 时，若父控制台可见则附着它，把 sidecar 的 `pathPrepend` 应用到 `PATH`，并在父控制台缺失或隐藏时用 `CREATE_NO_WINDOW` exec sidecar 的 Node，而不是打开 GUI
- `pnpm.cmd`（Unix 上为 `pnpm`）：若选定 pnpm 旁存在 `pnpm.cjs` 则运行 `node …/pnpm.cjs`，否则 `call` `.cmd` / `.bat` 或 exec 选定二进制

因此每个 `dsh` 入口都优先解析预配的 pnpm：任何终端里运行 `dsh plugin` 用的都与桌面端同一个 pnpm 大版本，一个 profile 只落在一个 pnpm store 上；旧版构建写出的无 `pathPrepend` sidecar 仍可解析（前置为空），行为不变。

Windows **不**写入无扩展名的 `dsh` 文件：该名称会挡住 Node `spawn('dsh')` 对 `dsh.cmd` / `dsh.exe` 的查找。重写 shim 时会删除遗留的无扩展名文件。

Host 子进程 PATH 从发现 PATH（进程 PATH，Windows 上再加上用户/系统持久 Path）出发，再前置该 bin 目录、已选定 Node 目录（`node` / `npm` / `npx`）、已选定 pnpm 目录，以及在发现 PATH 上仍解析不到 `git` 或 `bash` 时的常见 Git `cmd`/`bin` 目录。同一份合并 PATH 也写回桌面进程。ripgrep 仍打包在 `@vscode/ripgrep` 内，不加入 PATH。可选的第三方 CLI（`claude`、`codex`、`tmux`）不植入。

主机 pnpm 只有在可被直接 spawn 时才算存在（Windows 上为 `.exe` / `.cmd` / `.bat` / `.com`）。`.ps1` 被忽略，以便改用私有 `pnpm.cmd` 或 bin 里的 shim。

**用户 PATH 只补仍然缺失的项。** Windows 通过注册表把 shim 目录追加到 HKCU 用户 Path，并广播 `WM_SETTINGCHANGE`；仅当发现 PATH 上仍没有可 spawn 的对应命令时，才追加 Node 或 pnpm 目录。Unix 把 `dsh` shim 复制到 `~/.local/bin`，若该目录还不在 PATH 上，则向 `~/.zprofile`（macOS）或 `~/.profile`（Linux）追加带标记的 `export PATH` 块。已有 Path 条目不会被重排或删除。持久化失败只记日志，不阻止 Host 启动。

这建立在[桌面端主机工具链扫描与主目录匹配](2026-08-14-desktop-host-env-and-home-adoption.zh.md)之上：同一次选定的 Node 和 pnpm 同时供给扫描和 shim。

## 曾考虑的替代方案

**把发现到的每个目录（包括 Git）都写入用户 Path。** 不采用：为第三方安装改写用户 Path 会令人意外；Host 进程 PATH 已足够支持 agent 的 `git`/`bash` 调用。

**把 `dsh` 装成全局 npm 包。** 不采用：桌面端已经拥有捆绑树中的特定 CLI 入口；全局安装会解析到另一个版本。

**改 bash-local、`dsh plugin` 或 MCP，改为使用绝对路径。** 不采用：这些查找是建立在 PATH 上的产品约定；桌面 fork 不得修改 `packages/`。

**Windows 上只写 `dsh.cmd` 外加一份无扩展名 Unix 脚本。** 不采用：插件目录里的 Node `spawn('dsh')` 无 shell 时不能运行 `.cmd`，无扩展名文件还会导致 `spawn dsh ENOENT`。

**通过拉起 PowerShell `SetEnvironmentVariable` 写入用户 Path。** 不采用：GUI 进程里该 spawn 经常失败；改为写 HKCU `Environment` 并广播 `WM_SETTINGCHANGE`。

## 后果

新开的终端仍可能要重启后，Explorer 才会重建 Path。Host 本身不需要：它在 spawn 时就收到合并后的 PATH，应用内的 `spawn('dsh')` / `spawnSync('pnpm')` 会解析到 bin shim。之后的工具链重新扫描会覆盖 shim，使其继续指向已选定的 Node。`dsh.exe` 从正在运行的桌面二进制刷新；文件占用时跳过刷新并记日志。尚未安装 Git 的机器仍然没有 `git` 或 `bash`，直到用户自行安装。
