use std::{
    fmt,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use gitrust_core::{
    github::{AccountId, AuthMethod},
    workspaces::{Appearance, Locale, MAX_RECENT_REPOSITORIES, Preferences, RecentRepositories},
};

const MAGIC: &[u8; 4] = b"GTRS";
const VERSION: u8 = 1;
const MAX_BYTES: usize = 1024 * 1024;
const MAX_PATH_BYTES: usize = 16 * 1024;
const PLATFORM: u8 = if cfg!(windows) { 2 } else { 1 };

/// Failure to load or persist preferences or workspace history.
#[derive(Debug)]
pub enum StorageError {
    /// The platform did not supply a user directory.
    NoUserDirectory,
    /// Filesystem access or atomic replacement failed.
    Io(io::Error),
    /// The file uses a format this build cannot migrate.
    Version,
    /// The file is malformed, too big or for a different platform.
    Invalid,
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoUserDirectory => f.write_str("user directory unavailable"),
            Self::Io(e) => write!(f, "storage access failed: {e}"),
            Self::Version => f.write_str("unsupported storage version"),
            Self::Invalid => f.write_str("invalid storage data"),
        }
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for StorageError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Public numeric account associations keyed by canonical local workspace path.
pub type GithubBindings = std::collections::BTreeMap<PathBuf, AccountId>;

fn valid_client(client: &str) -> bool {
    client.len() <= 100
        && client
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn take_client(input: &mut &[u8]) -> Result<String, StorageError> {
    let size = usize::from(take_u16(input)?);
    let (client, tail) = input.split_at_checked(size).ok_or(StorageError::Invalid)?;
    *input = tail;
    let client = std::str::from_utf8(client).map_err(|_| StorageError::Invalid)?;
    if !valid_client(client) {
        return Err(StorageError::Invalid);
    }
    Ok(client.to_owned())
}

fn push_github_path(output: &mut Vec<u8>, bytes: &[u8]) -> Result<(), StorageError> {
    if bytes.len() > MAX_PATH_BYTES {
        return Err(StorageError::Invalid);
    }
    output.extend(
        u16::try_from(bytes.len())
            .map_err(|_| StorageError::Invalid)?
            .to_le_bytes(),
    );
    output.extend(bytes);
    Ok(())
}

/// Native per-user local settings and workspace storage.
#[derive(Debug)]
pub struct Store {
    preferences: PathBuf,
    recent: PathBuf,
}

impl Store {
    /// Find the platform's native config and local data locations.
    ///
    /// # Errors
    /// Returns an error if user directories are unavailable.
    pub fn discover() -> Result<Self, StorageError> {
        let dirs =
            ProjectDirs::from("io", "gitRUST", "gitRUST").ok_or(StorageError::NoUserDirectory)?;
        Ok(Self::at(dirs.config_dir(), dirs.data_local_dir()))
    }

    /// Construct paths for a selected local profile.
    #[must_use]
    pub fn at(config: &Path, data: &Path) -> Self {
        Self {
            preferences: config.join("preferences.bin"),
            recent: data.join("recent.bin"),
        }
    }

    /// Report whether onboarding was previously completed.
    ///
    /// # Errors
    /// Returns an error if the marker cannot be read.
    pub fn welcome_completed(&self) -> Result<bool, StorageError> {
        let Some(marker) = read(&self.preferences.with_file_name("welcome-v1"))? else {
            return Ok(false);
        };
        if marker == b"1" {
            Ok(true)
        } else {
            Err(StorageError::Invalid)
        }
    }

    /// Record completion without changing appearance, recents or account settings.
    ///
    /// # Errors
    /// Returns a filesystem error if the marker cannot be saved.
    pub fn complete_welcome(&self) -> Result<(), StorageError> {
        replace_file(&self.preferences.with_file_name("welcome-v1"), b"1")
    }

    /// Save a sanitized runtime log in the local application data folder.
    /// # Errors
    /// Returns a storage error if the log cannot be saved.
    pub fn save_runtime_log(&self, log: &str) -> Result<PathBuf, StorageError> {
        let folder = self.recent.with_file_name("logs");
        replace_file(&folder.join("runtime.log"), log.as_bytes())?;
        Ok(folder)
    }

    /// Load preferences, using defaults on first run.
    ///
    /// # Errors
    /// Returns typed errors for unreadable, malformed or unsupported files.
    pub fn load_preferences(&self) -> Result<Preferences, StorageError> {
        let Some(bytes) = read(&self.preferences)? else {
            return Ok(Preferences::default());
        };
        let [appearance, language, first, second] = body(&bytes, b'P')? else {
            return Err(StorageError::Invalid);
        };
        let appearance = match appearance {
            0 => Appearance::System,
            1 => Appearance::Light,
            2 => Appearance::Dark,
            _ => return Err(StorageError::Invalid),
        };
        let locale = match language {
            0 => Locale::English,
            1 => Locale::Portuguese,
            _ => return Err(StorageError::Invalid),
        };
        let scale_percent = u16::from_le_bytes([*first, *second]);
        if !(100..=200).contains(&scale_percent) {
            return Err(StorageError::Invalid);
        }
        Ok(Preferences {
            appearance,
            locale,
            scale_percent,
        })
    }

    /// Atomically save the supplied preferences.
    ///
    /// # Errors
    /// Rejects invalid preferences or an unsupported existing format.
    pub fn save_preferences(&self, value: Preferences) -> Result<(), StorageError> {
        if !(100..=200).contains(&value.scale_percent) {
            return Err(StorageError::Invalid);
        }
        let mut bytes = header(b'P');
        bytes.push(match value.appearance {
            Appearance::System => 0,
            Appearance::Light => 1,
            Appearance::Dark => 2,
        });
        bytes.push(match value.locale {
            Locale::English => 0,
            Locale::Portuguese => 1,
        });
        bytes.extend(value.scale_percent.to_le_bytes());
        write_atomic(&self.preferences, &bytes)
    }

    /// Read public client identifiers and method-specific workspace associations.
    ///
    /// # Errors
    /// Rejects malformed data and unsupported versions.
    pub fn load_github(&self) -> Result<(String, String, GithubBindings), StorageError> {
        let Some(bytes) = read(&self.recent.with_file_name("github.bin"))? else {
            return Ok((String::new(), String::new(), GithubBindings::new()));
        };
        // G is the original Device Flow-only record. H adds the browser client and method.
        let legacy = bytes.get(6) == Some(&b'G');
        let mut input = body(&bytes, if legacy { b'G' } else { b'H' })?;
        let device = take_client(&mut input)?;
        let browser = if legacy {
            String::new()
        } else {
            take_client(&mut input)?
        };
        let count = usize::from(take_u16(&mut input)?);
        let mut bindings = GithubBindings::new();
        for _ in 0..count {
            let size = usize::from(take_u16(&mut input)?);
            let (path, tail) = input.split_at_checked(size).ok_or(StorageError::Invalid)?;
            let path = decode_path(path)?;
            let (number, tail) = tail.split_at_checked(8).ok_or(StorageError::Invalid)?;
            let number = u64::from_le_bytes(number.try_into().map_err(|_| StorageError::Invalid)?);
            input = tail;
            let method = if legacy {
                AuthMethod::Device
            } else {
                let (tag, rest) = input.split_first().ok_or(StorageError::Invalid)?;
                input = rest;
                match tag {
                    0 => AuthMethod::Device,
                    1 if !browser.is_empty() => AuthMethod::Browser,
                    _ => return Err(StorageError::Invalid),
                }
            };
            if !path.is_absolute()
                || number == 0
                || bindings
                    .insert(path, AccountId { method, number })
                    .is_some()
            {
                return Err(StorageError::Invalid);
            }
        }
        if !input.is_empty() {
            return Err(StorageError::Invalid);
        }
        Ok((device, browser, bindings))
    }

    /// Save public client IDs and account associations in one atomic replacement.
    ///
    /// # Errors
    /// Rejects invalid identifiers or paths and reports filesystem failures.
    pub fn save_github(
        &self,
        device: &str,
        browser: &str,
        bindings: &GithubBindings,
    ) -> Result<(), StorageError> {
        if !valid_client(device) || !valid_client(browser) {
            return Err(StorageError::Invalid);
        }
        let mut bytes = header(b'H');
        push_github_path(&mut bytes, device.as_bytes())?;
        push_github_path(&mut bytes, browser.as_bytes())?;
        bytes.extend(
            u16::try_from(bindings.len())
                .map_err(|_| StorageError::Invalid)?
                .to_le_bytes(),
        );
        for (path, id) in bindings {
            if !path.is_absolute() || id.number == 0 {
                return Err(StorageError::Invalid);
            }
            push_github_path(&mut bytes, &encode_path(path)?)?;
            bytes.extend(id.number.to_le_bytes());
            bytes.push(match id.method {
                AuthMethod::Device => 0,
                AuthMethod::Browser if !browser.is_empty() => 1,
                AuthMethod::Browser => return Err(StorageError::Invalid),
            });
        }
        if bytes.len() > MAX_BYTES {
            return Err(StorageError::Invalid);
        }
        write_atomic(&self.recent.with_file_name("github.bin"), &bytes)
    }

    /// Load only the repository paths and selection history.
    ///
    /// # Errors
    /// Returns typed errors for unreadable, malformed or unsupported files.
    pub fn load_recent(&self) -> Result<RecentRepositories, StorageError> {
        let Some(bytes) = read(&self.recent)? else {
            return Ok(RecentRepositories::default());
        };
        let mut input = body(&bytes, b'R')?;
        let count = usize::from(take_u16(&mut input)?);
        let last_index = take_u16(&mut input)?;
        if count > MAX_RECENT_REPOSITORIES {
            return Err(StorageError::Invalid);
        }
        let mut paths = Vec::with_capacity(count);
        for _ in 0..count {
            let n = usize::from(take_u16(&mut input)?);
            if n == 0 || n > MAX_PATH_BYTES {
                return Err(StorageError::Invalid);
            }
            let (value, tail) = input.split_at_checked(n).ok_or(StorageError::Invalid)?;
            input = tail;
            let path = decode_path(value)?;
            if !path.is_absolute() || paths.contains(&path) {
                return Err(StorageError::Invalid);
            }
            paths.push(path);
        }
        if !input.is_empty() {
            return Err(StorageError::Invalid);
        }
        let last = if last_index == u16::MAX {
            None
        } else {
            Some(
                paths
                    .get(usize::from(last_index))
                    .ok_or(StorageError::Invalid)?
                    .clone(),
            )
        };
        Ok(RecentRepositories { paths, last })
    }

    /// Atomically save recent workspace paths without authorizations.
    ///
    /// # Errors
    /// Rejects invalid paths, excessive size and unsupported existing formats.
    pub fn save_recent(&self, value: &RecentRepositories) -> Result<(), StorageError> {
        let paths = &value.paths;
        if paths.len() > MAX_RECENT_REPOSITORIES {
            return Err(StorageError::Invalid);
        }
        let count = u16::try_from(paths.len()).map_err(|_| StorageError::Invalid)?;
        let last = value
            .last
            .as_ref()
            .map(|path| {
                paths
                    .iter()
                    .position(|entry| entry == path)
                    .and_then(|idx| u16::try_from(idx).ok())
                    .ok_or(StorageError::Invalid)
            })
            .transpose()?
            .unwrap_or(u16::MAX);
        let mut bytes = header(b'R');
        bytes.extend(count.to_le_bytes());
        bytes.extend(last.to_le_bytes());
        for (index, path) in paths.iter().enumerate() {
            if !path.is_absolute() || paths[..index].contains(path) {
                return Err(StorageError::Invalid);
            }
            let encoded = encode_path(path)?;
            if encoded.is_empty() || encoded.len() > MAX_PATH_BYTES {
                return Err(StorageError::Invalid);
            }
            let length = u16::try_from(encoded.len()).map_err(|_| StorageError::Invalid)?;
            bytes.extend(length.to_le_bytes());
            bytes.extend(encoded);
            if bytes.len() > MAX_BYTES {
                return Err(StorageError::Invalid);
            }
        }
        write_atomic(&self.recent, &bytes)
    }
}

fn header(kind: u8) -> Vec<u8> {
    let mut result = MAGIC.to_vec();
    result.extend([VERSION, PLATFORM, kind]);
    result
}

fn body(bytes: &[u8], kind: u8) -> Result<&[u8], StorageError> {
    match bytes.strip_prefix(MAGIC) {
        Some([VERSION, PLATFORM, actual, rest @ ..]) if *actual == kind => Ok(rest),
        Some([version, ..]) if *version != VERSION => Err(StorageError::Version),
        _ => Err(StorageError::Invalid),
    }
}

fn take_u16(input: &mut &[u8]) -> Result<u16, StorageError> {
    let [a, b, rest @ ..] = *input else {
        return Err(StorageError::Invalid);
    };
    *input = rest;
    Ok(u16::from_le_bytes([*a, *b]))
}

fn read(path: &Path) -> Result<Option<Vec<u8>>, StorageError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !metadata.file_type().is_file() {
        return Err(StorageError::Invalid);
    }
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(u64::try_from(MAX_BYTES + 1).map_err(|_| StorageError::Invalid)?)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(StorageError::Invalid);
    }
    Ok(Some(bytes))
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    if let Some(old) = read(path)?
        && matches!(body(&old, bytes[6]), Err(StorageError::Version))
    {
        return Err(StorageError::Version);
    }
    replace_file(path, bytes)
}

