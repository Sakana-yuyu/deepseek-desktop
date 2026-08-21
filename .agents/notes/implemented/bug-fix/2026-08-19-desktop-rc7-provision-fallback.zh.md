# Agent Note: 桌面 rc.7 预配失败与不可启动的回退

Status: implemented

[English](2026-08-19-desktop-rc7-provision-fallback.md) | 中文

## Problem

安装包内的 `pnpm-workspace.yaml` 由硬编码模板生成，其 `patchedDependencies` 仍声明 `node-pty@1.1.0`。上游 `0.1.0-rc.7` 将补丁移至 `node-pty@1.2.0-beta.15`，而 pnpm 把"声明但未使用的补丁"当作安装硬错误，因此 rc.7-0.1 更新后的每次首启 `pnpm install --prod --no-frozen-lockfile` 都以 `ERR_PNPM_UNUSED_PATCH` 失败。回退逻辑随后把失败变成永久性瘫痪：`find_existing_harness` 只要求树里存在预构建的 CLI 入口，并按哈希名排序候选，于是选中了刚播种、无依赖的新树而不是此前可用的 rc.5 树；Host 以 `ERR_MODULE_NOT_FOUND` 退出，应用无法启动。安装与下载步骤也没有任何期限，注册源或子进程卡住时启动页会无限停留。

## Decision

打包脚本改为从仓库的 `pnpm-workspace.yaml` 派生安装包内的 workspace 文件，只替换 `packages:` 成员，`patchedDependencies`、`allowBuilds` 及其余段落原样复制，声明因此始终与实际交付的源码树一致。一棵 harness 树只有在 `apps/cli/lib/bin.js` 与 `node_modules/.pnpm` 同时存在时才算可启动；bundle 哈希回退候选与 `find_existing_harness` 只接受可启动的树，并按修改时间从新到旧排序。`pnpm install`、pnpm 自安装与 Node 归档下载分别有 20、10、15 分钟的期限；到期即让该步骤失败并落入既有回退路径，而不是让启动页停摆。预配成功后只保留最新的三棵 `harness-versions` 树，更旧的树按目录容忍失败地删除。

本记录拥有安装包 workspace 派生、回退有效性、步骤期限与旧树清理。预配模型与更新流程仍由[跨平台桌面源码预配](../feature/2026-08-14-cross-platform-desktop-source-provisioning.zh.md)拥有。

## Alternatives considered

**把模板固定为 rc.7 的补丁集。** 否决：下一次上游补丁升级会复现同样的硬失败；从仓库文件派生消除了可能漂移的副本。

**打包时剔除未使用的补丁条目。** 否决：需要在打包脚本里计算依赖图；逐字复制声明既精确又是一遍完成。

**继续接受任何含 CLI 入口的树并在其上重试 `pnpm install`。** 否决：播种出的树必然含有预构建 CLI，每次启动都会在同一棵坏树上重试安装；已安装的依赖库才是区分"可启动树"与"播种树"的判据。

**把卡住的安装当作仍在推进。** 否决：启动页没有取消路径；到期转入回退可以复用最后一个能运行的树。

**立即清理所有被取代的树。** 否决：更新期间更旧的运行中 Host 可能仍持有上一棵树的文件；保留三棵覆盖当前树、回退树与一棵余量。

## Consequences

预配失败的桌面更新现在会启动最近一棵真正运行过的树，而不是瘫痪启动。启动期安装与下载不再可能无限挂起。预配成功后磁盘占用不再随每次发布增长；被旧进程持有的树可再存活一个更新周期。仓库的 workspace 文件一变，安装包内的 workspace 文件随之改变，因此即便交付源码未动，上游 workspace 编辑也会改变 bundle 哈希。桌面 crate 测试固定了可启动树判定、回退排序与清理；打包脚本测试固定 workspace 派生，发布前还会对重新生成的 bundle 验证与首启完全相同的安装命令。
