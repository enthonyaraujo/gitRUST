//! Native secret storage. The only persistent implementation is Secret Service.
#![allow(missing_docs)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialError {
    Unavailable,
    Invalid,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CredentialVault;

#[cfg(target_os = "linux")]
mod linux {
    use super::{CredentialError as Error, CredentialVault};
    use gitrust_core::github::AuthMethod;
    use secret_service::{EncryptionType, blocking::SecretService};
    use std::collections::HashMap;

    fn connect() -> Result<SecretService<'static>, Error> {
        SecretService::connect(EncryptionType::Dh).map_err(|_| Error::Unavailable)
    }

    fn attributes(client: &str, method: AuthMethod) -> HashMap<&str, &str> {
        HashMap::from([
            ("application", "gitRUST"),
            (
                "provider",
                match method {
                    AuthMethod::Device => "github.com",
                    AuthMethod::Browser => "github.com OAuth App",
                },
            ),
            ("client_id", client),
        ])
    }

    impl CredentialVault {
        /// Read the provisioned developer secret. Its namespace excludes session records.
        ///
        /// # Errors
        /// Returns an error if the entry is inaccessible, ambiguous or malformed.
        pub fn oauth_client_secret(self, client: &str) -> Result<String, Error> {
            let service = connect()?;
            let items = service
                .search_items(HashMap::from([
                    ("application", "gitRUST"),
                    ("provider", "github.com OAuth client"),
                    ("client_id", client),
                ]))
                .map_err(|_| Error::Unavailable)?;
            let mut found = items.unlocked.iter().chain(&items.locked);
            let item = found.next().ok_or(Error::Unavailable)?;
            if found.next().is_some() {
                return Err(Error::Invalid);
            }
            if !items.locked.is_empty() {
                item.unlock().map_err(|_| Error::Unavailable)?;
            }
            let data = item.get_secret().map_err(|_| Error::Unavailable)?;
            let secret = String::from_utf8(data).map_err(|_| Error::Invalid)?;
            if !(16..=1024).contains(&secret.len())
                || !secret
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
            {
                return Err(Error::Invalid);
            }
            Ok(secret)
        }

        /// Read unlocked sessions. Locked items remain in the OS vault.
        ///
        /// # Errors
        /// Returns `Unavailable` if the native service cannot be reached.
        pub fn load(self, client: &str, method: AuthMethod) -> Result<Vec<(u64, Vec<u8>)>, Error> {
            let service = connect()?;
            let items = service
                .search_items(attributes(client, method))
                .map_err(|_| Error::Unavailable)?;
            if !items.locked.is_empty() {
                service
                    .unlock_all(&items.locked.iter().collect::<Vec<_>>())
                    .map_err(|_| Error::Unavailable)?;
            }
            let mut output = Vec::new();
            for item in items.unlocked.into_iter().chain(items.locked).take(32) {
                let attrs = item.get_attributes().map_err(|_| Error::Unavailable)?;
                let Some(id) = attrs.get("account").and_then(|id| id.parse().ok()) else {
                    continue;
                };
                let data = item.get_secret().map_err(|_| Error::Unavailable)?;
                if data.len() <= 32 * 1024 {
                    output.push((id, data));
                }
            }
            Ok(output)
        }

        /// Write an entire session atomically under its app and account identity.
        ///
        /// # Errors
        /// Returns `Unavailable` if the native service cannot store the item.
        pub fn store(
            self,
            client: &str,
            method: AuthMethod,
            id: u64,
            record: &[u8],
        ) -> Result<(), Error> {
            if record.len() > 32 * 1024 {
                return Err(Error::Invalid);
            }
            let service = connect()?;
            let collection = service
                .get_default_collection()
                .map_err(|_| Error::Unavailable)?;
            if collection.is_locked().map_err(|_| Error::Unavailable)? {
                collection.unlock().map_err(|_| Error::Unavailable)?;
            }
            let account = id.to_string();
            let mut attrs = attributes(client, method);
            attrs.insert("account", &account);
            collection
                .create_item(
                    "gitRUST GitHub session",
                    attrs,
                    record,
                    true,
                    "application/octet-stream",
                )
                .map_err(|_| Error::Unavailable)?;
            Ok(())
        }

        /// Remove the specified account's matching secrets from the device.
        ///
        /// # Errors
        /// Returns `Unavailable` if the service cannot delete the item.
        pub fn remove(self, client: &str, method: AuthMethod, id: u64) -> Result<(), Error> {
            let service = connect()?;
            let account = id.to_string();
            let mut attrs = attributes(client, method);
            attrs.insert("account", &account);
            let items = service
                .search_items(attrs)
                .map_err(|_| Error::Unavailable)?;
            if !items.locked.is_empty() {
                service
                    .unlock_all(&items.locked.iter().collect::<Vec<_>>())
                    .map_err(|_| Error::Unavailable)?;
            }
            for item in items.unlocked.into_iter().chain(items.locked) {
                item.delete().map_err(|_| Error::Unavailable)?;
            }
            Ok(())
        }
    }
}

