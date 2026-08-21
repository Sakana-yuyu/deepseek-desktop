# Agent Note: 桌面端重复启动复用已记录的主机工具链

Status: implemented

[English](2026-08-17-desktop-repeat-boot-host-toolchain.md) | 中文

## 问题

桌面端“运行时 manifest 已就绪”的跳过路径要求私有的 `runtime/node` 和 `runtime/pnpm-global` 文件存在，并返回这两条私有路径。[主机工具链扫描](../feature/2026-08-14-desktop-host-env-and-home-adoption.zh.md) 会复用本机已有的兼容 Node 和 pnpm，因此这些私有文件常常从未被创建。之后每次启动都会删除按哈希隔离的 harness 树（包括 `node_modules`），并再次执行 `pnpm install --prod`。本机已安装 Node `^22.19 || >=24` 的用户因此每次都要在启动页上等数分钟。PATH 桥的 `dsh.exe` shim 也会在每次启动时从桌面二进制解除链接并重新复制。

## 决策

`ready_toolchain` 从运行时 manifest 读取 `nodePath` 和 `pnpmPath`。当 bundle 哈希仍匹配、`apps/cli/lib/bin.js` 与 `node_modules/.pnpm` 存在、且这些二进制（或私有回退文件）仍在时，预配返回已记录路径，并跳过主机扫描、源码释放和 `pnpm install`。已有 CLI 入口的 harness 目录不会被清空；只有缺少 `.pnpm` 时才运行 `pnpm install`。manifest 会记录 `pnpmPath`。仅当 `dsh.exe` shim 缺失、大小不同、或比桌面二进制更旧时才刷新。

本笔记只负责重复启动的跳过路径。主机匹配、主目录采用和首次镜像拉取仍由[主机工具链扫描与主目录匹配](../feature/2026-08-14-desktop-host-env-and-home-adoption.zh.md)和[跨平台源码预配](../feature/2026-08-14-cross-platform-desktop-source-provisioning.zh.md)负责。

## 曾考虑的替代方案

**始终下载私有 Node，让旧的跳过路径继续要求这些文件。** 不采用：复用本机工具链是 host-env 决策；强制私有副本会取消该决策，并且首次启动仍要下载压缩包。

**每次启动都对 `node.exe` 做哈希。** 不采用：现有的文件大小检查已经识别已记录的二进制；在启动页路径上哈希大型 Node，正是该检查要避免的开销。

**每次预配都删除 harness 树，以保持与捆绑资源完全一致。** 不采用：目标目录已经按哈希隔离；新 bundle 对应新目录，清空仍在使用的树正是让本机 Node 启动重复 `pnpm install` 的原因。

## 后果

之后的启动只要仍有已释放的源码树和已记录的 Node / pnpm，就会跳过释放和安装，即使这些二进制是主机路径。没有 `pnpmPath` 的旧 manifest 在私有 pnpm 文件存在时使用它；否则一次扫描仍会在 `node_modules/.pnpm` 已存在时跳过释放和安装，然后写入 `pnpmPath`。主机 Node 消失时跳过失败，回退到扫描。桌面 crate 测试钉住主机路径复用、缺少 `node_modules` 时拒绝、源码树复用、私有 pnpm 回退，以及 `dsh.exe` 刷新跳过。
