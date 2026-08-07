#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlEndpoint {
    UnixSocket(PathBuf),
    NamedPipe(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub legacy_config_files: Vec<PathBuf>,
    pub cache_dir: PathBuf,
    pub thumbnail_dir: PathBuf,
    pub control_endpoint: ControlEndpoint,
}

impl AppPaths {
    pub fn discover() -> Self {
        #[cfg(target_os = "linux")]
        {
            return Self::discover_linux();
        }
        #[cfg(target_os = "windows")]
        {
            return Self::discover_windows();
        }
        #[allow(unreachable_code)]
        Self::fallback()
    }

    #[cfg(target_os = "linux")]
    fn discover_linux() -> Self {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let config_root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let runtime_root = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
                std::env::temp_dir().join(format!("wreath-{user}"))
            });
        let cache_root = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cache"));
        let config_dir = config_root.join("wreath");
        let cache_dir = cache_root.join("wreath");
        Self {
            config_file: config_dir.join("config.toml"),
            legacy_config_files: ["trace", "riftclip"]
                .map(|name| config_root.join(name).join("config.toml"))
                .into(),
            config_dir,
            thumbnail_dir: cache_dir.join("thumbnails"),
            cache_dir,
            control_endpoint: ControlEndpoint::UnixSocket(runtime_root.join("wreath.sock")),
        }
    }

    #[cfg(target_os = "windows")]
    fn discover_windows() -> Self {
        let profile = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let local_app_data = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| profile.join("AppData").join("Local"));
        let config_dir = local_app_data.join("Wreath");
        let cache_dir = config_dir.join("Cache");
        Self {
            config_file: config_dir.join("config.toml"),
            legacy_config_files: ["Trace", "Riftclip"]
                .map(|name| local_app_data.join(name).join("config.toml"))
                .into(),
            config_dir,
            thumbnail_dir: cache_dir.join("thumbnails"),
            cache_dir,
            control_endpoint: ControlEndpoint::NamedPipe(r"\\.\pipe\wreath".into()),
        }
    }

    #[allow(dead_code)]
    fn fallback() -> Self {
        let root = std::env::temp_dir().join("wreath");
        Self {
            config_file: root.join("config.toml"),
            legacy_config_files: Vec::new(),
            thumbnail_dir: root.join("cache").join("thumbnails"),
            cache_dir: root.join("cache"),
            config_dir: root,
            control_endpoint: if cfg!(target_os = "windows") {
                ControlEndpoint::NamedPipe(r"\\.\pipe\wreath".into())
            } else {
                ControlEndpoint::UnixSocket(std::env::temp_dir().join("wreath.sock"))
            },
        }
    }

    #[cfg(target_os = "linux")]
    pub fn socket_file(&self) -> &Path {
        match &self.control_endpoint {
            ControlEndpoint::UnixSocket(path) => path,
            ControlEndpoint::NamedPipe(_) => unreachable!("Linux requires a Unix socket"),
        }
    }

    #[cfg(target_os = "windows")]
    pub fn pipe_name(&self) -> &str {
        match &self.control_endpoint {
            ControlEndpoint::NamedPipe(name) => name,
            ControlEndpoint::UnixSocket(_) => unreachable!("Windows requires a named pipe"),
        }
    }
}
