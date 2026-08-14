# Agent Note: 跨平台桌面源码预配

Status: implemented

[English](2026-08-14-cross-platform-desktop-source-provisioning.md) | 中文

## 问题

桌面外壳需要为没有 Harness 开发环境的用户提供体积较小的安装包。携带完整 workspace 依赖树会让安装包过大且组装缓慢，而依赖系统 Node 又会让启动受制于未受管理的主机工具。发布流程还需要为每个受支持的操作系统和架构生成可独立识别的产物，并避免发布不完整的平台集合。

## 决策

**桌面安装包携带裁剪后的源码树和已构建应用产物，但不携带 `node_modules`。** 首次启动把该只读资源复制到应用数据目录，先扫描本机是否已有满足 `^22.19 || >=24` 的 Node 和可用的 pnpm，只在扫描失败时下载对应平台的 Node 压缩包，只在需要时通过已选定的 Node 安装 pnpm，再在移除 `CI` 的环境中执行 `pnpm install --prod --no-frozen-lockfile`。镜像端点仍可通过 `DSH_NODE_MIRROR` 和 `DSH_NPM_REGISTRY` 配置。主机匹配与 Harness 主目录采用由[桌面端主机工具链扫描与主目录匹配](2026-08-14-desktop-host-env-and-home-adoption.md)负责。

**每个源码包使用隔离的可写目录。** 内容哈希选择 `harness-versions/<bundle-hash>`，因此更新不会删除旧 Host 正在占用的文件。兼容的 Node 和 pnpm 运行时保持共享，并在源码更新之间复用。原生外壳只允许一个应用实例，再次启动时会聚焦已有窗口。

**已选定的 Node 负责执行所有预配命令。** Windows x64 和 x86 使用官方 zip 布局；macOS x64/arm64 与 Linux x64/arm64 使用 tar.gz 布局。压缩包条目必须位于预期的带版本 Node 目录之下。tar 解压保留 Unix 权限位，npm 按平台对应的 Node 分发布局解析，私有安装的 pnpm 由已选定的 Node 二进制直接执行其 JavaScript 入口。扫描到主机 pnpm 时则直接调用它。

**一个 tag 发布一套完整桌面矩阵和一份签名更新 manifest。** `desktop-v*` tag 构建 Windows x64/x86 NSIS 安装包、macOS Intel/Apple Silicon DMG，以及 Linux x64 AppImage/deb。每个矩阵任务为其 Tauri 更新产物签名并上传带操作系统和架构标识的文件；下游 release 任务先验证集合完整，再创建或更新一个 GitHub 预发布版本，并替换稳定 `desktop-updater` Release 通道中的 `latest.json`。更新公钥内置于应用，私钥和密码只存在于 Release Secrets 和维护者受保护的备份中。更新签名用于验证下载，但可执行文件仍没有操作系统代码签名，也未经过 notarization。

**Release 构建在预配前检查并安装更新。** 官方 Tauri updater 验证稳定 manifest 和产物签名，安装语义版本更高的版本并重启应用。更新检查或下载失败会写入日志，应用继续使用已安装版本启动。开发构建跳过网络更新检查。

**Windows 安装会关闭运行中的应用，并使陈旧快捷方式图标失效。** NSIS 预安装钩子在复制文件前结束 `dsh-desktop.exe` 及其子进程树。安装后钩子保留用户对桌面快捷方式的选择，使用文件名含版本的独立 ICO 资源重建已有快捷方式，并通知 Explorer 图标关联已变化。

**所有原生界面使用同一个鱼形标志。** 透明背景黑色 SVG 路径与 `FishLogo.tsx` 共用；生成的 PNG、ICO 和 ICNS 资源用于 Tauri bundle、NSIS 安装器与卸载器、配置声明的启动窗口、运行时创建的主窗口和 Windows 快捷方式刷新。

## 曾考虑的替代方案

**携带完整离线依赖树。** 不采用：workspace 依赖闭包会产生很大的安装包和昂贵的文件系统操作。裁剪后的源码包既能保持发布产物较小，也能保留准确的已构建 Harness 应用。

**完全依赖主机工具链、不保留私有回退。** 不采用：全新 Windows、macOS 和 Linux 系统常常没有 Node，或版本不在 `^22.19 || >=24` 范围内。兼容的主机工具链会先被扫描；私有运行时仍是回退。见[桌面端主机工具链扫描与主目录匹配](2026-08-14-desktop-host-env-and-home-adoption.md)。

**让每个矩阵任务分别发布 Release 资产。** 不采用：并发创建 Release 会产生竞态，并可能在其他平台仍在构建时暴露不完整版本。最终任务只在所有必需产物存在后发布。

**只发布已在本地验证的 Windows x64 安装包。** 不采用：桌面发布约定包含 Windows x86、两种 macOS 架构和 Linux x64，运行时压缩包处理也必须匹配这些二进制。

**让已安装应用指向各版本专属的预发布 Release。** 不采用：某一版本内置的端点无法发现下一个 tag，而 GitHub 的最新 Release 重定向会排除预发布版本。固定 Release 通道让所有已安装版本使用同一个持久 manifest URL，同时由版本化 Release 继续持有下载产物。

**自行实现更新下载和签名验证。** 不采用：Tauri updater 已定义平台产物格式、语义版本比较、强制签名验证、Windows 安装器交接和应用清理。自定义 updater 会重复实现安全敏感行为。

## 后果

安装包保持紧凑，但本机没有兼容 Node、或捆绑源码树还没有 `node_modules` 时，首次启动仍需要网络连接，并可能持续数分钟。运行时文件和依赖占用应用数据目录，而不是安装目录。源码更新期间，旧 Host 仍占用文件时可能暂时保留旧的 bundle 专属目录；后续清理可以移除不活动目录，而不阻塞启动。自动更新要求更高的语义版本，因此替换同一 tag 下的资产只能为手动重装一次该版本的用户启用后续更新。丢失更新私钥或密码会使已安装客户端无法继续信任后续更新。发布工作流需要为完整平台矩阵投入构建时间，并在缺少任一必需包或签名时阻止发布。手动 Windows 安装会强制关闭活动中的应用工作，操作系统安全策略要求时，用户还必须明确批准二进制文件。
