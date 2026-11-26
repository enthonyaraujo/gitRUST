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
    use secret_service::{EncryptionType, blocking::SecretService};
    use std::collections::HashMap;

    fn connect() -> Result<SecretService<'static>, Error> {
        SecretService::connect(EncryptionType::Dh).map_err(|_| Error::Unavailable)
    }

    fn attributes(client: &str) -> HashMap<&str, &str> {
        HashMap::from([
            ("application", "gitRUST"),
            ("provider", "github.com"),
            ("client_id", client),
        ])
    }

    impl CredentialVault {
        /// Read unlocked sessions. Locked items remain in the OS vault.
        ///
        /// # Errors
        /// Returns `Unavailable` if the native service cannot be reached.
        pub fn load(self, client: &str) -> Result<Vec<(u64, Vec<u8>)>, Error> {
            let service = connect()?;
            let items = service
                .search_items(attributes(client))
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
        pub fn store(self, client: &str, id: u64, record: &[u8]) -> Result<(), Error> {
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
            let mut attrs = attributes(client);
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
        pub fn remove(self, client: &str, id: u64) -> Result<(), Error> {
            let service = connect()?;
            let account = id.to_string();
            let mut attrs = attributes(client);
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
    pub fn load(self, _client: &str) -> Result<Vec<(u64, Vec<u8>)>, CredentialError> {
        Err(CredentialError::Unavailable)
    }

    pub fn store(self, _client: &str, _id: u64, _record: &[u8]) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }

    pub fn remove(self, _client: &str, _id: u64) -> Result<(), CredentialError> {
        Err(CredentialError::Unavailable)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

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
            .store(client, 100, b"fixture-one")
            .expect("store first account");
        vault
            .store(client, 200, b"fixture-two")
            .expect("store second account");
        vault
            .store(other, 100, b"other-fixture")
            .expect("store other app");
        let found = vault.load(client).expect("read accounts");
        assert!(found.contains(&(100, b"fixture-one".to_vec())));
        assert!(found.contains(&(200, b"fixture-two".to_vec())));
        assert_eq!(found.len(), 2);
        vault
            .store(client, 100, b"rotated-fixture")
            .expect("rotate pair");
        assert_eq!(
            vault
                .load(client)
                .expect("read rotated")
                .iter()
                .find(|(id, _)| *id == 100)
                .map(|(_, body)| body.as_slice()),
            Some(&b"rotated-fixture"[..])
        );
        vault.remove(client, 100).expect("remove first account");
        assert_eq!(
            vault.load(client).expect("remaining account"),
            [(200, b"fixture-two".to_vec())]
        );
        assert_eq!(
            vault.load(other).expect("other app"),
            [(100, b"other-fixture".to_vec())]
        );
        vault.remove(client, 200).expect("remove second account");
        vault.remove(other, 100).expect("remove other app");
    }
}
