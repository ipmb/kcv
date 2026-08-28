//! Drives the real binary against a throwaway keychain.

use std::path::PathBuf;
use std::process::Command;

struct TempKeychain {
    path: PathBuf,
}

impl TempKeychain {
    fn create(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "kcv-cli-{}-{}.keychain-db",
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

    fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_kcv"));
        c.env("KCV_KEYCHAIN", &self.path);
        c.env_remove("KCV_ENV");
        c
    }
}

impl Drop for TempKeychain {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[test]
fn set_then_exec_injects_the_variable() {
    let kc = TempKeychain::create("e2e");

    let status = kc
        .cmd()
        .args(["--environment", "prod", "set", "GREETING=hello"])
        .status()
        .unwrap();
    assert!(status.success(), "set should succeed");

    let out = kc
        .cmd()
        .args([
            "--environment",
            "prod",
            "exec",
            "--",
            "sh",
            "-c",
            "printf %s \"$GREETING\"",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello");
}

#[test]
fn set_never_prints_the_value() {
    let kc = TempKeychain::create("quiet");
    let out = kc
        .cmd()
        .args(["-e", "prod", "set", "TOKEN=sup3rs3cret"])
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !combined.contains("sup3rs3cret"),
        "value leaked: {combined}"
    );
    assert!(combined.contains("prod"), "should name the environment");
}

#[test]
fn exec_passes_through_the_child_exit_code() {
    let kc = TempKeychain::create("exit");
    kc.cmd()
        .args(["-e", "prod", "set", "X=1"])
        .status()
        .unwrap();
    let status = kc
        .cmd()
        .args(["-e", "prod", "exec", "--", "sh", "-c", "exit 42"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(42));
}

#[test]
fn exec_forwards_arguments_that_look_like_flags() {
    let kc = TempKeychain::create("flags");
    kc.cmd()
        .args(["-e", "prod", "set", "X=1"])
        .status()
        .unwrap();
    let out = kc
        .cmd()
        .args([
            "-e", "prod", "exec", "--", "printf", "%s|%s", "--port", "8000",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "--port|8000");
}

#[test]
fn a_missing_environment_fails_with_a_useful_message() {
    let kc = TempKeychain::create("missing");
    let out = kc
        .cmd()
        .args(["-e", "nope", "exec", "--", "true"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nope"), "stderr was: {stderr}");
}

#[test]
fn a_missing_command_exits_127() {
    let kc = TempKeychain::create("enoent");
    kc.cmd()
        .args(["-e", "prod", "set", "X=1"])
        .status()
        .unwrap();
    let status = kc
        .cmd()
        .args(["-e", "prod", "exec", "--", "definitely-not-a-real-command"])
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(127));
}

#[test]
fn kcv_env_supplies_the_environment_name() {
    let kc = TempKeychain::create("envvar");
    kc.cmd()
        .args(["-e", "staging", "set", "WHO=staging"])
        .status()
        .unwrap();
    let out = kc
        .cmd()
        .env("KCV_ENV", "staging")
        .args(["exec", "--", "sh", "-c", "printf %s \"$WHO\""])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "staging");
}

#[test]
fn a_value_can_be_piped_in_without_an_equals_sign() {
    use std::io::Write;
    use std::process::Stdio;
    let kc = TempKeychain::create("piped");
    let mut child = kc
        .cmd()
        .args(["-e", "prod", "set", "PIPED"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"from-stdin\n")
        .unwrap();
    assert!(child.wait().unwrap().success());

    let out = kc
        .cmd()
        .args([
            "-e",
            "prod",
            "exec",
            "--",
            "sh",
            "-c",
            "printf %s \"$PIPED\"",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "from-stdin");
}

#[test]
fn omitting_the_environment_entirely_is_a_usage_error() {
    let kc = TempKeychain::create("noenv");
    let out = kc.cmd().args(["set", "X=1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn a_multiline_value_survives_the_round_trip() {
    let kc = TempKeychain::create("multiline");
    kc.cmd()
        .args(["-e", "prod", "set", "PEM=line1\nline2\nline3"])
        .status()
        .unwrap();
    let out = kc
        .cmd()
        .args(["-e", "prod", "exec", "--", "sh", "-c", "printf %s \"$PEM\""])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "line1\nline2\nline3");
}

fn write_env_file(tag: &str, contents: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("kcv-cli-import-{}-{}.env", tag, std::process::id()));
    std::fs::write(&p, contents).unwrap();
    p
}

#[test]
fn import_loads_a_dotenv_file_and_exec_reads_it_back() {
    let kc = TempKeychain::create("import");
    let f = write_env_file(
        "basic",
        "# a comment\nexport GREETING=hello\nQUOTED=\"spaced value\"\nURL=https://x/?a=b\n",
    );

    let out = kc
        .cmd()
        .args(["-e", "myproject", "import"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Imported 3 variables"), "{stderr}");

    let out = kc
        .cmd()
        .args([
            "-e",
            "myproject",
            "exec",
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s' \"$GREETING\" \"$QUOTED\" \"$URL\"",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "hello|spaced value|https://x/?a=b"
    );
    std::fs::remove_file(&f).ok();
}

#[test]
fn import_keeps_the_file_when_there_is_no_terminal_to_ask() {
    let kc = TempKeychain::create("importkeep");
    let f = write_env_file("keep", "FOO=bar\n");
    let out = kc
        .cmd()
        .args(["-e", "myproject", "import"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(f.exists(), "the file must survive when nobody was asked");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Kept"), "{stderr}");
    std::fs::remove_file(&f).ok();
}

#[test]
fn import_never_prints_a_value() {
    let kc = TempKeychain::create("importquiet");
    let f = write_env_file("quiet", "TOKEN=sup3rs3cret\n");
    let out = kc
        .cmd()
        .args(["-e", "myproject", "import"])
        .arg(&f)
        .output()
        .unwrap();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !combined.contains("sup3rs3cret"),
        "value leaked: {combined}"
    );
    std::fs::remove_file(&f).ok();
}

#[test]
fn a_malformed_file_fails_and_names_the_line() {
    let kc = TempKeychain::create("importbad");
    let f = write_env_file("bad", "GOOD=1\nthis line has no equals\n");
    let out = kc
        .cmd()
        .args(["-e", "myproject", "import"])
        .arg(&f)
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(":2:"), "{stderr}");
    assert!(f.exists(), "a failed import must not delete the file");

    // Nothing should have been stored.
    let out = kc
        .cmd()
        .args(["-e", "myproject", "exec", "--", "true"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "the environment should not exist");
    std::fs::remove_file(&f).ok();
}

#[test]
fn a_multiline_quoted_value_survives_import() {
    let kc = TempKeychain::create("importpem");
    let f = write_env_file("pem", "PEM=\"-----BEGIN-----\nmiddle\n-----END-----\"\n");
    kc.cmd()
        .args(["-e", "myproject", "import"])
        .arg(&f)
        .status()
        .unwrap();
    let out = kc
        .cmd()
        .args([
            "-e",
            "myproject",
            "exec",
            "--",
            "sh",
            "-c",
            "printf %s \"$PEM\"",
        ])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "-----BEGIN-----\nmiddle\n-----END-----"
    );
    std::fs::remove_file(&f).ok();
}
