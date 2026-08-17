//! Recoverable Windows IO failures must not abort desktop boot.

/// True when an IO error is access-denied, missing-path, or file-in-use.
pub fn is_recoverable_io(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("os error 5")
        || lower.contains("os error 3")
        || lower.contains("os error 32")
        || lower.contains("拒绝访问")
        || lower.contains("access is denied")
        || lower.contains("系统找不到指定的路径")
        || lower.contains("cannot find the path")
        || lower.contains("cannot find the file")
        || lower.contains("the system cannot find")
        || lower.contains("另一个程序正在使用")
        || lower.contains("being used by another process")
        || lower.contains("process cannot access")
}

/// Format a recoverable failure that names the path and the operation.
pub fn recoverable_message(
    operation: &str,
    path: &std::path::Path,
    error: impl std::fmt::Display,
) -> String {
    format!("{operation} {}: {error}", path.display())
}

#[cfg(test)]
mod tests {
    use super::is_recoverable_io;

    #[test]
    fn treats_windows_access_missing_and_busy_as_recoverable() {
        assert!(is_recoverable_io("拒绝访问。 (os error 5)"));
        assert!(is_recoverable_io("系统找不到指定的路径。 (os error 3)"));
        assert!(is_recoverable_io(
            "另一个程序正在使用此文件，进程无法访问。 (os error 32)"
        ));
        assert!(is_recoverable_io("Access is denied. (os error 5)"));
        assert!(!is_recoverable_io("harness CLI 缺失"));
    }
}
