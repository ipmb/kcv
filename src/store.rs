use crate::error::{Error, Result};
use security_framework::item::{ItemClass, ItemSearchOptions, Limit};
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
    /// Every environment that exists, sorted. Removing an environment that is
    /// not there succeeds, so callers need not check first.
    fn environments(&self) -> Result<Vec<String>>;
    fn delete(&self, environment: &str) -> Result<()>;
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

    fn environments(&self) -> Result<Vec<String>> {
        // BTreeMap keys are already sorted.
        Ok(self.items.borrow().keys().cloned().collect())
    }

    fn delete(&self, environment: &str) -> Result<()> {
        self.items.borrow_mut().remove(environment);
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

    /// Searches attributes only, never item data, so this is the one read in
    /// kcv that needs no authorization. It reports which environments exist,
    /// not which ones the caller is approved to decrypt.
    fn environments(&self) -> Result<Vec<String>> {
        let results = match ItemSearchOptions::new()
            .class(ItemClass::generic_password())
            .keychains(std::slice::from_ref(&self.keychain))
            .service(SERVICE)
            .load_attributes(true)
            .limit(Limit::All)
            .search()
        {
            Ok(r) => r,
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => return Ok(Vec::new()),
            Err(e) => return Err(Error::Keychain(e)),
        };

        let mut names: Vec<String> = results
            .iter()
            .filter_map(|r| r.simplify_dict())
            .filter_map(|d| d.get("acct").cloned())
            .collect();
        // Search order is not specified, so impose one.
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn delete(&self, environment: &str) -> Result<()> {
        match self.keychain.find_generic_password(SERVICE, environment) {
            Ok((_, item)) => {
                item.delete();
                Ok(())
            }
            Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
            Err(e) => Err(Error::Keychain(e)),
        }
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
    fn lists_environments_sorted() {
        let store = MemStore::new();
        store.save("zed", b"z").unwrap();
        store.save("alpha", b"a").unwrap();
        store.save("mid", b"m").unwrap();
        assert_eq!(
            store.environments().unwrap(),
            vec!["alpha".to_string(), "mid".to_string(), "zed".to_string()]
        );
    }

    #[test]
    fn lists_nothing_when_empty() {
        let store = MemStore::new();
        assert_eq!(store.environments().unwrap(), Vec::<String>::new());
    }

    #[test]
    fn delete_removes_an_environment() {
        let store = MemStore::new();
        store.save("gone", b"x").unwrap();
        store.save("kept", b"y").unwrap();
        store.delete("gone").unwrap();
        assert_eq!(store.load("gone").unwrap(), None);
        assert_eq!(store.load("kept").unwrap(), Some(b"y".to_vec()));
        assert_eq!(store.environments().unwrap(), vec!["kept".to_string()]);
    }

    #[test]
    fn deleting_something_absent_is_not_an_error() {
        let store = MemStore::new();
        assert!(store.delete("never-existed").is_ok());
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
