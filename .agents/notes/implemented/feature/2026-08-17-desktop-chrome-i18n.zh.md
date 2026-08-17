# Agent Note: 桌面原生 chrome i18n

Status: implemented

[English](2026-08-17-desktop-chrome-i18n.md) | 中文

## Problem

Tauri 外壳的启动页、托盘、关闭对话框、系统通知和启动状态行此前只有中文。嵌入的 `dsh web` 客户端已有 Settings 语言（`@deepseek-ai/dsh-client-locale`），因此英语操作系统用户会看到中文外壳包着一套英语（或另行选择）的 Web UI。启动页字标还把 `Deep` 和 `Seek` 拆成两种字重，读起来像两个词。

## Decision

**原生 chrome 跟随操作系统 UI 语言，与 NSIS 安装器一致。** `zh*` 用中文，其余 tag 用英语。托盘和关闭对话框不另做语言选择器。嵌入的 `dsh web` 客户端继续使用自己的 Settings 语言；改它不会改写托盘或启动页文案。Rust 通过 `sys-locale` 在 `apps/desktop-tauri/src-tauri/src/i18n.rs` 查字符串；`splash.html` 和 `shell.html` 使用 `desktop-i18n.js`。外壳注入 `window.__DSH_LOCALE__`，使 HTML 字典与 Rust 进程 locale 一致。单元测试固定为中文，以便现有启动页断言在英语 CI 主机上保持稳定。

**启动页字标是一个词。** `DeepSeek` 以单一字重（`600`）和较紧的字距渲染；产品名不是 `Deepseek`。

完整的启动页句子、托盘项、关闭对话框、更新器 toast、WSL 选择失败，以及拒绝 Windows `node.exe` 的文案已翻译。路径和 IO 错误前缀（`无法读取 …`）维持原样。

## Alternatives considered

**让原生 chrome 跟随 Web 客户端的 Settings 语言。** 否决：启动页和托盘在 `dsh web` 加载之前就存在；接到 Settings 会多一条持久化路径，并在首屏产生竞态。

**继续只提供中文 chrome。** 否决：NSIS 安装器已经跟随操作系统 locale，英语主机不应只能读中文启动失败。

**在托盘增加语言项。** 否决：会与 Settings 和安装器策略重复；原生 chrome 没有需要第三套开关的独立受众。

## Consequences

英语 Windows 安装显示英语启动页、托盘和关闭对话框；中文操作系统显示中文。更改操作系统语言在下一次桌面进程生效。Web Settings 语言保持独立。英语 CI 仍行使中文启动页字符串，因为测试固定该 locale。剩余 IO 错误前缀在后续补齐前仍为中文。