fn replace_file(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().ok_or(StorageError::Invalid)?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn encode_path(path: &Path) -> Result<Vec<u8>, StorageError> {
    use std::os::unix::ffi::OsStrExt as _;
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) {
        Err(StorageError::Invalid)
    } else {
        Ok(bytes.to_vec())
    }
}

#[cfg(unix)]
fn decode_path(bytes: &[u8]) -> Result<PathBuf, StorageError> {
    use std::os::unix::ffi::OsStrExt as _;
    if bytes.contains(&0) {
        return Err(StorageError::Invalid);
    }
    Ok(PathBuf::from(std::ffi::OsStr::from_bytes(bytes)))
}

#[cfg(windows)]
fn encode_path(path: &Path) -> Result<Vec<u8>, StorageError> {
    use std::os::windows::ffi::OsStrExt as _;
    let wide: Vec<_> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(StorageError::Invalid);
    }
    Ok(wide.into_iter().flat_map(u16::to_le_bytes).collect())
}

#[cfg(windows)]
fn decode_path(bytes: &[u8]) -> Result<PathBuf, StorageError> {
    use std::os::windows::ffi::OsStringExt as _;
    let (words, tail) = bytes.as_chunks::<2>();
    let wide: Vec<_> = words.iter().copied().map(u16::from_le_bytes).collect();
    if !tail.is_empty() || wide.contains(&0) {
        return Err(StorageError::Invalid);
    }
    Ok(std::ffi::OsString::from_wide(&wide).into())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn profile_round_trips_only_local_choices_and_keeps_mru_order() {
        let root = tempfile::tempdir().expect("profile");
        let store = || Store::at(&root.path().join("config"), &root.path().join("data"));
        assert_eq!(
            store().load_preferences().expect("default preferences"),
            Preferences::default()
        );
        assert_eq!(
            store().load_recent().expect("first run"),
            RecentRepositories::default()
        );
        let prefs = Preferences {
            appearance: Appearance::Dark,
            locale: Locale::Portuguese,
            scale_percent: 150,
        };
        let mut recent = RecentRepositories::default();
        for path in ["one", "two", "one"] {
            recent.open(root.path().join(path));
        }
        assert_eq!(
            recent.paths,
            [root.path().join("one"), root.path().join("two")]
        );
        store().save_preferences(prefs).expect("save preferences");
        store().save_recent(&recent).expect("save recent paths");
        assert_eq!(
            store().load_preferences().expect("reopened preferences"),
            prefs
        );
        assert_eq!(store().load_recent().expect("reopened recents"), recent);
        assert!(
            !fs::read(store().recent)
                .expect("native record")
                .windows(5)
                .any(|window| window == b"token")
        );
    }

    #[test]
    fn damaged_file_and_future_version_are_detected_without_overwriting_the_future_file() {
        let root = tempfile::tempdir().expect("profile");
        let store = Store::at(root.path(), root.path());
        fs::write(&store.preferences, b"GTRS\xff").expect("future settings");
        assert!(matches!(
            store.load_preferences(),
            Err(StorageError::Version)
        ));
        assert!(matches!(
            store.save_preferences(Preferences::default()),
            Err(StorageError::Version)
        ));
        assert_eq!(
            fs::read(&store.preferences).expect("unchanged future file"),
            b"GTRS\xff"
        );
        fs::write(&store.recent, b"corrupt").expect("broken catalog");
        assert!(matches!(store.load_recent(), Err(StorageError::Invalid)));
        let blocked = root.path().join("blocked");
        fs::write(&blocked, b"regular file").expect("unusable parent");
        let inaccessible = Store::at(&blocked, &blocked);
        assert!(
            inaccessible
                .save_preferences(Preferences::default())
                .is_err()
        );
    }

    #[test]
    fn legacy_github_workspace_bindings_keep_the_device_account() {
        let root = tempfile::tempdir().expect("profile");
        let store = Store::at(root.path(), root.path());
        let path = root.path().join("repository");
        let legacy = AccountId {
            method: AuthMethod::Device,
            number: 42,
        };
        let mut original = header(b'G');
        push_github_path(&mut original, b"fixtureDeviceApp").expect("client");
        original.extend(1_u16.to_le_bytes());
        push_github_path(&mut original, &encode_path(&path).expect("encoded path")).expect("path");
        original.extend(legacy.number.to_le_bytes());
        let record = store.recent.with_file_name("github.bin");
        fs::write(record, original).expect("legacy profile");

        let (device, browser, mut bindings) = store.load_github().expect("legacy profile restored");
        assert_eq!(device, "fixtureDeviceApp");
        assert_eq!(browser, "");
        assert_eq!(bindings.get(&path), Some(&legacy));
        let other = root.path().join("other");
        let oauth = AccountId {
            method: AuthMethod::Browser,
            number: legacy.number,
        };
        bindings.insert(other, oauth);
        store
            .save_github(&device, "fixtureBrowserApp", &bindings)
            .expect("versioned record");
        assert_eq!(
            store.load_github().expect("versioned record"),
            (device, "fixtureBrowserApp".into(), bindings)
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_paths_retain_arbitrary_filename_bytes() {
        use std::os::unix::ffi::OsStringExt as _;
        let root = tempfile::tempdir().expect("profile");
        let path = root
            .path()
            .join(std::ffi::OsString::from_vec(b"there-\xff".to_vec()));
        let store = Store::at(root.path(), root.path());
        let mut recent = RecentRepositories::default();
        recent.open(path);
        store.save_recent(&recent).expect("save native bytes");
        assert_eq!(store.load_recent().expect("reload native bytes"), recent);
    }
}
