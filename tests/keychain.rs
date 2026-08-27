//! Exercises KeychainStore against a throwaway keychain file, so the
//! developer's login keychain is never touched and no GUI prompt appears.

use kcv::store::{KeychainStore, Store};
use std::path::PathBuf;

struct TempKeychain {
    path: PathBuf,
}

impl TempKeychain {
    fn create(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "kcv-test-{}-{}.keychain-db",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        security_framework::os::macos::keychain::CreateOptions::new()
            .password("test-password")
            .create(&path)
            .expect("create temp keychain");
        Self { path }
    }
}

impl Drop for TempKeychain {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn round_trips_a_payload_through_a_real_keychain() {
    let kc = TempKeychain::create("roundtrip");
    let store = KeychainStore::at(&kc.path).expect("open temp keychain");

    assert_eq!(
        store.load("prod").unwrap(),
        None,
        "absent item reads as None"
    );

    store.save("prod", br#"{"FOO":"bar"}"#).unwrap();
    assert_eq!(
        store.load("prod").unwrap(),
        Some(br#"{"FOO":"bar"}"#.to_vec())
    );

    store.save("prod", br#"{"FOO":"updated"}"#).unwrap();
    assert_eq!(
        store.load("prod").unwrap(),
        Some(br#"{"FOO":"updated"}"#.to_vec()),
        "save must update an existing item, not add a duplicate"
    );

    store.save("dev", br#"{"OTHER":"x"}"#).unwrap();
    assert_eq!(
        store.load("prod").unwrap(),
        Some(br#"{"FOO":"updated"}"#.to_vec()),
        "environments must not collide"
    );
}

#[test]
fn stores_values_with_newlines_and_unicode() {
    let kc = TempKeychain::create("unicode");
    let store = KeychainStore::at(&kc.path).expect("open temp keychain");
    let payload = r#"{"PEM":"-----BEGIN\nline2\n-----END","EMOJI":"café ☕"}"#;
    store.save("prod", payload.as_bytes()).unwrap();
    assert_eq!(
        store.load("prod").unwrap(),
        Some(payload.as_bytes().to_vec())
    );
}
