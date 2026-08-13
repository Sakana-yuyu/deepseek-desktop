# Agent Note: 跨平台桌面源码预配

Status: implemented

[English](2026-08-14-cross-platform-desktop-source-provisioning.md) | 中文

## 问题

桌面外壳需要为没有 Harness 开发环境的用户提供体积较小的安装包。携带完整 workspace 依赖树会让安装包过大且组装缓慢，而依赖系统 Node 又会让启动受制于未受管理的主机工具。发布流程还需要为每个受支持的操作系统和架构生成可独立识别的产物，并避免发布不完整的平台集合。

## 决策

**桌面安装包携带裁剪后的源码树和已构建应用产物，但不携带 `node_modules`。** 首次启动把该只读资源复制到应用数据目录，下载与编译目标操作系统和架构匹配的 Node 压缩包，通过该 Node 运行时安装 pnpm，再在移除 `CI` 的环境中执行 `pnpm install --prod --no-frozen-lockfile`。镜像端点仍可通过 `DSH_NODE_MIRROR` 和 `DSH_NPM_REGISTRY` 配置。

**下载的运行时负责执行所有预配命令。** Windows x64 和 x86 使用官方 zip 布局；macOS x64/arm64 与 Linux x64/arm64 使用 tar.gz 布局。压缩包条目必须位于预期的带版本 Node 目录之下。tar 解压保留 Unix 权限位，npm 按平台对应的 Node 分发布局解析，pnpm 则由下载的 Node 二进制直接执行其 JavaScript 入口，不依赖 shebang 或主机 `PATH`。

**一个 tag 发布一套完整桌面矩阵。** `desktop-v*` tag 构建 Windows x64/x86 NSIS 安装包、macOS Intel/Apple Silicon DMG，以及 Linux x64 AppImage/deb。每个矩阵任务上传带操作系统和架构标识的产物；下游 release 任务先验证集合完整，再创建或更新一个 GitHub 预发布版本。这些预发布产物未签名，也未经过 notarization。

**所有原生界面使用同一个鱼形标志。** 透明背景黑色 SVG 路径与 `FishLogo.tsx` 共用；生成的 PNG、ICO 和 ICNS 资源用于 Tauri bundle、NSIS 安装器与卸载器、配置声明的启动窗口，以及运行时创建的主窗口。

## 曾考虑的替代方案

**携带完整离线依赖树。** 不采用：workspace 依赖闭包会产生很大的安装包和昂贵的文件系统操作。裁剪后的源码包既能保持发布产物较小，也能保留准确的已构建 Harness 应用。

**使用主机上的 Node、npm 或 pnpm。** 不采用：全新 Windows、macOS 和 Linux 系统上的版本、安装路径与可用性各不相同。私有运行时让首次启动只使用一套受控工具链。

**让每个矩阵任务分别发布 Release 资产。** 不采用：并发创建 Release 会产生竞态，并可能在其他平台仍在构建时暴露不完整版本。最终任务只在所有必需产物存在后发布。

**只发布已在本地验证的 Windows x64 安装包。** 不采用：桌面发布约定包含 Windows x86、两种 macOS 架构和 Linux x64，运行时压缩包处理也必须匹配这些二进制。

## 后果

安装包保持紧凑，但首次启动需要网络连接，并可能在安装依赖时持续数分钟。运行时文件和依赖占用应用数据目录，而不是安装目录。发布工作流需要为完整平台矩阵投入构建时间，并在缺少任一必需包时阻止发布。操作系统安全策略要求时，用户必须明确批准这些未签名的预发布二进制。
