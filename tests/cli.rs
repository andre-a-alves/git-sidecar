//! End-to-end tests that drive the compiled `git-sidecar` binary against
//! real git repositories in a temporary directory, with the config
//! directory isolated per test via the platform's config env var.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_git-sidecar");
const PARENT_ORIGIN: &str = "git@github.com:example/parent.git";

struct TestEnv {
    root: tempfile::TempDir,
}

impl TestEnv {
    /// A parent repo with `PARENT_ORIGIN` as origin, plus a local bare
    /// repo (`sidecar.git`) to clone sidecars from.
    fn new() -> Self {
        let env = TestEnv {
            root: tempfile::tempdir().unwrap(),
        };

        let src = env.path("sidecar-src");
        fs::create_dir(&src).unwrap();
        git(&src, &["init", "-q"]);
        fs::write(src.join("README.md"), "sidecar readme\n").unwrap();
        git(&src, &["add", "README.md"]);
        git(
            &src,
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
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
                env.sidecar_remote().to_str().unwrap(),
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

    /// Local bare repo used as the sidecar's remote; derives the default
    /// nickname "sidecar".
    fn sidecar_remote(&self) -> PathBuf {
        self.path("sidecar.git")
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

        base.join("git-sidecar")
            .join("github.com")
            .join("example")
            .join("parent")
            .join("config.toml")
    }

    fn exclude_file(&self) -> PathBuf {
        self.parent().join(".git").join("info").join("exclude")
    }

    /// Where the unified layout keeps a sidecar's git directory.
    fn sidecar_gitdir(&self, name: &str) -> PathBuf {
        self.parent()
            .join(".git")
            .join("git-sidecar")
            .join(name)
            .join("gitdir")
    }

    /// Writes the parent repo's config file directly, bypassing the binary.
    fn write_config(&self, body: &str) {
        let path = self.config_file();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    /// Clones the shared remote into `dir` with plain git and registers it
    /// in the config by hand, simulating a sidecar checked out before the
    /// unified layout existed (git dir at `<mapping>/.git`, no
    /// `standalone` key).
    fn old_layout_sidecar(&self, name: &str) {
        git(
            &self.parent(),
            &["clone", "-q", self.sidecar_remote().to_str().unwrap(), name],
        );

        let entry = format!(
            "[sidecars.{name}]\nrepo = '{}'\nmapping = \"{name}/\"\n",
            self.sidecar_remote().display()
        );
        let config = if self.config_file().exists() {
            format!(
                "{}\n{entry}",
                fs::read_to_string(self.config_file()).unwrap()
            )
        } else {
            format!("version = 1\n\n{entry}")
        };
        self.write_config(&config);
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

/// Runs plain git in `dir` and captures its output, for asserting what
/// bare git (not the binary under test) sees from a given directory.
fn git_output(dir: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
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
    assert!(stderr(&output).contains("Usage: git sidecar"));
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
fn sidecar_name_without_git_command_prints_usage() {
    let env = TestEnv::new();

    let output = env.run(&["lonely-name"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("usage: git sidecar [<sidecar-name>] <git-command>"));
}

#[test]
fn list_without_config_reports_no_sidecars() {
    let env = TestEnv::new();

    let output = env.run(&["list"]);

    assert!(output.status.success());
    assert!(
        stdout(&output).contains("no sidecars configured for git@github.com:example/parent.git")
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
fn clone_registers_sidecar_and_updates_exclude() {
    let env = TestEnv::new();

    let output = env.run(&["clone", env.sidecar_remote().to_str().unwrap()]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("registered sidecar 'sidecar'"));
    assert!(out.contains("added '/sidecar/'"));

    let config = fs::read_to_string(env.config_file()).unwrap();
    assert!(config.contains("version = 1"));
    assert!(config.contains("[sidecars.sidecar]"));
    assert!(config.contains("mapping = \"sidecar/\""));

    let exclude = fs::read_to_string(env.exclude_file()).unwrap();
    assert!(exclude.contains("# >>> git-sidecar (managed) >>>"));
    assert!(exclude.contains("/sidecar/"));

    // unified layout: no .git in the mapping dir, git dir relocated
    assert!(!env.parent().join("sidecar").join(".git").exists());
    assert!(env.sidecar_gitdir("sidecar").join("HEAD").is_file());
    assert!(env.parent().join("sidecar").join("README.md").is_file());
    assert_eq!(parent_status(&env), "", "sidecar dir must be excluded");
}

#[test]
fn clone_refuses_duplicate_nickname() {
    let env = TestEnv::new();
    let remote = env.sidecar_remote();

    assert!(
        env.run(&["clone", remote.to_str().unwrap()])
            .status
            .success()
    );
    let output = env.run(&["clone", remote.to_str().unwrap()]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("sidecar 'sidecar' already exists"));
}

#[test]
fn clone_refuses_non_empty_target_directory() {
    let env = TestEnv::new();
    let busy = env.parent().join("busy");
    fs::create_dir(&busy).unwrap();
    fs::write(busy.join("file.txt"), "").unwrap();

    let output = env.run(&[
        "clone",
        env.sidecar_remote().to_str().unwrap(),
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
            env.sidecar_remote().to_str().unwrap(),
            "vendor/fb",
            "--name",
            "fb",
        ],
    );

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let config = fs::read_to_string(env.config_file()).unwrap();
    assert!(config.contains("mapping = \"sub/vendor/fb/\""));
    assert!(!env.parent().join("sub/vendor/fb/.git").exists());
    assert!(env.sidecar_gitdir("fb").join("HEAD").is_file());
}

#[test]
fn list_shows_registered_sidecars() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["list"]);

    assert!(output.status.success());
    let out = stdout(&output);
    assert!(out.contains("sidecar"));
    assert!(out.contains("sidecar/"));
}

#[test]
fn sync_clones_missing_sidecar_and_restores_exclude() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );
    fs::remove_dir_all(env.parent().join("sidecar")).unwrap();
    fs::remove_dir_all(env.parent().join(".git").join("git-sidecar")).unwrap();
    fs::remove_file(env.exclude_file()).unwrap();

    let output = env.run(&["sync"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("sidecar: cloning"));
    assert!(!env.parent().join("sidecar").join(".git").exists());
    assert!(env.sidecar_gitdir("sidecar").join("HEAD").is_file());
    let exclude = fs::read_to_string(env.exclude_file()).unwrap();
    assert!(exclude.contains("/sidecar/"));
}

#[test]
fn sync_restores_a_deleted_working_tree_from_the_external_gitdir() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );
    fs::remove_dir_all(env.parent().join("sidecar")).unwrap();

    let output = env.run(&["sync"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("sidecar: restoring working tree"));
    assert!(env.parent().join("sidecar").join("README.md").is_file());
    assert!(!env.parent().join("sidecar").join(".git").exists());
}

#[test]
fn sync_reports_present_sidecars_and_is_idempotent() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["sync"]);

    assert!(output.status.success());
    let out = stdout(&output);
    assert!(out.contains("sidecar: already present"));
    assert!(!out.contains("updated exclude entries"));
}

#[test]
fn sync_warns_and_fails_on_non_repo_mapping() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );
    let dir = env.parent().join("sidecar");
    fs::remove_dir_all(&dir).unwrap();
    fs::remove_dir_all(env.parent().join(".git").join("git-sidecar")).unwrap();
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
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["remove", "sidecar"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let config = fs::read_to_string(env.config_file()).unwrap();
    assert!(!config.contains("[sidecars.sidecar]"));
    let exclude = fs::read_to_string(env.exclude_file()).unwrap();
    assert!(!exclude.contains("/sidecar/"));
    // the kept directory is reattached to its git dir as a standalone repo
    assert!(env.parent().join("sidecar").join(".git").exists());
    assert!(!env.sidecar_gitdir("sidecar").exists());
    let toplevel = git_output(&env.parent().join("sidecar"), &["log", "--oneline"]);
    assert!(
        toplevel.status.success(),
        "kept directory must still be a working repo"
    );
}

#[test]
fn rm_alias_with_delete_removes_directory() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["rm", "sidecar", "--delete"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!env.parent().join("sidecar").exists());
    assert!(!env.sidecar_gitdir("sidecar").exists());
}

#[test]
fn remove_unknown_sidecar_fails() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["remove", "nope"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("sidecar 'nope' not found"));
}

