use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// All of one environment's variables. Backed by a `BTreeMap` so that
/// serialization is deterministic and test assertions are stable.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EnvSet(BTreeMap<String, String>);

/// Variable names must be usable in a C environment block: non-empty, and
/// free of '=' (the separator) and NUL (the terminator).
pub fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() || key.contains('=') || key.contains('\0') {
        return Err(Error::InvalidKey(key.to_string()));
    }
    Ok(())
}

impl EnvSet {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn from_json(bytes: &[u8], environment: &str) -> Result<Self> {
        let map: BTreeMap<String, String> = serde_json::from_slice(bytes)
            .map_err(|_| Error::CorruptItem(environment.to_string()))?;
        Ok(Self(map))
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec(&self.0).expect("BTreeMap<String, String> always serializes")
    }

    pub fn insert(&mut self, key: &str, value: &str) -> Result<()> {
        validate_key(key)?;
        if value.contains('\0') {
            // Named without echoing the value, which is a secret.
            return Err(Error::InvalidKey(format!(
                "{key} (value contains a NUL byte)"
            )));
        }
        self.0.insert(key.to_string(), value.to_string());
        Ok(())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn round_trips_through_json() {
        let mut set = EnvSet::new();
        set.insert("FOO", "bar").unwrap();
        set.insert("MULTI", "line1\nline2\t\"quoted\"").unwrap();
        let back = EnvSet::from_json(&set.to_json(), "test").unwrap();
        assert_eq!(back.len(), 2);
        let map: BTreeMap<_, _> = back.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        assert_eq!(map["FOO"], "bar");
        assert_eq!(map["MULTI"], "line1\nline2\t\"quoted\"");
    }

    #[test]
    fn insert_overwrites_and_preserves_other_keys() {
        let mut set = EnvSet::new();
        set.insert("KEEP", "untouched").unwrap();
        set.insert("FOO", "first").unwrap();
        set.insert("FOO", "second").unwrap();
        assert_eq!(set.len(), 2);
        let map: BTreeMap<_, _> = set.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        assert_eq!(map["FOO"], "second");
        assert_eq!(map["KEEP"], "untouched");
    }

    #[test]
    fn empty_values_are_legal() {
        let mut set = EnvSet::new();
        set.insert("EMPTY", "").unwrap();
        let back = EnvSet::from_json(&set.to_json(), "test").unwrap();
        assert_eq!(back.iter().next().unwrap().1, "");
    }

    #[test]
    fn rejects_invalid_keys() {
        let mut set = EnvSet::new();
        assert!(matches!(set.insert("", "v"), Err(Error::InvalidKey(_))));
        assert!(matches!(set.insert("A=B", "v"), Err(Error::InvalidKey(_))));
        assert!(matches!(set.insert("A\0B", "v"), Err(Error::InvalidKey(_))));
    }

    #[test]
    fn rejects_values_containing_nul() {
        let mut set = EnvSet::new();
        assert!(set.insert("K", "has\0nul").is_err());
    }

    #[test]
    fn corrupt_data_is_an_error_naming_the_environment() {
        let err = EnvSet::from_json(b"not json at all", "prod").unwrap_err();
        assert!(matches!(err, Error::CorruptItem(_)));
        assert!(err.to_string().contains("prod"));
    }

    #[test]
    fn json_of_wrong_shape_is_corrupt() {
        assert!(EnvSet::from_json(b"[1,2,3]", "prod").is_err());
        assert!(EnvSet::from_json(br#"{"K":5}"#, "prod").is_err());
    }

    #[test]
    fn reports_emptiness() {
        let mut set = EnvSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        set.insert("K", "v").unwrap();
        assert!(!set.is_empty());
    }

    #[test]
    fn serialization_is_deterministic() {
        let mut a = EnvSet::new();
        a.insert("B", "2").unwrap();
        a.insert("A", "1").unwrap();
        let mut b = EnvSet::new();
        b.insert("A", "1").unwrap();
        b.insert("B", "2").unwrap();
        assert_eq!(a.to_json(), b.to_json());
    }
}
