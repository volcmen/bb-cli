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
        // Recover the guard on poison rather than panicking: bb is single-threaded
        // per command, so a poisoned lock only means a prior panic, and the state
        // is still consistent enough to flush.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        if self.dir.as_os_str().is_empty() {
            return Err(ConfigError::Io(
                "cannot save a blank in-memory config (no directory)".to_owned(),
            ));
        }
        let s = self.lock();
        std::fs::create_dir_all(&self.dir).map_err(|e| ConfigError::Io(e.to_string()))?;
        // The config dir can hold credentials (hosts.toml); keep it owner-only.
        set_dir_owner_only(&self.dir)?;

        let global_toml =
            toml::to_string(&s.global).map_err(|e| ConfigError::Parse(e.to_string()))?;
        std::fs::write(self.dir.join("config.toml"), global_toml)
            .map_err(|e| ConfigError::Io(e.to_string()))?;

        let hosts_toml =
            toml::to_string(&s.hosts).map_err(|e| ConfigError::Parse(e.to_string()))?;
        let hosts_path = self.dir.join("hosts.toml");
        // Write credentials 0600 *atomically*: create the file owner-only so it is
        // never briefly world-readable under the process umask (the old
        // write-then-chmod left a TOCTOU window).
        write_owner_only(&hosts_path, hosts_toml.as_bytes())?;
        Ok(())
    }
}

/// A process-env getter: returns `Some(value)` only for set, non-empty vars.
type EnvGetter = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Decorates a provider so `BB_TOKEN` / `BB_HOST` env vars take precedence.
pub struct EnvConfig {
    inner: Arc<dyn ConfigProvider>,
    /// How env vars are read. Defaults to the real process environment
    /// ([`nonempty_env`]); tests inject a fake to avoid process-global env
    /// races under parallel threads.
    env: EnvGetter,
}

impl EnvConfig {
    #[must_use]
    pub fn new(inner: Arc<dyn ConfigProvider>) -> Self {
        Self {
            inner,
            env: Arc::new(|k: &str| nonempty_env(k)),
        }
    }

    /// Like [`EnvConfig::new`], but with an injected env getter (tests). The
    /// getter must return `None` for unset *or empty* values to match the real
    /// [`nonempty_env`] semantics.
    #[cfg(test)]
    #[must_use]
    fn with_env_getter(inner: Arc<dyn ConfigProvider>, env: EnvGetter) -> Self {
        Self { inner, env }
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
        (self.env)("BB_HOST").unwrap_or_else(|| self.inner.default_host())
    }

    fn auth_token(&self, host: &str) -> Option<String> {
        (self.env)("BB_TOKEN").or_else(|| self.inner.auth_token(host))
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
    config_dir_from(nonempty_env)
}

/// Resolve the config dir from an injected env getter, with the precedence
/// `BB_CONFIG_DIR` → `$XDG_CONFIG_HOME/bb` → `$HOME/.config/bb`.
///
/// The getter is expected to return `None` for unset *or empty* values (the
/// real getter, [`nonempty_env`], does this). Factoring the resolution behind a
/// getter lets tests exercise precedence deterministically without mutating the
/// process-global environment, which races under parallel test threads.
fn config_dir_from(get: impl Fn(&str) -> Option<String>) -> Result<PathBuf, ConfigError> {
    if let Some(d) = get("BB_CONFIG_DIR") {
        return Ok(PathBuf::from(d));
    }
    if let Some(d) = get("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(d).join("bb"));
    }
    let home = get("HOME").ok_or_else(|| ConfigError::Io("$HOME is not set".to_owned()))?;
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
fn write_owner_only(path: &Path, contents: &[u8]) -> Result<(), ConfigError> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    // Create with 0600 from the start (no umask window), then also enforce the
    // mode in case the file already existed with looser permissions.
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| ConfigError::Io(e.to_string()))?;
    f.write_all(contents)
        .map_err(|e| ConfigError::Io(e.to_string()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| ConfigError::Io(e.to_string()))
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, contents: &[u8]) -> Result<(), ConfigError> {
    std::fs::write(path, contents).map_err(|e| ConfigError::Io(e.to_string()))
}

/// Restrict the config directory to the owner (`0700`) so credential files
/// underneath it aren't enumerable by other users.
#[cfg(unix)]
fn set_dir_owner_only(path: &Path) -> Result<(), ConfigError> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| ConfigError::Io(e.to_string()))
}

