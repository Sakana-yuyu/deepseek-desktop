# @deepseek-ai/dsh-permission-presets

[English](README.md) | 中文

通过 `ctx.permissionPresets`（[`PermissionPresetService`](src/index.ts)）提供面向用户的权限预设。每个配置名称都会将 `sandbox/mode` 与 `approval/policy` 组成一组；默认项为 `workspace-write`（`workspace-write` + `ask`）和 `danger-full-access`（`danger-full-access` + `never`）。UI 适配器可以将该表作为单个选择器公开，而沙箱执行与审批仍分别消费各自的调节项。

`set(session, name)` 会先在仅写日志的 `permission/preset` 事件中记录已变更的选择，再仅对实际值发生变化的调节项调用 setter。选择事件先于调节项事件，并在多个预设共享同一组取值时保留用户意图；净变化为零的选择不会追加任何内容。`current(events)` 优先返回仍与当前调节项匹配的已记录选择，其次返回表中第一个匹配项，否则返回 `custom`。客户端可以把 `custom` 显示为当前值，但不能选择它。

该服务拥有 `permission` Settings namespace。其 `defaultPreset` 会初始化新建会话，以及 Web 明确确认为新会话复用目标、且权限来自默认值的 Workspace 空白会话：组合项使用 `Config.defaultPreset`；省略时，则推断与组合后的沙箱和审批默认值匹配的 preset。创建会话时会读取当前设置，并固定 `permission/preset`、`sandbox/mode` 和 `approval/policy`；preset 事实还会记录它来自默认值、显式选择还是旧旋钮推断。已提交的设置变更不会扫描或改写现有会话。当 Web workspace runtime 选中 cwd 匹配、属于该 Workspace 且未归档的空白会话时，host 会重新检查这些事实，并接纳这个确切会话（包括冷存储中的持久会话），而且只在它尚未开始轮次、最近选择来自默认值且有效旋钮仍匹配该选择时推进默认值。显式选择、由旧旋钮推断或没有来源标记的旧选择、独立变更的旋钮，以及普通 seed 恢复都会继续固定原权限。挂载服务时还会遍历所有已存活会话，因此 HMR（热模块替换）会固定插件缺席期间创建的所有会话。

该服务要求存在具有约束能力的 `ctx.shell` 执行器和 `ctx.approval`。表中名为 `custom` 的条目会在加载时抛出异常。当组合默认值与任何 preset 都不匹配时，插件要求显式配置 `defaultPreset`；独立构造的零事件会话仍可能推导出 `custom`。详见[沙箱切换设计](../../../.agents/notes/implemented/feature/2026-07-06-sandbox.zh.md)。

两个可选子功能在同一服务之上提供产品界面：`permissions` 会话投影单元（`src/types.ts` 声明该 key；单元以组合默认值为基础折叠三个全量值可调参数事件，并生成选择器视图，其中包含表内选项和仅作当前值的 `custom`）与 `/permission` 命令（不带参数调用时报告当前预设与表；预设参数经 `set` 切换）。每个子功能仅在其注册表（`ctx.sessionProjections` / `ctx.commands`）被组合时激活。

## 模型体验

间接地，通过 `dsh-user-approval` 和 `dsh-tool-bash`：二者会渲染由此服务的可调参数事件所选择的审批策略提示词、切换通知和沙箱工具结果；`permission/preset` 本身只写入日志。

#### KV Cache 影响

不会直接使缓存失效；具名消费方拥有所有请求前缀变更。

## 已知限制与暂缓事项

- **只组合两个机制级可调参数**：预设选择沙箱模式和审批策略；agent（智能体）／profile 选择尚未纳入 `PresetSpec`。
- **`custom` 只能推导得出**：调用方可以从不匹配的调节项组合切换出去，但无法通过此服务选中或持久化一个名为 custom 的预设。
- **预设表是进程级配置**：配置在插件生命周期内固定；更改可用预设必须重新加载插件。
- **已存储的默认值必须保留在 preset 表中**：移除被引用的 preset 会导致权限设置注册失败，直到更新或重置 `settings.yaml` 中的 `permission` 分节。
