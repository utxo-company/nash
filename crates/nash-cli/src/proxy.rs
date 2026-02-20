use std::path::PathBuf;

use miette::{IntoDiagnostic, Result};

/// Walk up from cwd looking for `nash.jsonc`. If the project requires a different
/// compiler version, exec the correct binary (downloading it first if needed).
pub async fn maybe_proxy() -> Result<()> {
    let cwd = std::env::current_dir().into_diagnostic()?;

    let config_path = match find_config(&cwd) {
        Some(p) => p,
        None => return Ok(()),
    };

    let config = nash_config::parse_file(&config_path).into_diagnostic()?;

    let required = match config.compiler() {
        Some(v) if v != crate::VERSION => v,
        _ => return Ok(()),
    };

    let cache_dir = cached_binary_dir(required);
    let binary = cached_binary_path(required);

    if binary.exists() {
        exec_cached_binary(&binary);
    }

    crate::download::download_version(required, &cache_dir).await?;
    exec_cached_binary(&binary);
}

/// Search for `nash.jsonc` by walking up from `start`.
fn find_config(start: &std::path::Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join(nash_config::CONFIG_FILE_NAME);
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn cached_binary_dir(version: &str) -> PathBuf {
    let base = std::env::var("NASH_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("nash")
        });
    base.join("versions").join(version)
}

fn cached_binary_path(version: &str) -> PathBuf {
    let dir = cached_binary_dir(version);
    if cfg!(windows) {
        dir.join("nash.exe")
    } else {
        dir.join("nash")
    }
}

/// Replace this process with the cached binary, forwarding all args.
fn exec_cached_binary(binary: &std::path::Path) -> ! {
    let args: Vec<String> = std::env::args().skip(1).collect();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(binary).args(&args).exec();
        eprintln!("Failed to exec cached nash binary: {err}");
        std::process::exit(1);
    }

    #[cfg(not(unix))]
    {
        let status = std::process::Command::new(binary)
            .args(&args)
            .status()
            .unwrap_or_else(|e| {
                eprintln!("Failed to run cached nash binary: {e}");
                std::process::exit(1);
            });
        std::process::exit(status.code().unwrap_or(1));
    }
}