#[cfg(not(target_os = "linux"))]
impl CredentialVault {
    pub fn oauth_client_secret(self, _client: &str) -> Result<String, CredentialError> {
        Err(CredentialError::Unavailable)
    }

    pub fn load(
        self,
        _client: &str,
        _method: gitrust_core::github::AuthMethod,
    ) -> Result<Vec<(u64, Vec<u8>)>, CredentialError> {
        Err(CredentialError::Unavailable)
    }

    pub fn store(
        self,
        _client: &str,
        _method: gitrust_core::github::AuthMethod,
        _id: u64,
        _record: &[u8],
    ) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }

    pub fn remove(
        self,
        _client: &str,
        _method: gitrust_core::github::AuthMethod,
        _id: u64,
    ) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use gitrust_core::github::AuthMethod::{Browser, Device};

    #[test]
    #[ignore = "run inside an isolated D-Bus session with a disposable Secret Service collection"]
    fn encrypted_secret_service_sessions_are_isolated_and_removable() {
        assert_eq!(
            std::env::var("GITRUST_TEST_DISPOSABLE_VAULT").as_deref(),
            Ok("1")
        );
        let vault = CredentialVault;
        let client = "stage07VaultFixture";
        let other = "otherAppVaultFixture";
        vault
            .store(client, Device, 100, b"fixture-one")
            .expect("store first account");
        vault
            .store(client, Device, 200, b"fixture-two")
            .expect("store second account");
        vault
            .store(other, Device, 100, b"other-fixture")
            .expect("store other app");
        vault
            .store(client, Browser, 100, b"browser-session")
            .expect("different kind");
        assert_eq!(
            vault.load(client, Browser).expect("browser records"),
            [(100, b"browser-session".to_vec())]
        );
        let found = vault.load(client, Device).expect("read accounts");
        assert!(found.contains(&(100, b"fixture-one".to_vec())));
        assert!(found.contains(&(200, b"fixture-two".to_vec())));
        assert_eq!(found.len(), 2);
        vault
            .store(client, Device, 100, b"rotated-fixture")
            .expect("rotate pair");
        assert_eq!(
            vault
                .load(client, Device)
                .expect("read rotated")
                .iter()
                .find(|(id, _)| *id == 100)
                .map(|(_, body)| body.as_slice()),
            Some(&b"rotated-fixture"[..])
        );
        vault
            .remove(client, Device, 100)
            .expect("remove first account");
        assert_eq!(
            vault.load(client, Device).expect("remaining account"),
            [(200, b"fixture-two".to_vec())]
        );
        assert_eq!(
            vault.load(other, Device).expect("other app"),
            [(100, b"other-fixture".to_vec())]
        );
        vault
            .remove(client, Device, 200)
            .expect("remove second account");
        vault.remove(other, Device, 100).expect("remove other app");
        vault
            .remove(client, Browser, 100)
            .expect("remove browser record");
    }
}
