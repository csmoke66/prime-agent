//! Shell selection for the bash tool and the kernel's `bash()`. Unix:
//! explicit path, `/bin/bash`, `which bash`, `sh`. Windows: Git Bash from
//! the canonical install dirs, then `where bash.exe` (System32 candidates
//! demoted to last — the WSL launcher), never PATH for the kernel shell.

use std::path::Path;

/// Shell program plus the fixed argument list used to run a command string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellConfig {
    pub shell: String,
    pub args: Vec<String>,
}

/// One predicate for every explicit `shellPath` consumer (the user-bash
/// [`get_shell_config`] and the kernel [`resolve_kernel_bash_shell`]):
/// the same setting fails the same way everywhere, naming the setting.
///
/// # Errors
///
/// Error when the path does not exist or (on unix) is not an executable
/// regular file.
fn check_custom_shell_path(path: &str) -> anyhow::Result<()> {
    if !Path::new(path).exists() {
        anyhow::bail!("Custom shell path not found: {path} (shellPath setting)");
    }
    {
        use std::os::unix::fs::PermissionsExt;
        let executable = std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0);
        if !executable {
            anyhow::bail!("Custom shell path is not executable: {path} (shellPath setting)");
        }
    }
    Ok(())
}

/// Resolve the shell to run commands with, honoring an explicit custom path.
///
/// # Errors
///
/// Error when the explicit custom shell path fails [`check_custom_shell_path`];
/// the built-ins never fail.
pub fn get_shell_config(custom_shell_path: Option<&str>) -> anyhow::Result<ShellConfig> {
    if let Some(path) = custom_shell_path {
        check_custom_shell_path(path)?;
        return Ok(ShellConfig {
            shell: path.to_string(),
            args: vec!["-c".to_string()],
        });
    }

    if Path::new("/bin/bash").exists() {
        return Ok(ShellConfig {
            shell: "/bin/bash".to_string(),
            args: vec!["-c".to_string()],
        });
    }

    if let Some(bash) = find_bash_on_path() {
        return Ok(ShellConfig {
            shell: bash,
            args: vec!["-c".to_string()],
        });
    }

    Ok(ShellConfig {
        shell: "sh".to_string(),
        args: vec!["-c".to_string()],
    })
}



fn find_bash_on_path() -> Option<String> {
    let out = std::process::Command::new("which")
        .arg("bash")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.trim().lines().next()?;
    (!first.is_empty()).then(|| first.to_string())
}





/// Absolute default shell for the kernel's `bash()` when no `shellPath` is
/// configured: POSIX uses `/bin/bash` else `/bin/sh`. `None` when no shell
/// resolves (kernel startup must not fail; `bash()` raises its teaching error).
#[must_use]
pub fn default_kernel_bash_shell() -> Option<String> {
    if Path::new("/bin/bash").exists() {
        Some("/bin/bash".to_string())
    } else {
        Some("/bin/sh".to_string())
    }
}



/// Resolve the absolute shell for the kernel's `bash()` from an explicit
/// `shellPath`, validated here so a bad setting fails session creation
/// once instead of every kernel `bash()` call: a trimmed non-empty
/// explicit path must be absolute and pass [`check_custom_shell_path`];
/// anything else falls back to [`default_kernel_bash_shell`].
///
/// # Errors
///
/// Error naming the path and the `shellPath` setting when the explicit
/// path is relative, missing, or (on unix) not executable. The default
/// never fails.
pub fn resolve_kernel_bash_shell(custom: Option<&str>) -> anyhow::Result<Option<String>> {
    if let Some(path) = custom.map(str::trim).filter(|s| !s.is_empty()) {
        if !Path::new(path).is_absolute() {
            anyhow::bail!("Custom shell path must be absolute: {path} (shellPath setting)");
        }
        check_custom_shell_path(path)?;
        return Ok(Some(path.to_string()));
    }
    Ok(default_kernel_bash_shell())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A rejected explicit shell: the error names the path and the
    /// `shellPath` setting, whatever the validation failure was.
    #[test]
    fn explicit_shell_paths_are_validated() {
        for path in ["relative/bash", "/no/such/shell"] {
            let message = resolve_kernel_bash_shell(Some(path))
                .unwrap_err()
                .to_string();
            assert!(message.contains("shellPath"), "{path}: {message}");
            assert!(message.contains(path), "{path}: {message}");
        }
        let dir = tempfile::tempdir().unwrap();
        let not_executable = dir.path().join("shell");
        std::fs::write(&not_executable, b"").unwrap();
        let path = not_executable.to_string_lossy().to_string();
        let message = resolve_kernel_bash_shell(Some(path.as_str()))
            .unwrap_err()
            .to_string();
        assert!(message.contains("shellPath"), "not executable: {message}");
        assert!(message.contains(&path), "not executable: {message}");
    }

    #[test]
    fn padded_executable_shell_resolves_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let shell = dir.path().join("bash");
        std::fs::write(&shell, b"").unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        let padded = format!("  {}  ", shell.to_string_lossy());
        assert_eq!(
            resolve_kernel_bash_shell(Some(padded.as_str())).unwrap(),
            Some(shell.to_string_lossy().to_string())
        );
    }
}
