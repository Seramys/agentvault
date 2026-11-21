use agentvault_core::{Result, Root, RootSource};
use std::path::{Path, PathBuf};

pub fn launch_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

pub fn show(root: &Root) -> Result<()> {
    println!("home:       {}", root.dir.display());
    println!("home from:  {}", root.source.as_str());
    println!(
        "default:    {}",
        agentvault_core::default_data_dir().display()
    );
    println!(
        "exe:        {}",
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "-".into())
    );
    #[cfg(windows)]
    {
        for scope in ["User", "Machine"] {
            println!(
                "persisted:  {} ({})",
                read_var(scope, "AGENTVAULT_HOME")
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "not set".into()),
                scope.to_ascii_lowercase()
            );
            if let Some(dir) = launch_dir() {
                let entries = read_var(scope, "Path").unwrap_or_default();
                println!(
                    "path:       {} ({})",
                    if path_list_contains(&entries, &dir, ';') {
                        "present"
                    } else {
                        "missing"
                    },
                    scope.to_ascii_lowercase()
                );
            }
        }
    }
    #[cfg(not(windows))]
    {
        let file = environment_file();
        println!(
            "persisted:  {} ({})",
            if file.exists() {
                file.display().to_string()
            } else {
                "not set".into()
            },
            "environment.d"
        );
        if let Some(dir) = launch_dir() {
            let path = std::env::var("PATH").unwrap_or_default();
            println!(
                "path:       {} (current shell)",
                if path_list_contains(&path, &dir, ':') {
                    "present"
                } else {
                    "missing"
                }
            );
        }
    }
    let process_env = std::env::var("AGENTVAULT_HOME").ok();
    println!(
        "env var:    {}",
        process_env.unwrap_or_else(|| "not set in this process".into())
    );
    Ok(())
}

/// Extra one-line diagnostics used by `doctor`; never fails.
pub fn diagnostics(root: &Root) {
    #[cfg(windows)]
    {
        let user = read_var("User", "AGENTVAULT_HOME").unwrap_or_default();
        let machine = read_var("Machine", "AGENTVAULT_HOME").unwrap_or_default();
        if user.trim().is_empty() && machine.trim().is_empty() {
            println!("env:       AGENTVAULT_HOME is not persisted (run `agentvault env install`)");
        } else {
            println!(
                "env:       user={} machine={}",
                if user.trim().is_empty() {
                    "-"
                } else {
                    user.trim()
                },
                if machine.trim().is_empty() {
                    "-"
                } else {
                    machine.trim()
                }
            );
        }
        if let Some(dir) = launch_dir() {
            let user_path = read_var("User", "Path").unwrap_or_default();
            println!(
                "path:      {} (install dir {})",
                if path_list_contains(&user_path, &dir, ';') {
                    "present"
                } else {
                    "missing"
                },
                dir.display()
            );
        }
    }
    #[cfg(not(windows))]
    {
        let file = environment_file();
        if !file.exists() {
            println!("env:       not persisted (run `agentvault env install`)");
        } else if let Some(dir) = launch_dir() {
            let path = std::env::var("PATH").unwrap_or_default();
            println!(
                "env:       {} ({})",
                file.display(),
                if path_list_contains(&path, &dir, ':') {
                    "path present"
                } else {
                    "add install dir to PATH"
                }
            );
        }
    }
    if root.source != RootSource::Default {
        println!(
            "note:      this run used {}; persisted values affect new terminals only",
            root.source.as_str()
        );
    }
}

