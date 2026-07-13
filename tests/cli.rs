//! End-to-end tests that drive the compiled `git-shadow` binary against
//! real git repositories in a temporary directory, with the config
//! directory isolated per test via the platform's config env var.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_git-shadow");
const PARENT_ORIGIN: &str = "git@github.com:example/parent.git";

struct TestEnv {
    root: tempfile::TempDir,
}

impl TestEnv {
    /// A parent repo with `PARENT_ORIGIN` as origin, plus a local bare
    /// repo (`shadow.git`) to clone shadows from.
    fn new() -> Self {
        let env = TestEnv {
            root: tempfile::tempdir().unwrap(),
        };

        let src = env.path("shadow-src");
        fs::create_dir(&src).unwrap();
        git(&src, &["init", "-q"]);
        git(
            &src,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "init",
            ],
        );
        git(
            env.root.path(),
            &[
                "clone",
                "-q",
                "--bare",
                src.to_str().unwrap(),
                env.shadow_remote().to_str().unwrap(),
            ],
        );

        let parent = env.parent();
        fs::create_dir(&parent).unwrap();
        git(&parent, &["init", "-q"]);
        git(&parent, &["remote", "add", "origin", PARENT_ORIGIN]);

        env
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn parent(&self) -> PathBuf {
        self.path("parent")
    }

    /// Local bare repo used as the shadow's remote; derives the default
    /// nickname "shadow".
    fn shadow_remote(&self) -> PathBuf {
        self.path("shadow.git")
    }

    /// Runs the binary in `dir` with the config directory redirected into
    /// this test's temp root.
    fn run_in(&self, dir: &Path, args: &[&str]) -> Output {
        let mut cmd = Command::new(BIN);
        cmd.args(args).current_dir(dir);

        let config_home = self.path("config-home");
        #[cfg(target_os = "windows")]
        cmd.env("APPDATA", &config_home);
        #[cfg(target_os = "macos")]
        cmd.env("HOME", &config_home);
        #[cfg(all(unix, not(target_os = "macos")))]
        cmd.env("XDG_CONFIG_HOME", &config_home);

        cmd.output().unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in(&self.parent(), args)
    }

    /// The config file the binary should use for the parent repo's origin.
    fn config_file(&self) -> PathBuf {
        #[cfg(target_os = "macos")]
        let base = self
            .path("config-home")
            .join("Library")
            .join("Application Support");
        #[cfg(not(target_os = "macos"))]
        let base = self.path("config-home");

        base.join("git-shadow")
            .join("github.com")
            .join("example")
            .join("parent")
            .join("config.toml")
    }

