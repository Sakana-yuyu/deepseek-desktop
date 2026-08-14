# Agent Note: 桌面端 Host CLI 的 PATH 桥接

Status: implemented

[English](2026-08-14-desktop-path-bridge.md) | 中文

## 问题

桌面端 Host 以 `node apps/cli/lib/bin.js web` 启动，却没有把已选定的工具链放进 PATH。产品代码随后按裸名称查找：`dsh plugin` 会 `spawnSync('pnpm')`，bash-local 运行 `bash -c`，MCP stdio 示例使用 `npx`，agent 还会调用 `dsh` 或 `git`。机器上只有私有运行时、或 Git for Windows 不在 PATH 上时，这些查找会失败，即使文件已经存在。

## 决策

**外壳写入 `dsh` shim，并把所有必需 CLI 目录前置到 Host PATH。** `%APPDATA%\DeepSeek Harness\bin`（或平台应用数据目录下的 `bin`）写入 `dsh.cmd` 和一份 `dsh` shell 脚本，二者都 exec 已选定的 Node 与 `apps/cli/lib/bin.js`。Host 子进程 PATH 从发现 PATH（进程 PATH，Windows 上再加上用户/系统持久 Path）出发，再前置该 bin 目录、已选定 Node 目录（`node` / `npm` / `npx`）、已选定 pnpm 目录，以及在发现 PATH 上仍解析不到 `git` 或 `bash` 时的常见 Git `cmd`/`bin` 目录。ripgrep 仍打包在 `@vscode/ripgrep` 内，不加入 PATH。可选的第三方 CLI（`claude`、`codex`、`tmux`）不植入。

**用户 PATH 只补仍然缺失的项。** Windows 把 shim 目录追加到 HKCU 用户 Path；仅当发现 PATH 上仍没有对应命令时，才追加 Node 或 pnpm 目录。Unix 把 `dsh` shim 复制到 `~/.local/bin`，若该目录还不在 PATH 上，则向 `~/.zprofile`（macOS）或 `~/.profile`（Linux）追加带标记的 `export PATH` 块。已有 Path 条目不会被重排或删除。

这建立在[桌面端主机工具链扫描与主目录匹配](2026-08-14-desktop-host-env-and-home-adoption.md)之上：同一次选定的 Node 和 pnpm 同时供给扫描和 shim。

## 曾考虑的替代方案

**把发现到的每个目录（包括 Git）都写入用户 Path。** 不采用：为第三方安装改写用户 Path 会令人意外；Host 进程 PATH 已足够支持 agent 的 `git`/`bash` 调用。

**把 `dsh` 装成全局 npm 包。** 不采用：桌面端已经拥有捆绑树中的特定 CLI 入口；全局安装会解析到另一个版本。

**改 bash-local、`dsh plugin` 或 MCP，改为使用绝对路径。** 不采用：这些查找是建立在 PATH 上的产品约定；桌面 fork 不得修改 `packages/`。

## 后果

新开的终端可能需要重启后才能在用户 Path 上看到 `dsh`。Host 本身不需要：它在 spawn 时就收到合并后的 PATH。之后的工具链重新扫描会覆盖 shim，使其继续指向已选定的 Node。尚未安装 Git 的机器仍然没有 `git` 或 `bash`，直到用户自行安装。