#[test]
fn passthrough_runs_git_inside_the_sidecar() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["sidecar", "rev-parse", "--show-toplevel"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let toplevel = PathBuf::from(stdout(&output).trim().to_string());
    assert_eq!(
        toplevel.file_name().unwrap().to_str().unwrap(),
        "sidecar",
        "git must run inside the sidecar repo, got {}",
        toplevel.display()
    );
}

#[test]
fn passthrough_fails_for_unknown_sidecar() {
    let env = TestEnv::new();
    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );

    let output = env.run(&["nope", "status"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("sidecar 'nope' not found"));
}

/// Sets up a parent with one sidecar cloned by the binary (unified layout
/// by default) and returns its mapping directory.
fn cloned_env(extra_clone_args: &[&str]) -> (TestEnv, PathBuf) {
    let env = TestEnv::new();
    let mut args = vec![
        "clone".to_string(),
        env.sidecar_remote().to_str().unwrap().to_string(),
    ];
    args.extend(extra_clone_args.iter().map(|s| (*s).to_string()));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = env.run(&args);
    assert!(output.status.success(), "clone failed: {}", stderr(&output));
    let dir = env.parent().join("sidecar");
    (env, dir)
}

#[test]
fn bare_git_inside_sidecar_operates_on_the_parent() {
    let (env, sidecar_dir) = cloned_env(&[]);

    let output = git_output(&sidecar_dir, &["rev-parse", "--show-toplevel"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let toplevel = PathBuf::from(stdout(&output).trim().to_string());
    assert_eq!(
        toplevel.canonicalize().unwrap(),
        env.parent().canonicalize().unwrap(),
        "bare git inside a unified sidecar must discover the parent repo"
    );
}

#[test]
fn bare_git_inside_sidecar_sees_parent_status_with_sidecar_excluded() {
    let (env, sidecar_dir) = cloned_env(&[]);
    fs::write(sidecar_dir.join("scratch.txt"), "changed\n").unwrap();

    // the exclude entry hides the whole sidecar dir from the parent status
    let status = git_output(&sidecar_dir, &["status", "--porcelain"]);
    assert!(status.status.success());
    assert_eq!(stdout(&status), "", "sidecar contents must be excluded");

    // with --ignored the parent reports the sidecar dir as ignored
    let ignored = git_output(&sidecar_dir, &["status", "--porcelain", "--ignored"]);
    assert!(ignored.status.success());
    assert!(
        stdout(&ignored).contains("!! sidecar/"),
        "parent must see the sidecar dir as ignored, got: {}",
        stdout(&ignored)
    );

    // a change in the parent proper is visible from inside the sidecar
    fs::write(env.parent().join("parent-file.txt"), "new\n").unwrap();
    let status = git_output(&sidecar_dir, &["status", "--porcelain"]);
    assert!(stdout(&status).contains("?? parent-file.txt"));
}

#[test]
fn passthrough_without_name_inside_sidecar_targets_that_sidecar() {
    let (env, sidecar_dir) = cloned_env(&[]);

    let no_name = env.run_in(&sidecar_dir, &["log", "--oneline"]);
    let explicit = env.run(&["sidecar", "log", "--oneline"]);

    assert!(no_name.status.success(), "stderr: {}", stderr(&no_name));
    assert!(explicit.status.success(), "stderr: {}", stderr(&explicit));
    assert!(!stdout(&no_name).is_empty());
    assert_eq!(
        stdout(&no_name),
        stdout(&explicit),
        "the no-name form must behave exactly like the explicit-name form"
    );
}

#[test]
fn passthrough_with_redundant_explicit_name_inside_that_sidecar_matches() {
    let (env, sidecar_dir) = cloned_env(&[]);

    let redundant = env.run_in(&sidecar_dir, &["sidecar", "log", "--oneline"]);
    let no_name = env.run_in(&sidecar_dir, &["log", "--oneline"]);

    assert!(redundant.status.success(), "stderr: {}", stderr(&redundant));
    assert_eq!(stdout(&redundant), stdout(&no_name));
}

#[test]
fn explicit_name_wins_over_the_sidecar_containing_cwd() {
    let (env, sidecar_dir) = cloned_env(&[]);
    let output = env.run(&[
        "clone",
        env.sidecar_remote().to_str().unwrap(),
        "second",
        "--name",
        "second",
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let output = env.run_in(&sidecar_dir, &["second", "rev-parse", "--show-toplevel"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    let toplevel = PathBuf::from(stdout(&output).trim().to_string());
    assert_eq!(
        toplevel.file_name().unwrap().to_str().unwrap(),
        "second",
        "explicit name must win over cwd, got {}",
        toplevel.display()
    );
}

#[test]
fn passthrough_without_name_outside_a_sidecar_fails() {
    let (env, _) = cloned_env(&[]);

    let output = env.run(&["status"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("not inside a sidecar"));
    assert!(stderr(&output).contains("usage: git sidecar"));
}

#[test]
fn sync_relocates_old_layout_sidecar_in_place() {
    let env = TestEnv::new();
    env.old_layout_sidecar("legacy");
    let dir = env.parent().join("legacy");
    assert!(dir.join(".git").is_dir());

    let output = env.run(&["sync", "legacy"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("legacy: moving git dir"));
    assert!(!dir.join(".git").exists());
    assert!(env.sidecar_gitdir("legacy").join("HEAD").is_file());

    // history intact, and log/status/commit still work via passthrough
    let log = env.run(&["legacy", "log", "--oneline"]);
    assert!(log.status.success(), "stderr: {}", stderr(&log));
    assert!(stdout(&log).contains("init"));

    fs::write(dir.join("new-file.txt"), "content\n").unwrap();
    let status = env.run(&["legacy", "status", "--porcelain"]);
    assert!(stdout(&status).contains("?? new-file.txt"));

    assert!(env.run(&["legacy", "add", "new-file.txt"]).status.success());
    let commit = env.run(&[
        "legacy",
        "-c",
        "user.email=test@example.com",
        "-c",
        "user.name=test",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-q",
        "-m",
        "after relocation",
    ]);
    assert!(commit.status.success(), "stderr: {}", stderr(&commit));
    let log = env.run(&["legacy", "log", "--oneline"]);
    assert!(stdout(&log).contains("after relocation"));
}

#[test]
fn sync_relocation_is_idempotent() {
    let env = TestEnv::new();
    env.old_layout_sidecar("legacy");

    assert!(env.run(&["sync", "legacy"]).status.success());
    let output = env.run(&["sync", "legacy"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(stdout(&output).contains("legacy: already present"));
    assert!(!env.parent().join("legacy").join(".git").exists());
    assert!(env.sidecar_gitdir("legacy").join("HEAD").is_file());
}

#[test]
fn sync_without_name_relocates_all_old_layout_sidecars() {
    let env = TestEnv::new();
    env.old_layout_sidecar("alpha");
    env.old_layout_sidecar("beta");

    let output = env.run(&["sync"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    for name in ["alpha", "beta"] {
        assert!(!env.parent().join(name).join(".git").exists(), "{name}");
        assert!(env.sidecar_gitdir(name).join("HEAD").is_file(), "{name}");
    }
}

#[test]
fn clone_standalone_keeps_git_in_mapping_and_records_the_flag() {
    let (env, sidecar_dir) = cloned_env(&["--standalone"]);

    assert!(sidecar_dir.join(".git").is_dir());
    assert!(!env.sidecar_gitdir("sidecar").exists());
    let config = fs::read_to_string(env.config_file()).unwrap();
    assert!(config.contains("standalone = true"));
}

#[test]
fn sync_standalone_reverses_relocation_and_persists_the_setting() {
    let (env, sidecar_dir) = cloned_env(&[]);
    assert!(!sidecar_dir.join(".git").exists());

    let output = env.run(&["sync", "sidecar", "--standalone"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(sidecar_dir.join(".git").is_dir());
    assert!(!env.sidecar_gitdir("sidecar").exists());
    let config = fs::read_to_string(env.config_file()).unwrap();
    assert!(config.contains("standalone = true"));

    // history and working tree intact
    let log = env.run(&["sidecar", "log", "--oneline"]);
    assert!(log.status.success(), "stderr: {}", stderr(&log));
    assert!(stdout(&log).contains("init"));
    assert!(sidecar_dir.join("README.md").is_file());
}

#[test]
fn plain_sync_never_relocates_a_standalone_sidecar() {
    let (env, sidecar_dir) = cloned_env(&["--standalone"]);

    for args in [&["sync"][..], &["sync", "sidecar"][..]] {
        let output = env.run(args);
        assert!(output.status.success(), "stderr: {}", stderr(&output));
        assert!(
            sidecar_dir.join(".git").is_dir(),
            "git sidecar {args:?} must not move a standalone sidecar's git dir"
        );
        assert!(
            !env.sidecar_gitdir("sidecar").exists(),
            "git sidecar {args:?} must not create an external git dir"
        );
    }
}

#[test]
fn sync_unify_reverts_a_standalone_sidecar() {
    let (env, sidecar_dir) = cloned_env(&["--standalone"]);

    let output = env.run(&["sync", "sidecar", "--unify"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    assert!(!sidecar_dir.join(".git").exists());
    assert!(env.sidecar_gitdir("sidecar").join("HEAD").is_file());
    let config = fs::read_to_string(env.config_file()).unwrap();
    assert!(!config.contains("standalone = true"));
}

#[test]
fn sync_rejects_standalone_together_with_unify() {
    let env = TestEnv::new();

    let output = env.run(&["sync", "sidecar", "--standalone", "--unify"]);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("cannot be used with"));
}

#[test]
fn sync_standalone_and_unify_require_a_name() {
    let (env, _) = cloned_env(&[]);

    for flag in ["--standalone", "--unify"] {
        let output = env.run(&["sync", flag]);
        assert!(!output.status.success());
        assert!(stderr(&output).contains("requires a sidecar name"));
    }
}

#[test]
fn list_distinguishes_unified_and_standalone_sidecars() {
    let (env, _) = cloned_env(&[]);
    let output = env.run(&[
        "clone",
        env.sidecar_remote().to_str().unwrap(),
        "loner",
        "--name",
        "loner",
        "--standalone",
    ]);
    assert!(output.status.success(), "stderr: {}", stderr(&output));

    let output = env.run(&["list"]);

    assert!(output.status.success());
    let out = stdout(&output);
    let sidecar_row = out.lines().find(|l| l.starts_with("sidecar")).unwrap();
    let loner_row = out.lines().find(|l| l.starts_with("loner")).unwrap();
    assert!(sidecar_row.ends_with("unified"), "row: {sidecar_row}");
    assert!(loner_row.ends_with("standalone"), "row: {loner_row}");
}

#[test]
fn exclude_managed_block_round_trips_for_unified_sidecars() {
    let env = TestEnv::new();
    fs::create_dir_all(env.exclude_file().parent().unwrap()).unwrap();
    fs::write(env.exclude_file(), "*.log\n").unwrap();

    assert!(
        env.run(&["clone", env.sidecar_remote().to_str().unwrap()])
            .status
            .success()
    );
    let exclude = fs::read_to_string(env.exclude_file()).unwrap();
    assert!(exclude.contains("*.log"));
    assert!(exclude.contains("/sidecar/"));

    assert!(env.run(&["remove", "sidecar"]).status.success());
    let exclude = fs::read_to_string(env.exclude_file()).unwrap();
    assert!(exclude.contains("*.log"), "manual entries must survive");
    assert!(!exclude.contains("/sidecar/"));
}

#[test]
fn old_config_without_standalone_key_defaults_to_unified() {
    let env = TestEnv::new();
    env.write_config(&format!(
        "version = 1\n\n[sidecars.sidecar]\nrepo = '{}'\nmapping = \"sidecar/\"\n",
        env.sidecar_remote().display()
    ));
    let before = fs::read_to_string(env.config_file()).unwrap();

    let output = env.run(&["sync"]);

    assert!(output.status.success(), "stderr: {}", stderr(&output));
    // treated as unified: cloned into the external-gitdir layout
    assert!(!env.parent().join("sidecar").join(".git").exists());
    assert!(env.sidecar_gitdir("sidecar").join("HEAD").is_file());
    // a flag-less sync never rewrites the config
    assert_eq!(before, fs::read_to_string(env.config_file()).unwrap());

    let output = env.run(&["list"]);
    let row = stdout(&output);
    assert!(
        row.lines()
            .any(|l| l.starts_with("sidecar") && l.ends_with("unified"))
    );
}

#[test]
fn missing_external_gitdir_is_a_clear_error() {
    let (env, _) = cloned_env(&[]);
    fs::remove_dir_all(env.parent().join(".git").join("git-sidecar")).unwrap();

    let output = env.run(&["sidecar", "status"]);

    assert!(!output.status.success());
    let err = stderr(&output);
    assert!(err.contains("has no git directory"), "stderr: {err}");
    assert!(err.contains("git sidecar sync"), "stderr: {err}");
}
