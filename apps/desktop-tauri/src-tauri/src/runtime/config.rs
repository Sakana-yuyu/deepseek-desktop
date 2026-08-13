//! Mirror URLs and harness version for first-run provisioning.

/// Default Node LTS aligned with repo engines (^22.19).
pub const DEFAULT_NODE_VERSION: &str = "22.19.0";

/// pnpm version aligned with root packageManager.
pub const DEFAULT_PNPM_VERSION: &str = "11.7.0";

/// Host port range start for `dsh web`.
pub const DEFAULT_WEB_PORT: u16 = 17_890;

/// Bundled harness resource directory name inside Tauri resources.
pub const BUNDLED_HARNESS_DIR: &str = "harness-source";

/// Parent for bundle-specific writable harness trees under app data.
pub const HARNESS_VERSIONS_DIR: &str = "harness-versions";

/// China-friendly Node mirror (override with `DSH_NODE_MIRROR`).
pub fn node_mirror_base() -> String {
    std::env::var("DSH_NODE_MIRROR").unwrap_or_else(|_| "https://npmmirror.com/mirrors/node".into())
}

/// npm/pnpm registry (override with `DSH_NPM_REGISTRY`).
pub fn npm_registry() -> String {
    std::env::var("DSH_NPM_REGISTRY").unwrap_or_else(|_| "https://registry.npmmirror.com".into())
}

/// When set to `local`, use monorepo checkout instead of bundled tree.
pub fn dev_launch_mode() -> Option<String> {
    std::env::var("DSH_DESKTOP_LAUNCH").ok()
}
