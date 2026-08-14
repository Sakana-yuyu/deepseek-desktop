# Agent Note: 桌面端主机工具链扫描与主目录匹配

Status: implemented

[English](2026-08-14-desktop-host-env-and-home-adoption.md) | 中文

## 问题

桌面安装包把 `DSH_HOME` 隔离在应用数据目录，并且在本机已有兼容工具链或 CLI/Web Harness 主目录时仍会下载 Node。已经用过 `dsh web` 或 `dsh` 的用户在首次启动桌面端时看不到原有会话和 API 密钥，首次启动还会重复一次长达数分钟的运行时下载。

## 决策

**预配在任何镜像下载之前先扫描主机。** 外壳探测私有运行时、进程 `PATH` 和常见 Node/pnpm 安装位置。在 Windows 上还会读取用户/系统环境中的持久 Path（以及 `DSH_HOME`），因为从开始菜单启动时，GUI 进程 PATH 常常是过期的。满足 `^22.19 || >=24` 的 Node 会被复用于 Host 启动和 `pnpm install`。可用的 pnpm 同样复用，包括选定 Node 旁边的 Corepack shim。只有扫描不到兼容 Node 时才下载私有 Node 压缩包；只有找不到可用 pnpm 时，才通过已选定的 Node 安装 pnpm。这是对[跨平台桌面源码预配](2026-08-14-cross-platform-desktop-source-provisioning.md)的延伸，回退下载路径不变。

**Host 匹配已有 Harness 主目录，而不是始终使用 `dsh-home/`。** 选择顺序为：已包含 Harness 数据的 `$DSH_HOME`，然后是 `~/.dsh`，最后才是隔离的应用数据主目录。目录中存在 `sessions`、`.credentials.yaml`、`.env`、`profiles` 或 `settings.yaml` / `.yml` / `.json` 时，即视为 Harness 主目录。其他已发现主目录中缺失的文件和会话目录会复制进选定主目录；已有文件保持不动。`desktop-overlay` 从不导入，因为外壳会重新生成它。

## 曾考虑的替代方案

**继续使用隔离的 `dsh-home/`，把 CLI 数据复制进去。** 不采用：之后的 CLI 或 `dsh web` 启动看不到桌面端会话，密钥也会在两个主目录之间分叉。

**即使 `~/.dsh` 为空，也始终把 `DSH_HOME` 设为该目录。** 不采用：仅使用桌面端的新用户会在未选择共享目录的情况下，把数据写进默认 CLI 主目录。

**使用 `PATH` 上任意 Node，不做引擎版本检查。** 不采用：Node 18/20 以及低于 22.19 的 Node 22 不满足 workspace engines 范围，会导致 Host 启动失败。

**按键合并凭据 YAML 文档。** 不采用：结构化合并可能损坏受管存储。仅在目标缺失时复制文件：两边都有 `.credentials.yaml` 时保留 CLI 密钥；选定主目录没有该文件时才复制桌面端文件。

## 后果

本机已有兼容 Node 时跳过运行时压缩包下载；捆绑源码树还没有 `node_modules` 时，仍会执行 `pnpm install`。`~/.dsh` 已存在时，桌面端与 CLI 共用一个主目录，包括会话和密钥。overlay 插件写入选定主目录。主机 Node 后来消失时，下一次启动的扫描会失败并回退到私有运行时。已选定的二进制随后由[桌面端 PATH 桥接](2026-08-14-desktop-path-bridge.md)暴露到 PATH。
