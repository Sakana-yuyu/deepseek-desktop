# Agent Note: Desktop native chrome i18n

Status: implemented

English | [中文](2026-08-17-desktop-chrome-i18n.zh.md)

## Problem

The Tauri shell's splash, tray, close dialog, toasts, and boot status lines were Chinese-only. The embedded `dsh web` client already has a Settings language (`@deepseek-ai/dsh-client-locale`), so English OS users saw Chinese chrome around an English (or independently chosen) web UI. The splash wordmark also split `Deep` and `Seek` into two font weights, which reads as two words.

## Decision

**Native chrome follows the OS UI language, matching the NSIS installer.** `zh*` is Chinese; every other tag is English. There is no second language picker in the tray or close dialog. The embedded `dsh web` client keeps its own Settings language; changing it does not rewrite tray or splash copy. Rust looks up strings in `apps/desktop-tauri/src-tauri/src/i18n.rs` through `sys-locale`; `splash.html` and `shell.html` use `desktop-i18n.js`. The shell injects `window.__DSH_LOCALE__` so the HTML dictionaries match the Rust process locale. Unit tests pin Chinese so existing splash assertions stay stable on English CI hosts.

**The splash wordmark is one word.** `DeepSeek` renders at a single weight (`600`) with tight tracking; the product name is not `Deepseek`.

Complete splash sentences, tray items, close-dialog copy, updater toasts, WSL selection failures, and the Windows-`node.exe` refusal are translated. Path and IO error prefixes (`无法读取 …`) stay as they are.

## Alternatives considered

**Follow the web client's Settings language for native chrome.** Rejected because splash and tray exist before `dsh web` loads, and wiring them to Settings would add a second persistence path and a race on first paint.

**Keep Chinese-only chrome.** Rejected: the NSIS installer already follows the OS locale, and English hosts should not require reading Chinese boot failures.

**Add a tray language item.** Rejected because it duplicates Settings and the installer policy; native chrome has no independent audience that needs a third switch.

## Consequences

An English Windows install shows English splash, tray, and close dialog; a Chinese OS shows Chinese. Changing the OS language takes effect on the next desktop process. Web Settings language remains independent. English CI still exercises the Chinese splash strings because tests pin that locale. Remaining IO error prefixes are still Chinese until a later pass.
