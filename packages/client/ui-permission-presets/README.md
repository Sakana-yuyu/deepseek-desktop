# @deepseek-ai/dsh-client-ui-permission-presets

English | [中文](README.zh.md)

Permission browser surfaces for two different lifetimes. The General-settings row reads the explicitly exposed `permission` Settings descriptor, derives its options from the host's dynamic `defaultPreset` enum, and writes one `settings.mutate` path operation with the descriptor revision. Its observable rides the slot system's `hooks` compartment, so the renderer owns React hook binding; a push invalidation refetches the descriptor. The value applies to fresh sessions; when Web later confirms an existing Workspace blank as the New Session reuse target, the host refreshes a still-default-derived permission after adopting that exact live or cold session. Choosing the full-access preset requires an explicit risk acknowledgement before the row writes it.

The current-session surface remains a popupSelect DECORATION hung on the host `/permission` command (`ctx.commandUi.decorate`). A decoration is not a second command — the host command keeps its slash-menu row, the argued path (`/permission <preset>` switches directly), and the durable lifecycle logging; the decoration replaces only the bare invocation with the picker: one flat preset list with the current value marked active, canonical built-in names rendered as localized product labels, explicit host labels preserved, and unknown kebab-case preset names rendered in title case. A pick submits the `/permission <preset>` command line. Options and the active mark read the session's `permissions` projection (the same host-computed select the composer chip renders), so both current-session surfaces share one read source and one write path, and the pushed projection frame is the single confirmation both follow. The decoration is available exactly while the projection key is present; a permission-less composition shows neither picker nor Settings row.

The `/client` exports are the plugin body (`apply`/`inject`).

## Model Experience

Indirectly, through the permission facts written by its two surfaces: the Settings row causes a future session to start with whole-value knob events (`permission/preset`, `sandbox/mode`, `approval/policy`), while the `/permission` picker appends the same facts when it switches the current session; those events select the sandbox mode and approval policy later tool calls resolve, and picker interaction adds no prompt content.

#### KV Cache effect

No direct invalidation; the knob consumers own any request-prefix changes.

## Known Limitations and Deferred Work

- **The Settings row is Web-only** — non-Web clients may still switch the current session through `/permission`, but do not receive this browser contribution.
- **Preset descriptions come from the host** — localized built-in labels may therefore appear beside a description written in another language.
