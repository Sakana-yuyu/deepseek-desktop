# Agent Note: 托盘安装 Sakana 插件库

Status: implemented

[English](2026-08-17-desktop-tray-plugin-catalog.md) | 中文

## Problem

桌面 Host 的 `web` profile 不含插件目录。安装 [dsh-plugins](https://github.com/Sakana-yuyu/dsh-plugins) 必须对**正在使用的** Host 主目录执行 `dsh plugin --profile web add github:Sakana-yuyu/dsh-plugins`（Windows 桌面 `$DSH_HOME`，或 WSL 发行版内的 `~/.dsh`）。让操作者自己开终端并选对 `dsh` / `DSH_HOME` 很容易弄错，尤其是在 PATH 桥和 WSL 分流之后。

## Decision

**托盘一项把该插件库装进当前 Host profile，然后重启。** 文案为「安装插件库」/ Install plugin catalog。规格写死：`github:Sakana-yuyu/dsh-plugins`（包名 `dsh-plugins-catalog`，带已构建的 `index.js`，没有 `prepare` 脚本）。外壳用 Host 已经在用的 Node、CLI、`DSH_HOME` 和 PATH 运行 `node <cli> plugin --profile web add <spec>`。WSL 走 `wsl.exe -d <发行版> --exec` 和 Linux Node，从不执行 `node.exe`。安装进行中的第二次点击会被忽略。成功则 toast 并调用与托盘「重启」相同的 `request_restart`，让 Host 重新加载新的 profile 层。失败则 toast 进程输出尾部，不重启。启动页尚未交出 `DesktopRuntime` 时该项实际不可用。不修改 `packages/`。

## Alternatives considered

**托盘只打开 GitHub 页面。** 否决：操作者仍要对正确的主目录自己跑 `dsh plugin`。

**弹出任意 git 规格。** 否决：本次只装这一份已知目录；通用安装器还要处理只带源码的包的 `allowBuilds`。

**安装后不重启。** 否决：`dsh web` 已经组合过 `web` profile；新 bundle 要等 Host 再次启动才可见。

## Consequences

Windows 与 WSL 主目录仍然分开：在 Windows 上安装不会写进 WSL 的 `~/.dsh`。必须能访问 GitHub；`github.com` 不通时托盘项失败并带上 pnpm/git 输出尾部。再次点击会更新已有的 git 依赖。
