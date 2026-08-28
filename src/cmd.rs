use crate::envset::EnvSet;
use crate::error::{Error, Result};
use crate::prompt::read_secret;
use crate::store::Store;
use std::collections::BTreeMap;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Splits `KEY=VALUE` on the first '=' only, so values may contain '='.
/// An argument with no '=' yields `None`, meaning "prompt for this one".
pub fn parse_assignment(arg: &str) -> (String, Option<String>) {
    match arg.split_once('=') {
        Some((key, value)) => (key.to_string(), Some(value.to_string())),
        None => (arg.to_string(), None),
    }
}

pub fn validate_environment(name: &str) -> Result<()> {
    if name.is_empty() || name.contains('\0') {
        return Err(Error::InvalidEnvironmentName(name.to_string()));
    }
    Ok(())
}

/// Reads and parses an environment, or `Ok(None)` when it does not exist.
pub fn load_env_set(store: &dyn Store, environment: &str) -> Result<Option<EnvSet>> {
    match store.load(environment)? {
        Some(bytes) => Ok(Some(EnvSet::from_json(&bytes, environment)?)),
        None => Ok(None),
    }
}

/// Merges assignments into an environment and returns how many keys were
/// written. A single load and a single save keeps this to one authorization.
pub fn set(store: &dyn Store, environment: &str, assignments: &[String]) -> Result<usize> {
    validate_environment(environment)?;

    // Resolve every assignment before touching the store, so a bad key or a
    // failed prompt cannot leave a half-applied write behind.
    let mut resolved: Vec<(String, String)> = Vec::with_capacity(assignments.len());
    for arg in assignments {
        let (key, value) = parse_assignment(arg);
        crate::envset::validate_key(&key)?;
        let value = match value {
            Some(v) => v,
            None => read_secret(&key)?,
        };
        resolved.push((key, value));
    }

    let mut env_set = load_env_set(store, environment)?.unwrap_or_default();
    for (key, value) in &resolved {
        env_set.insert(key, value)?;
    }
    store.save(environment, &env_set.to_json())?;
    Ok(resolved.len())
}

/// Reads a `.env` file and merges it into an environment, returning how many
/// variables were written. Like `set`, everything is resolved before anything
/// is stored, so a malformed file leaves the environment untouched.
///
/// Deleting the source file is deliberately not done here. That is a
/// destructive act needing a human answer, so it lives with the caller.
pub fn import(store: &dyn Store, environment: &str, path: &std::path::Path) -> Result<usize> {
    validate_environment(environment)?;
    let display = path.display().to_string();

    let text = std::fs::read_to_string(path).map_err(|source| Error::ReadFile {
        path: display.clone(),
        source,
    })?;

    let pairs = crate::dotenv::parse(&text).map_err(|e| Error::Dotenv {
        path: display.clone(),
        line: e.line,
        reason: e.reason,
    })?;

    // Importing nothing and then offering to delete the file would be a good
    // way to lose a file for no gain.
    if pairs.is_empty() {
        return Err(Error::EmptyImport(display));
    }

    let mut env_set = load_env_set(store, environment)?.unwrap_or_default();
    for (key, value) in &pairs {
        env_set.insert(key, value)?;
    }
    store.save(environment, &env_set.to_json())?;
    Ok(pairs.len())
}

