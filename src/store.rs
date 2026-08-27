use crate::error::{Error, Result};
use security_framework::os::macos::keychain::SecKeychain;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

/// The keychain service attribute shared by every kcv item.
pub const SERVICE: &str = "kcv";

const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// Byte-level persistence for one environment. Deliberately knows nothing
/// about JSON or variables, so all real logic is testable against `MemStore`.
pub trait Store {
    fn load(&self, environment: &str) -> Result<Option<Vec<u8>>>;
    fn save(&self, environment: &str, data: &[u8]) -> Result<()>;
}

/// In-memory store used by unit tests.
#[derive(Debug, Default)]
pub struct MemStore {
    items: RefCell<BTreeMap<String, Vec<u8>>>,
}

impl MemStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Store for MemStore {
    fn load(&self, environment: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.items.borrow().get(environment).cloned())
    }

    fn save(&self, environment: &str, data: &[u8]) -> Result<()> {
        self.items
            .borrow_mut()
            .insert(environment.to_string(), data.to_vec());
        Ok(())
    }
}

/// Backed by a macOS keychain. Every environment is one generic-password
/// item, so reading an environment costs exactly one authorization event.
pub struct KeychainStore {
    keychain: SecKeychain,
}

impl KeychainStore {
    /// Opens the keychain named by `KCV_KEYCHAIN`, or the user's default
    /// keychain when that variable is unset.
    pub fn open() -> Result<Self> {
        match std::env::var_os("KCV_KEYCHAIN") {
            Some(path) => Self::at(Path::new(&path)),
            None => Ok(Self {
                keychain: SecKeychain::default()?,
            }),
        }
    }

    /// Opens a specific keychain file. Used by `KCV_KEYCHAIN` and by tests.
    pub fn at(path: &Path) -> Result<Self> {
        Ok(Self {
            keychain: SecKeychain::open(path)?,
        })
    }
}

impl Store for KeychainStore {
    fn load(&self, environment: &str) -> Result<Option<Vec<u8>>> {
        match self.keychain.find_generic_password(SERVICE, environment) {
            Ok((password, _item)) => Ok(Some(password.to_vec())),
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(Error::Keychain(e)),
        }
    }

    fn save(&self, environment: &str, data: &[u8]) -> Result<()> {
        // set_generic_password updates an existing item or adds a new one,
        // keeping exactly one item per environment.
        self.keychain
            .set_generic_password(SERVICE, environment, data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_environment_loads_as_none() {
        let store = MemStore::new();
        assert_eq!(store.load("nothing").unwrap(), None);
    }

    #[test]
    fn saved_bytes_come_back_verbatim() {
        let store = MemStore::new();
        store.save("prod", b"payload").unwrap();
        assert_eq!(store.load("prod").unwrap(), Some(b"payload".to_vec()));
    }

    #[test]
    fn save_replaces_previous_contents() {
        let store = MemStore::new();
        store.save("prod", b"first").unwrap();
        store.save("prod", b"second").unwrap();
        assert_eq!(store.load("prod").unwrap(), Some(b"second".to_vec()));
    }

    #[test]
    fn environments_are_isolated() {
        let store = MemStore::new();
        store.save("prod", b"p").unwrap();
        store.save("dev", b"d").unwrap();
        assert_eq!(store.load("prod").unwrap(), Some(b"p".to_vec()));
        assert_eq!(store.load("dev").unwrap(), Some(b"d".to_vec()));
    }
}
