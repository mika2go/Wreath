use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub legacy_config_files: Vec<PathBuf>,
    pub cache_dir: PathBuf,
    pub thumbnail_dir: PathBuf,
    pub socket_file: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Self {
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
            socket_file: runtime_root.join("wreath.sock"),
        }
    }
}