#[cfg(not(unix))]
fn set_dir_owner_only(_path: &Path) -> Result<(), ConfigError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Build an env getter from a fixed table, mirroring `nonempty_env`
    /// semantics (empty string ⇒ `None`).
    fn fake_env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k: &str| map.get(k).filter(|v| !v.is_empty()).cloned()
    }

    fn env_arc(pairs: &[(&str, &str)]) -> EnvGetter {
        let getter = fake_env(pairs);
        Arc::new(move |k: &str| getter(k))
    }

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

    // ---- config_dir precedence (via the injected getter) ----

    #[test]
    fn config_dir_prefers_bb_config_dir() {
        let dir = config_dir_from(fake_env(&[
            ("BB_CONFIG_DIR", "/explicit/cfg"),
            ("XDG_CONFIG_HOME", "/xdg"),
            ("HOME", "/home/me"),
        ]))
        .unwrap();
        assert_eq!(dir, PathBuf::from("/explicit/cfg"));
    }

    #[test]
    fn config_dir_falls_back_to_xdg() {
        let dir = config_dir_from(fake_env(&[
            ("XDG_CONFIG_HOME", "/xdg"),
            ("HOME", "/home/me"),
        ]))
        .unwrap();
        assert_eq!(dir, PathBuf::from("/xdg").join("bb"));
    }

    #[test]
    fn config_dir_falls_back_to_home() {
        let dir = config_dir_from(fake_env(&[("HOME", "/home/me")])).unwrap();
        assert_eq!(dir, PathBuf::from("/home/me").join(".config").join("bb"));
    }

    #[test]
    fn config_dir_errors_without_home() {
        let err = config_dir_from(fake_env(&[])).unwrap_err();
        assert!(matches!(err, ConfigError::Io(_)));
    }

    #[test]
    fn config_dir_treats_empty_vars_as_unset() {
        // Empty BB_CONFIG_DIR / XDG_CONFIG_HOME must not win.
        let dir = config_dir_from(fake_env(&[
            ("BB_CONFIG_DIR", ""),
            ("XDG_CONFIG_HOME", ""),
            ("HOME", "/home/me"),
        ]))
        .unwrap();
        assert_eq!(dir, PathBuf::from("/home/me").join(".config").join("bb"));
    }

    // ---- EnvConfig overrides (via the injected getter, no real env) ----

    #[test]
    fn env_config_bb_token_overrides_inner() {
        let inner = FileConfig::blank();
        inner.set("bitbucket.org", "token", "file-token").unwrap();
        let cfg =
            EnvConfig::with_env_getter(Arc::new(inner), env_arc(&[("BB_TOKEN", "env-token")]));
        assert_eq!(
            cfg.auth_token("bitbucket.org").as_deref(),
            Some("env-token")
        );
    }

    #[test]
    fn env_config_bb_token_unset_falls_back_to_inner() {
        let inner = FileConfig::blank();
        inner.set("bitbucket.org", "token", "file-token").unwrap();
        let cfg = EnvConfig::with_env_getter(Arc::new(inner), env_arc(&[]));
        assert_eq!(
            cfg.auth_token("bitbucket.org").as_deref(),
            Some("file-token")
        );
    }

    #[test]
    fn env_config_empty_bb_token_falls_back_to_inner() {
        let inner = FileConfig::blank();
        inner.set("bitbucket.org", "token", "file-token").unwrap();
        let cfg = EnvConfig::with_env_getter(Arc::new(inner), env_arc(&[("BB_TOKEN", "")]));
        assert_eq!(
            cfg.auth_token("bitbucket.org").as_deref(),
            Some("file-token")
        );
    }

    #[test]
    fn env_config_bb_host_overrides_default_host() {
        let inner = FileConfig::blank();
        inner
            .set("", "default_host", "configured.example.com")
            .unwrap();
        let cfg =
            EnvConfig::with_env_getter(Arc::new(inner), env_arc(&[("BB_HOST", "env.example.com")]));
        assert_eq!(cfg.default_host(), "env.example.com");
    }

    #[test]
    fn env_config_bb_host_unset_falls_back_to_inner_then_default() {
        let inner = FileConfig::blank();
        let cfg = EnvConfig::with_env_getter(Arc::new(inner), env_arc(&[]));
        assert_eq!(cfg.default_host(), DEFAULT_HOST);

        let inner2 = FileConfig::blank();
        inner2
            .set("", "default_host", "configured.example.com")
            .unwrap();
        let cfg2 = EnvConfig::with_env_getter(Arc::new(inner2), env_arc(&[]));
        assert_eq!(cfg2.default_host(), "configured.example.com");
    }

    #[test]
    fn env_config_real_constructor_delegates_when_no_env() {
        // The public `new` reads the real env; in a clean test process neither
        // BB_TOKEN nor BB_HOST is set, so it must delegate to the inner config.
        // (Guarded so it stays correct even if the harness sets them.)
        if nonempty_env("BB_HOST").is_none() {
            let inner = FileConfig::blank();
            let cfg = EnvConfig::new(Arc::new(inner));
            assert_eq!(cfg.default_host(), DEFAULT_HOST);
        }
    }

    // ---- dotted host keys, multiple hosts, removal, missing lookups ----

    #[test]
    fn dotted_host_key_round_trips_through_toml() {
        // "bitbucket.org" contains a dot, which TOML must quote as a table key.
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set("bitbucket.org", "token", "t").unwrap();
        cfg.set("bitbucket.org", "username", "davidd").unwrap();
        cfg.save().unwrap();

        // The on-disk file must quote the dotted table header.
        let raw = std::fs::read_to_string(dir.path().join("hosts.toml")).unwrap();
        assert!(
            raw.contains("[\"bitbucket.org\"]"),
            "expected quoted dotted table header, got: {raw}"
        );

        let reloaded = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        assert_eq!(reloaded.get("bitbucket.org", "token").as_deref(), Some("t"));
        assert_eq!(
            reloaded.get("bitbucket.org", "username").as_deref(),
            Some("davidd")
        );
    }

    #[test]
    fn multiple_hosts_round_trip_and_are_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set("bitbucket.org", "token", "cloud").unwrap();
        cfg.set("bb.acme.com", "token", "datacenter").unwrap();
        cfg.save().unwrap();

        let reloaded = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        let mut hosts = reloaded.hosts();
        hosts.sort();
        assert_eq!(
            hosts,
            vec!["bb.acme.com".to_owned(), "bitbucket.org".to_owned()]
        );
        assert_eq!(
            reloaded.auth_token("bitbucket.org").as_deref(),
            Some("cloud")
        );
        assert_eq!(
            reloaded.auth_token("bb.acme.com").as_deref(),
            Some("datacenter")
        );
    }

    #[test]
    fn unset_host_removes_only_that_host() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        cfg.set("bitbucket.org", "token", "cloud").unwrap();
        cfg.set("bb.acme.com", "token", "datacenter").unwrap();

        cfg.unset_host("bitbucket.org").unwrap();
        assert_eq!(cfg.hosts(), vec!["bb.acme.com".to_owned()]);
        assert!(cfg.auth_token("bitbucket.org").is_none());
        assert_eq!(cfg.auth_token("bb.acme.com").as_deref(), Some("datacenter"));

        // Persists across save/reload.
        cfg.save().unwrap();
        let reloaded = FileConfig::load_from(dir.path().to_path_buf()).unwrap();
        assert_eq!(reloaded.hosts(), vec!["bb.acme.com".to_owned()]);
    }

    #[test]
    fn get_on_missing_host_or_key_is_none() {
        let cfg = FileConfig::blank();
        cfg.set("bitbucket.org", "token", "t").unwrap();
        assert!(cfg.get("nope.example.com", "token").is_none());
        assert!(cfg.get("bitbucket.org", "missing_key").is_none());
        assert!(cfg.auth_token("nope.example.com").is_none());
    }

    #[test]
    fn global_keys_use_empty_host() {
        let cfg = FileConfig::blank();
        cfg.set("", "git_protocol", "ssh").unwrap();
        assert_eq!(cfg.get("", "git_protocol").as_deref(), Some("ssh"));
        // A host lookup must not see the global value.
        assert!(cfg.get("bitbucket.org", "git_protocol").is_none());
    }
}