    fn exclude_file(&self) -> PathBuf {
        self.parent().join(".git").join("info").join("exclude")
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// `git status --porcelain` output for the parent repo.
fn parent_status(env: &TestEnv) -> String {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(env.parent())
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn no_arguments_prints_usage() {
    let env = TestEnv::new();

    let output = env.run(&[]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("Usage: git shadow"));
}

#[test]
fn help_flag_documents_the_subcommands() {
    let env = TestEnv::new();

    let output = env.run(&["--help"]);

    assert!(output.status.success());
    let out = stdout(&output);
    for subcommand in ["list", "sync", "clone", "remove"] {
        assert!(out.contains(subcommand), "help must mention '{subcommand}'");
    }
}

#[test]
fn shadow_name_without_git_command_prints_usage() {
    let env = TestEnv::new();

    let output = env.run(&["lonely-name"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("usage: git shadow <shadow-name> <git-command>"));
}

#[test]
fn list_without_config_reports_no_shadows() {
    let env = TestEnv::new();

    let output = env.run(&["list"]);

    assert!(output.status.success());
    assert!(
        stdout(&output).contains("no shadows configured for git@github.com:example/parent.git")
    );
}

#[test]
fn list_rejects_unknown_flags() {
    let env = TestEnv::new();

    let output = env.run(&["list", "--bogus"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("unexpected argument '--bogus'"));
}

#[test]
fn clone_registers_shadow_and_updates_exclude() {
    let env = TestEnv::new();

    let output = env.run(&["clone", env.shadow_remote().to_str().unwrap()]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("registered shadow 'shadow'"));
    assert!(out.contains("added '/shadow/'"));

    let config = fs::read_to_string(env.config_file()).unwrap();
    assert!(config.contains("version = 1"));
    assert!(config.contains("[shadows.shadow]"));
    assert!(config.contains("mapping = \"shadow/\""));

    let exclude = fs::read_to_string(env.exclude_file()).unwrap();
    assert!(exclude.contains("# >>> git-shadow (managed) >>>"));
    assert!(exclude.contains("/shadow/"));

    assert!(env.parent().join("shadow").join(".git").exists());
    assert_eq!(parent_status(&env), "", "shadow dir must be excluded");
}

#[test]
fn clone_refuses_duplicate_nickname() {
    let env = TestEnv::new();
    let remote = env.shadow_remote();

    assert!(
        env.run(&["clone", remote.to_str().unwrap()])
            .status
            .success()
    );
    let output = env.run(&["clone", remote.to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("shadow 'shadow' already exists"));
}

#[test]
fn clone_refuses_non_empty_target_directory() {
    let env = TestEnv::new();
    let busy = env.parent().join("busy");
    fs::create_dir(&busy).unwrap();
    fs::write(busy.join("file.txt"), "").unwrap();

    let output = env.run(&[
        "clone",
        env.shadow_remote().to_str().unwrap(),
        "busy",
        "--name",
        "busy",
    ]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("exists and is not empty"));
    assert!(!env.config_file().exists(), "config must not be created");
}

#[test]
fn clone_from_subdirectory_stores_mapping_relative_to_repo_root() {
    let env = TestEnv::new();
    let sub = env.parent().join("sub");
    fs::create_dir(&sub).unwrap();

    let output = env.run_in(
        &sub,
        &[
            "clone",
            env.shadow_remote().to_str().unwrap(),
            "vendor/fb",
            "--name",
            "fb",
        ],
    );

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let config = fs::read_to_string(env.config_file()).unwrap();
    assert!(config.contains("mapping = \"sub/vendor/fb/\""));
    assert!(env.parent().join("sub/vendor/fb/.git").exists());
}

#[test]
fn list_shows_registered_shadows() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.shadow_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["list"]);

    assert!(output.status.success());
    let out = stdout(&output);
    assert!(out.contains("shadow"));
    assert!(out.contains("shadow/"));
}

#[test]
fn sync_clones_missing_shadow_and_restores_exclude() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.shadow_remote().to_str().unwrap()])
            .status
            .success()
    );
    fs::remove_dir_all(env.parent().join("shadow")).unwrap();
    fs::remove_file(env.exclude_file()).unwrap();

    let output = env.run(&["sync"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("shadow: cloning"));
    assert!(env.parent().join("shadow").join(".git").exists());
    let exclude = fs::read_to_string(env.exclude_file()).unwrap();
    assert!(exclude.contains("/shadow/"));
}

#[test]
fn sync_reports_present_shadows_and_is_idempotent() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.shadow_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["sync"]);

    assert!(output.status.success());
    let out = stdout(&output);
    assert!(out.contains("shadow: already present"));
    assert!(!out.contains("updated exclude entries"));
}

#[test]
fn sync_warns_and_fails_on_non_repo_mapping() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.shadow_remote().to_str().unwrap()])
            .status
            .success()
    );
    let dir = env.parent().join("shadow");
    fs::remove_dir_all(&dir).unwrap();
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("unrelated.txt"), "").unwrap();

    let output = env.run(&["sync"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("is not a git repository"));
}

#[test]
fn remove_unregisters_but_keeps_directory() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.shadow_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["remove", "shadow"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let config = fs::read_to_string(env.config_file()).unwrap();
    assert!(!config.contains("[shadows.shadow]"));
    let exclude = fs::read_to_string(env.exclude_file()).unwrap();
    assert!(!exclude.contains("/shadow/"));
    assert!(env.parent().join("shadow").join(".git").exists());
}

#[test]
fn rm_alias_with_delete_removes_directory() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.shadow_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["rm", "shadow", "--delete"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!env.parent().join("shadow").exists());
}

#[test]
fn remove_unknown_shadow_fails() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.shadow_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["remove", "nope"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("shadow 'nope' not found"));
}

#[test]
fn passthrough_runs_git_inside_the_shadow() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.shadow_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["shadow", "rev-parse", "--show-toplevel"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let toplevel = PathBuf::from(stdout(&output).trim().to_string());
    assert_eq!(
        toplevel.file_name().unwrap().to_str().unwrap(),
        "shadow",
        "git must run inside the shadow repo, got {}",
        toplevel.display()
    );
}

#[test]
fn passthrough_fails_for_unknown_shadow() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.shadow_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["nope", "status"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("shadow 'nope' not found"));
}
