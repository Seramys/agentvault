use crate::error::Result;
use crate::util::{ensure_dir, home_dir};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootSource {
    Flag,
    Env,
    Default,
}

impl RootSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Flag => "--home",
            Self::Env => "AGENTVAULT_HOME",
            Self::Default => "platform default",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Root {
    pub dir: PathBuf,
    pub source: RootSource,
}

impl Root {
    pub fn discover() -> Result<Self> {
        if let Ok(custom) = std::env::var("AGENTVAULT_HOME") {
            if !custom.trim().is_empty() {
                return Ok(Self {
                    dir: absolutize(crate::util::expand_path(custom.trim())),
                    source: RootSource::Env,
                });
            }
        }
        let dir = default_data_dir();
        Ok(Self {
            dir,
            source: RootSource::Default,
        })
    }

    pub fn new<P: AsRef<Path>>(dir: P) -> Self {
        Self {
            dir: absolutize(dir.as_ref().to_path_buf()),
            source: RootSource::Flag,
        }
    }

    pub fn ensure(&self) -> Result<()> {
        ensure_dir(&self.dir)
    }

    pub fn config(&self) -> PathBuf {
        self.dir.join("config.json")
    }

    pub fn audit_log(&self) -> PathBuf {
        self.dir.join("audit.jsonl")
    }

    pub fn revoked(&self) -> PathBuf {
        self.dir.join("revoked.json")
    }

    pub fn honeypots(&self) -> PathBuf {
        self.dir.join("honeypots.json")
    }

    pub fn broker_port(&self) -> PathBuf {
        self.dir.join("broker.port")
    }

    pub fn admin_token(&self) -> PathBuf {
        self.dir.join("admin.token")
    }

    pub fn tickets(&self) -> PathBuf {
        self.dir.join("tickets.json")
    }

    pub fn pid(&self) -> PathBuf {
        self.dir.join("broker.pid")
    }
}
fn absolutize(dir: PathBuf) -> PathBuf {
    if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&dir))
            .unwrap_or(dir)
    }
}

pub fn default_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(base) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(base).join("agentvault");
        }
        if let Some(home) = home_dir() {
            return home.join("AppData").join("Local").join("agentvault");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = home_dir() {
            return home
                .join("Library")
                .join("Application Support")
                .join("agentvault");
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(base) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(base).join("agentvault");
        }
        if let Some(home) = home_dir() {
            return home.join(".local").join("share").join("agentvault");
        }
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agentvault")
}
