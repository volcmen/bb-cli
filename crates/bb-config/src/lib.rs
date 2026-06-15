//! `bb-config` — configuration and credential storage.
//!
//! Two TOML files in the config dir (the analog of `gh`'s `config.yml` +
//! `hosts.yml`):
//! - `config.toml` — global settings (`default_host`, `git_protocol`, ...)
//! - `hosts.toml` — per-host credentials, written with `0600` permissions.
//!
//! [`FileConfig`] implements [`ConfigProvider`]; [`EnvConfig`] decorates any
//! provider so `BB_TOKEN` / `BB_HOST` take precedence (the analog of `gh`'s
//! `envConfig`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bb_core::{ConfigError, ConfigProvider, DEFAULT_HOST};

type Map = BTreeMap<String, String>;

#[derive(Default)]
struct State {
    global: Map,
    hosts: BTreeMap<String, Map>,
}

/// File-backed config provider.
pub struct FileConfig {
    dir: PathBuf,
    state: Mutex<State>,
}

impl FileConfig {
    /// Load from the resolved config dir
    /// (`$BB_CONFIG_DIR` → `$XDG_CONFIG_HOME/bb` → `~/.config/bb`).
    ///
    /// # Errors
    /// Returns [`ConfigError`] if the dir cannot be resolved or a file fails to
    /// parse.
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(config_dir()?)
    }

    /// Load from an explicit directory (tests, custom locations).
    ///
    /// # Errors
    /// Returns [`ConfigError::Parse`] if a present file is malformed.
    pub fn load_from(dir: PathBuf) -> Result<Self, ConfigError> {
        let global = read_flat_map(&dir.join("config.toml"))?;
        let hosts = read_hosts(&dir.join("hosts.toml"))?;
        Ok(Self {
            dir,
            state: Mutex::new(State { global, hosts }),
        })
    }

    /// An empty in-memory config (tests).
    #[must_use]
    pub fn blank() -> Self {
        Self {
            dir: PathBuf::new(),
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().expect("config state poisoned")
    }
}

impl ConfigProvider for FileConfig {
    fn get(&self, host: &str, key: &str) -> Option<String> {
        let s = self.lock();
        if host.is_empty() {
            s.global.get(key).cloned()
        } else {
            s.hosts.get(host).and_then(|h| h.get(key)).cloned()
        }
    }

    fn set(&self, host: &str, key: &str, value: &str) -> Result<(), ConfigError> {
        let mut s = self.lock();
        if host.is_empty() {
            s.global.insert(key.to_owned(), value.to_owned());
        } else {
            s.hosts
                .entry(host.to_owned())
                .or_default()
                .insert(key.to_owned(), value.to_owned());
        }
        Ok(())
    }

    fn unset_host(&self, host: &str) -> Result<(), ConfigError> {
        self.lock().hosts.remove(host);
        Ok(())
    }

    fn default_host(&self) -> String {
        self.lock()
            .global
            .get("default_host")
            .cloned()
            .unwrap_or_else(|| DEFAULT_HOST.to_owned())
    }

    fn auth_token(&self, host: &str) -> Option<String> {
        self.get(host, "token")
    }

    fn hosts(&self) -> Vec<String> {
        self.lock().hosts.keys().cloned().collect()
    }

    fn save(&self) -> Result<(), ConfigError> {
        let s = self.lock();
        std::fs::create_dir_all(&self.dir).map_err(|e| ConfigError::Io(e.to_string()))?;

        let global_toml =
            toml::to_string(&s.global).map_err(|e| ConfigError::Parse(e.to_string()))?;
        std::fs::write(self.dir.join("config.toml"), global_toml)
            .map_err(|e| ConfigError::Io(e.to_string()))?;

        let hosts_toml =
            toml::to_string(&s.hosts).map_err(|e| ConfigError::Parse(e.to_string()))?;
        let hosts_path = self.dir.join("hosts.toml");
        std::fs::write(&hosts_path, hosts_toml).map_err(|e| ConfigError::Io(e.to_string()))?;
        set_owner_only(&hosts_path)?;
        Ok(())
    }
}

/// Decorates a provider so `BB_TOKEN` / `BB_HOST` env vars take precedence.
pub struct EnvConfig {
    inner: Arc<dyn ConfigProvider>,
}

impl EnvConfig {
    #[must_use]
    pub fn new(inner: Arc<dyn ConfigProvider>) -> Self {
        Self { inner }
    }
}

fn nonempty_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

impl ConfigProvider for EnvConfig {
    fn get(&self, host: &str, key: &str) -> Option<String> {
        self.inner.get(host, key)
    }

    fn set(&self, host: &str, key: &str, value: &str) -> Result<(), ConfigError> {
        self.inner.set(host, key, value)
    }

    fn unset_host(&self, host: &str) -> Result<(), ConfigError> {
        self.inner.unset_host(host)
    }

    fn default_host(&self) -> String {
        nonempty_env("BB_HOST").unwrap_or_else(|| self.inner.default_host())
    }

    fn auth_token(&self, host: &str) -> Option<String> {
        nonempty_env("BB_TOKEN").or_else(|| self.inner.auth_token(host))
    }

    fn hosts(&self) -> Vec<String> {
        self.inner.hosts()
    }

    fn save(&self) -> Result<(), ConfigError> {
        self.inner.save()
    }
}

/// Load the default config provider: a [`FileConfig`] wrapped in [`EnvConfig`].
///
/// # Errors
/// Returns [`ConfigError`] if the config dir cannot be resolved or a file fails
/// to parse.
pub fn load() -> Result<Arc<dyn ConfigProvider>, ConfigError> {
    let file = FileConfig::load()?;
    Ok(Arc::new(EnvConfig::new(Arc::new(file))))
}

fn config_dir() -> Result<PathBuf, ConfigError> {
    if let Some(d) = nonempty_env("BB_CONFIG_DIR") {
        return Ok(PathBuf::from(d));
    }
    if let Some(d) = nonempty_env("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(d).join("bb"));
    }
    let home =
        std::env::var_os("HOME").ok_or_else(|| ConfigError::Io("$HOME is not set".to_owned()))?;
    Ok(PathBuf::from(home).join(".config").join("bb"))
}

fn read_flat_map(path: &Path) -> Result<Map, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents).map_err(|e| ConfigError::Parse(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(ConfigError::Io(e.to_string())),
    }
}

fn read_hosts(path: &Path) -> Result<BTreeMap<String, Map>, ConfigError> {
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents).map_err(|e| ConfigError::Parse(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(ConfigError::Io(e.to_string())),
    }
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| ConfigError::Io(e.to_string()))
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> Result<(), ConfigError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_host_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set("bitbucket.org", "token", "secret").unwrap();
        cfg.set("bitbucket.org", "username", "davidd").unwrap();
        cfg.save().unwrap();

        let reloaded = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        assert_eq!(
            reloaded.auth_token("bitbucket.org").as_deref(),
            Some("secret")
        );
        assert_eq!(
            reloaded.get("bitbucket.org", "username").as_deref(),
            Some("davidd")
        );
        assert_eq!(reloaded.hosts(), vec!["bitbucket.org".to_owned()]);
    }

    #[test]
    fn default_host_falls_back() {
        let cfg = FileConfig::blank();
        assert_eq!(cfg.default_host(), DEFAULT_HOST);
    }

    #[cfg(unix)]
    #[test]
    fn hosts_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set("bitbucket.org", "token", "secret").unwrap();
        cfg.save().unwrap();
        let mode = std::fs::metadata(dir.path().join("hosts.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