/// What `exec` will run: the program, its arguments, and the complete
/// environment the child receives.
#[derive(Debug, PartialEq, Eq)]
pub struct ExecPlan {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

impl ExecPlan {
    /// Looks up a variable in the resolved child environment.
    pub fn env_value(&self, key: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Works out what to execute and with which environment, given an explicit
/// base environment. Taking the base as a parameter keeps the tests from
/// mutating the process environment, which would race under a parallel runner.
pub fn resolve_exec_with_base(
    store: &dyn Store,
    environment: &str,
    argv: &[String],
    base: Vec<(String, String)>,
) -> Result<ExecPlan> {
    validate_environment(environment)?;
    let (program, args) = argv.split_first().ok_or(Error::NoCommand)?;

    let env_set = load_env_set(store, environment)?
        .ok_or_else(|| Error::EnvironmentNotFound(environment.to_string()))?;

    // Inherit the caller's environment, then overlay the stored secrets so a
    // stored variable wins over one already present in the shell.
    let mut env: BTreeMap<String, String> = base.into_iter().collect();
    for (key, value) in env_set.iter() {
        env.insert(key.clone(), value.clone());
    }

    Ok(ExecPlan {
        program: program.clone(),
        args: args.to_vec(),
        env: env.into_iter().collect(),
    })
}

/// As `resolve_exec_with_base`, using this process's environment as the base.
pub fn resolve_exec(store: &dyn Store, environment: &str, argv: &[String]) -> Result<ExecPlan> {
    resolve_exec_with_base(store, environment, argv, std::env::vars().collect())
}

/// Replaces this process with the command. Returns only on failure: on
/// success the child inherits our PID, stdio, controlling terminal, process
/// group and session, which is what keeps TUIs and job control working.
pub fn exec(
    store: &dyn Store,
    environment: &str,
    argv: &[String],
) -> Result<std::convert::Infallible> {
    let plan = resolve_exec(store, environment, argv)?;
    let error = Command::new(&plan.program)
        .args(&plan.args)
        .env_clear()
        .envs(plan.env)
        .exec();
    Err(Error::Exec {
        program: plan.program,
        source: error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemStore;

    fn base() -> Vec<(String, String)> {
        vec![
            ("PATH".to_string(), "/usr/bin".to_string()),
            ("HOME".to_string(), "/Users/test".to_string()),
        ]
    }

    #[test]
    fn splits_on_the_first_equals_only() {
        assert_eq!(
            parse_assignment("URL=https://x/?a=b"),
            ("URL".to_string(), Some("https://x/?a=b".to_string()))
        );
    }

    #[test]
    fn an_empty_value_is_legal() {
        assert_eq!(
            parse_assignment("EMPTY="),
            ("EMPTY".to_string(), Some(String::new()))
        );
    }

    #[test]
    fn an_argument_without_equals_has_no_value() {
        assert_eq!(parse_assignment("FOO"), ("FOO".to_string(), None));
    }

    #[test]
    fn set_creates_a_new_environment() {
        let store = MemStore::new();
        let n = set(&store, "prod", &["FOO=bar".to_string()]).unwrap();
        assert_eq!(n, 1);
        let loaded = load_env_set(&store, "prod").unwrap().unwrap();
        assert_eq!(loaded.iter().next().unwrap().1, "bar");
    }

    #[test]
    fn set_merges_and_leaves_other_keys_alone() {
        let store = MemStore::new();
        set(&store, "prod", &["KEEP=untouched".to_string()]).unwrap();
        set(&store, "prod", &["NEW=added".to_string()]).unwrap();
        let loaded = load_env_set(&store, "prod").unwrap().unwrap();
        let map: BTreeMap<_, _> = loaded.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        assert_eq!(map["KEEP"], "untouched");
        assert_eq!(map["NEW"], "added");
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn set_overwrites_an_existing_key() {
        let store = MemStore::new();
        set(&store, "prod", &["FOO=old".to_string()]).unwrap();
        set(&store, "prod", &["FOO=new".to_string()]).unwrap();
        let loaded = load_env_set(&store, "prod").unwrap().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.iter().next().unwrap().1, "new");
    }

    #[test]
    fn last_value_wins_within_one_invocation() {
        let store = MemStore::new();
        set(
            &store,
            "prod",
            &["FOO=first".to_string(), "FOO=second".to_string()],
        )
        .unwrap();
        let loaded = load_env_set(&store, "prod").unwrap().unwrap();
        assert_eq!(loaded.iter().next().unwrap().1, "second");
    }

    #[test]
    fn set_refuses_to_clobber_a_corrupt_item() {
        let store = MemStore::new();
        store.save("prod", b"definitely not json").unwrap();
        assert!(matches!(
            set(&store, "prod", &["FOO=bar".to_string()]),
            Err(Error::CorruptItem(_))
        ));
        assert_eq!(
            store.load("prod").unwrap(),
            Some(b"definitely not json".to_vec()),
            "the original bytes must survive"
        );
    }

    #[test]
    fn set_rejects_a_bad_key_before_writing_anything() {
        let store = MemStore::new();
        assert!(set(&store, "prod", &["=novalue".to_string()]).is_err());
        assert_eq!(store.load("prod").unwrap(), None, "nothing was written");
    }

    #[test]
    fn environment_names_are_validated() {
        assert!(validate_environment("prod").is_ok());
        assert!(matches!(
            validate_environment(""),
            Err(Error::InvalidEnvironmentName(_))
        ));
        assert!(validate_environment("has\0nul").is_err());
    }

    fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("kcv-import-{}-{}.env", name, std::process::id()));
        std::fs::write(&p, contents).unwrap();
        p
    }

    #[test]
    fn import_creates_an_environment() {
        let store = MemStore::new();
        let f = write_temp("create", "FOO=bar\nBAZ=qux\n");
        assert_eq!(import(&store, "myproject", &f).unwrap(), 2);
        let loaded = load_env_set(&store, "myproject").unwrap().unwrap();
        let map: BTreeMap<_, _> = loaded.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        assert_eq!(map["FOO"], "bar");
        assert_eq!(map["BAZ"], "qux");
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn import_merges_with_existing_variables() {
        let store = MemStore::new();
        set(&store, "myproject", &["KEEP=untouched".to_string()]).unwrap();
        let f = write_temp("merge", "ADDED=new\n");
        import(&store, "myproject", &f).unwrap();
        let loaded = load_env_set(&store, "myproject").unwrap().unwrap();
        let map: BTreeMap<_, _> = loaded.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        assert_eq!(map["KEEP"], "untouched");
        assert_eq!(map["ADDED"], "new");
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn import_overwrites_a_key_that_already_exists() {
        let store = MemStore::new();
        set(&store, "myproject", &["FOO=old".to_string()]).unwrap();
        let f = write_temp("overwrite", "FOO=new\n");
        import(&store, "myproject", &f).unwrap();
        let loaded = load_env_set(&store, "myproject").unwrap().unwrap();
        assert_eq!(loaded.iter().next().unwrap().1, "new");
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn a_malformed_file_writes_nothing() {
        let store = MemStore::new();
        let f = write_temp("malformed", "GOOD=1\ngarbage\n");
        let err = import(&store, "myproject", &f).unwrap_err();
        assert!(matches!(err, Error::Dotenv { line: 2, .. }));
        assert!(err.to_string().contains(":2:"), "{err}");
        assert_eq!(
            store.load("myproject").unwrap(),
            None,
            "nothing was written"
        );
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn an_empty_file_is_an_error() {
        let store = MemStore::new();
        let f = write_temp("empty", "# only a comment\n\n");
        assert!(matches!(
            import(&store, "myproject", &f),
            Err(Error::EmptyImport(_))
        ));
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn a_missing_file_names_the_path() {
        let store = MemStore::new();
        let missing = std::path::Path::new("/nonexistent/kcv/does-not-exist.env");
        let err = import(&store, "myproject", missing).unwrap_err();
        assert!(matches!(err, Error::ReadFile { .. }));
        assert!(err.to_string().contains("does-not-exist.env"), "{err}");
    }

    #[test]
    fn import_leaves_the_source_file_alone() {
        let store = MemStore::new();
        let f = write_temp("keepfile", "FOO=bar\n");
        import(&store, "myproject", &f).unwrap();
        assert!(f.exists(), "import must not delete the file itself");
        std::fs::remove_file(&f).ok();
    }

    #[test]
    fn resolve_exec_splits_program_from_arguments() {
        let store = MemStore::new();
        set(&store, "prod", &["FOO=bar".to_string()]).unwrap();
        let argv = vec![
            "./server".to_string(),
            "--port".to_string(),
            "8000".to_string(),
        ];
        let plan = resolve_exec_with_base(&store, "prod", &argv, base()).unwrap();
        assert_eq!(plan.program, "./server");
        assert_eq!(plan.args, vec!["--port".to_string(), "8000".to_string()]);
    }

    #[test]
    fn secrets_are_present_in_the_child_environment() {
        let store = MemStore::new();
        set(&store, "prod", &["SECRET=s3cr3t".to_string()]).unwrap();
        let argv = vec!["true".to_string()];
        let plan = resolve_exec_with_base(&store, "prod", &argv, base()).unwrap();
        assert_eq!(plan.env_value("SECRET"), Some("s3cr3t"));
    }

    #[test]
    fn the_inherited_environment_is_preserved() {
        let store = MemStore::new();
        set(&store, "prod", &["SECRET=s".to_string()]).unwrap();
        let argv = vec!["true".to_string()];
        let plan = resolve_exec_with_base(&store, "prod", &argv, base()).unwrap();
        assert_eq!(plan.env_value("PATH"), Some("/usr/bin"));
        assert_eq!(plan.env_value("HOME"), Some("/Users/test"));
    }

    #[test]
    fn stored_values_win_over_inherited_ones() {
        let store = MemStore::new();
        set(&store, "prod", &["PATH=/from/kcv".to_string()]).unwrap();
        let argv = vec!["true".to_string()];
        let plan = resolve_exec_with_base(&store, "prod", &argv, base()).unwrap();
        assert_eq!(plan.env_value("PATH"), Some("/from/kcv"));
        assert_eq!(
            plan.env.iter().filter(|(k, _)| k == "PATH").count(),
            1,
            "the variable must appear exactly once"
        );
    }

    #[test]
    fn resolve_exec_uses_the_process_environment_by_default() {
        let store = MemStore::new();
        set(&store, "prod", &["SECRET=s".to_string()]).unwrap();
        let argv = vec!["true".to_string()];
        let plan = resolve_exec(&store, "prod", &argv).unwrap();
        assert!(plan.env_value("PATH").is_some(), "PATH must be inherited");
    }

    #[test]
    fn a_missing_environment_is_an_error_not_an_empty_set() {
        let store = MemStore::new();
        let argv = vec!["true".to_string()];
        assert!(matches!(
            resolve_exec_with_base(&store, "absent", &argv, base()),
            Err(Error::EnvironmentNotFound(_))
        ));
    }

    #[test]
    fn an_empty_command_is_a_usage_error() {
        let store = MemStore::new();
        set(&store, "prod", &["FOO=bar".to_string()]).unwrap();
        let err = resolve_exec_with_base(&store, "prod", &[], base()).unwrap_err();
        assert!(matches!(err, Error::NoCommand));
        assert_eq!(err.exit_code(), 2);
    }
}