pub fn install(root: &Root, scope: &str, add_path: bool, dry_run: bool) -> Result<()> {
    if scope != "user" && scope != "machine" {
        return Err(agentvault_core::Error::Other(
            "scope must be `user` or `machine`".into(),
        ));
    }
    let home = root.dir.display().to_string();
    #[cfg(windows)]
    {
        let scope_name = if scope == "machine" {
            "Machine"
        } else {
            "User"
        };
        let set_home = format!(
            "[Environment]::SetEnvironmentVariable('AGENTVAULT_HOME',{}, '{}')",
            ps_string(&home),
            scope_name
        );
        let dir = launch_dir();
        if dry_run {
            println!("dry run: would set AGENTVAULT_HOME={home} ({scope})");
            if add_path {
                if let Some(dir) = &dir {
                    println!("dry run: would add {} to {scope} PATH", dir.display());
                }
            }
            return Ok(());
        }
        run_powershell(&set_home)?;
        println!("persisted AGENTVAULT_HOME={home} ({scope})");
        if add_path {
            if let Some(dir) = &dir {
                let script = format!(
                    "$old=[Environment]::GetEnvironmentVariable('Path','{scope_name}'); \
                     if($null -eq $old){{$old=''}}; \
                     $parts=@($old -split ';' | Where-Object {{ $_ -ne '' }}); \
                     $dir={}; \
                     if($parts -notcontains $dir){{ $parts += $dir; \
                       [Environment]::SetEnvironmentVariable('Path',($parts -join ';'),'{scope_name}'); \
                       Write-Output 'added' }} else {{ Write-Output 'present' }}",
                    ps_string(&dir.display().to_string())
                );
                let output = run_powershell(&script)?;
                if output.trim() == "added" {
                    println!("added {} to {scope} PATH", dir.display());
                } else {
                    println!("{} is already on the {scope} PATH", dir.display());
                }
            }
        }
        println!("log out/in (or restart the terminal) for the change to reach existing shells");
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if scope == "machine" {
            return Err(agentvault_core::Error::Other(
                "machine scope requires root; edit /etc/environment manually".into(),
            ));
        }
        let file = environment_file();
        let mut body = format!("AGENTVAULT_HOME={home}\n");
        if add_path {
            if let Some(dir) = launch_dir() {
                body.push_str(&format!("PATH=${{PATH}}:{}\n", dir.display()));
            }
        }
        if dry_run {
            println!("dry run: would write {}\n{body}", file.display());
            return Ok(());
        }
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file, body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644));
        }
        println!(
            "wrote {}; log in again for new sessions to see it",
            file.display()
        );
        Ok(())
    }
}

pub fn uninstall(scope: &str, dry_run: bool) -> Result<()> {
    if scope != "user" && scope != "machine" {
        return Err(agentvault_core::Error::Other(
            "scope must be `user` or `machine`".into(),
        ));
    }
    #[cfg(windows)]
    {
        let scope_name = if scope == "machine" {
            "Machine"
        } else {
            "User"
        };
        let dir = launch_dir();
        if dry_run {
            println!("dry run: would remove AGENTVAULT_HOME ({scope})");
            if let Some(dir) = &dir {
                println!("dry run: would remove {} from {scope} PATH", dir.display());
            }
            return Ok(());
        }
        run_powershell(&format!(
            "[Environment]::SetEnvironmentVariable('AGENTVAULT_HOME',$null,'{scope_name}')"
        ))?;
        println!("removed AGENTVAULT_HOME from {scope} environment");
        if let Some(dir) = &dir {
            let script = format!(
                "$old=[Environment]::GetEnvironmentVariable('Path','{scope_name}'); \
                 if($null -eq $old){{$old=''}}; \
                 $dir={}; \
                 $parts=@($old -split ';' | Where-Object {{ $_ -ne '' -and $_.TrimEnd('\\') -ine $dir.TrimEnd('\\') }}); \
                 [Environment]::SetEnvironmentVariable('Path',($parts -join ';'),'{scope_name}')",
                ps_string(&dir.display().to_string())
            );
            run_powershell(&script)?;
            println!("removed {} from {scope} PATH (if present)", dir.display());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let file = environment_file();
        if dry_run {
            println!("dry run: would remove {}", file.display());
            return Ok(());
        }
        if file.exists() {
            std::fs::remove_file(file)?;
            println!("removed persisted AGENTVAULT_HOME");
        } else {
            println!("nothing to remove");
        }
        Ok(())
    }
}

#[cfg(windows)]
fn read_var(scope: &str, name: &str) -> Option<String> {
    let script = format!(
        "[Environment]::GetEnvironmentVariable({},{})",
        ps_string(name),
        ps_string(scope)
    );
    run_powershell(&script)
        .ok()
        .map(|value| value.trim().to_string())
}

#[cfg(windows)]
fn run_powershell(script: &str) -> Result<String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .map_err(|e| agentvault_core::Error::Other(format!("cannot start powershell: {e}")))?;
    if !output.status.success() {
        return Err(agentvault_core::Error::Other(format!(
            "powershell failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(windows)]
fn ps_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(not(windows))]
fn environment_file() -> PathBuf {
    agentvault_core::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("environment.d")
        .join("agentvault.conf")
}

fn path_list_contains(list: &str, dir: &Path, separator: char) -> bool {
    let target = dir.display().to_string();
    let target = target.trim_end_matches(['/', '\\']);
    list.split(separator).any(|entry| {
        let entry = entry.trim().trim_matches('"').trim_end_matches(['/', '\\']);
        if cfg!(windows) {
            entry.eq_ignore_ascii_case(target)
        } else {
            entry == target
        }
    })
}
