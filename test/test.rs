#[macro_use]
extern crate lazy_static;
extern crate libc;
extern crate semver;

use semver::Version as SemVer;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, ffi, fs, str, thread, time};

lazy_static! {
    static ref TMPDIR_ROOT: PathBuf = {
        let mut tmp = env::temp_dir();
        tmp.push("cargo-husky-test");
        ensure_empty_dir(&tmp);

        unsafe {
            ::libc::atexit(cleanup_tmpdir);
        }

        tmp
    };
    static ref TESTDIR: PathBuf = fs::canonicalize(file!())
        .unwrap()
        .parent()
        .unwrap()
        .join("testdata");
}

#[no_mangle]
extern "C" fn cleanup_tmpdir() {
    if TMPDIR_ROOT.exists() {
        fs::remove_dir_all(TMPDIR_ROOT.as_path()).unwrap();
    }
}

fn ensure_empty_dir(path: &Path) {
    if path.exists() {
        for entry in fs::read_dir(path).unwrap() {
            fs::remove_dir_all(entry.unwrap().path()).unwrap();
        }
    } else {
        fs::create_dir_all(path).unwrap();
    }
}

fn tmpdir_for(name: &str) -> PathBuf {
    let tmp = TMPDIR_ROOT.join(name);
    ensure_empty_dir(&tmp);
    tmp
}

fn open_cargo_toml(repo_dir: &Path) -> fs::File {
    OpenOptions::new()
        .write(true)
        .append(true)
        .open(repo_dir.join("Cargo.toml"))
        .unwrap()
}

fn run_cargo<'a, I, S, P>(project_root: P, args: I) -> Result<Output, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<ffi::OsStr>,
    P: AsRef<Path>,
{
    let out = Command::new("cargo")
        .args(args)
        .current_dir(&project_root)
        .output()
        .unwrap();
    if out.status.success() {
        Ok(out)
    } else {
        Err(str::from_utf8(out.stderr.as_slice()).unwrap().to_string())
    }
}

fn cargo_project_for(name: &str) -> PathBuf {
    let dir = tmpdir_for(name);
    run_cargo(&dir, &["init", "--lib"]).unwrap();

    let mut cargo_toml = open_cargo_toml(&dir);
    writeln!(
        cargo_toml,
        "\n\n[patch.crates-io]\ncargo-husky = {{ path = \"{}\" }}\n\n[dev-dependencies.cargo-husky]\nversion = \"{}\"",
        fs::canonicalize(file!())
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_string_lossy()
            .replace("\\", "\\\\"),
        env!("CARGO_PKG_VERSION"),
    ).unwrap();
    dir
}

fn hook_path(root: &Path, name: &str) -> PathBuf {
    let mut path = root.to_owned();
    path.push(".git");
    path.push("hooks");
    assert!(path.exists()); // hooks directory should always exist
    path.push(name);
    return path;
}

fn get_hook_script(root: &Path, hook: &str) -> Option<String> {
    let path = hook_path(root, hook);
    let mut f = File::open(path).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    Some(s)
}

fn decrease_patch(mut ver: SemVer) -> SemVer {
    if ver.patch > 0 {
        ver.patch -= 1;
        return ver;
    }
    ver.patch = 9;
    if ver.minor > 0 {
        ver.minor -= 1;
        return ver;
    }
    ver.minor = 9;
    if ver.major > 0 {
        ver.major -= 1;
        return ver;
    }
    unreachable!();
}

#[test]
fn default_behavior() {
    let root = cargo_project_for("default");
    run_cargo(&root, &["test"]).unwrap();
    let script = get_hook_script(&root, "pre-push").unwrap();

    assert_eq!(script.lines().nth(0).unwrap(), "#!/bin/sh");
    assert!(script
        .lines()
        .nth(2)
        .unwrap()
        .contains(format!("set by cargo-husky v{}", env!("CARGO_PKG_VERSION")).as_str()));
    assert_eq!(script.lines().filter(|l| *l == "cargo test --all").count(), 1);
    assert!(script.lines().all(|l| !l.contains("cargo clippy")));

    assert_eq!(get_hook_script(&root, "pre-commit"), None);
}

#[test]
#[cfg(not(target_os = "windows"))]
fn hook_file_is_executable() {
    use std::os::unix::fs::PermissionsExt;

    let root = cargo_project_for("unit-permission");
    run_cargo(&root, &["test"]).unwrap();

    let prepush_path = hook_path(&root, "pre-push");
    let mode = File::open(&prepush_path)
        .unwrap()
        .metadata()
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o555, 0o555);
}

#[test]
fn change_features() {
    let root = cargo_project_for("features");
    let mut cargo_toml = open_cargo_toml(&root);
    writeln!(
        cargo_toml,
        "default-features = false\nfeatures = [\"precommit-hook\", \"run-cargo-clippy\", \"run-cargo-check\", \"run-cargo-fmt\"]"
    ).unwrap();
    run_cargo(&root, &["test"]).unwrap();

    assert_eq!(get_hook_script(&root, "pre-push"), None);

    let script = get_hook_script(&root, "pre-commit").unwrap();
    assert!(script.lines().all(|l| l != "cargo test"));
    assert_eq!(
        script
            .lines()
            .filter(|l| *l == "cargo clippy -- -D warnings")
            .count(),
        1
    );
    assert_eq!(script.lines().filter(|l| *l == "cargo check").count(), 1);
    assert_eq!(
        script
            .lines()
            .filter(|l| *l == "cargo fmt -- --check")
            .count(),
        1
    );
}

#[test]
fn change_features_using_run_for_all() {
    let root = cargo_project_for("features_using_run_for_all");
    let mut cargo_toml = open_cargo_toml(&root);
    writeln!(
        cargo_toml,
        "default-features = false\nfeatures = [\"precommit-hook\", \"run-for-all\", \"run-cargo-test\", \"run-cargo-check\", \"run-cargo-clippy\", \"run-cargo-fmt\"]"
    ).unwrap();
    run_cargo(&root, &["test"]).unwrap();

    assert_eq!(get_hook_script(&root, "pre-push"), None);

    let script = get_hook_script(&root, "pre-commit").unwrap();
    assert_eq!(
        script
            .lines()
            .filter(|l| *l == "cargo test --all")
            .count(),
        1
    );
    assert_eq!(
        script
            .lines()
            .filter(|l| *l == "cargo clippy --all -- -D warnings")
            .count(),
        1
    );
    assert_eq!(script.lines().filter(|l| *l == "cargo check --all").count(), 1);
    assert_eq!(
        script
            .lines()
            .filter(|l| *l == "cargo fmt --all -- --check")
            .count(),
        1
    );
}

#[test]
fn hook_not_updated_twice() {
    let root = cargo_project_for("not-update-twice");
    run_cargo(&root, &["test"]).unwrap();

    let prepush_path = hook_path(&root, "pre-push");

    let first = File::open(&prepush_path)
        .unwrap()
        .metadata()
        .unwrap()
        .modified()
        .unwrap();

    // Remove 'target' directory to trigger compiling the package again.
    // When package is updated, the package is re-compiled. But here, package itself is not updated.
    // .git/hooks/pre-push was directly modified. So manually triggering re-compilation is necessary.
    fs::remove_dir_all(root.join("target")).unwrap();

    // Ensure modified time differs from previous
    thread::sleep(time::Duration::from_secs(1));

    run_cargo(&root, &["test"]).unwrap();
    let second = File::open(&prepush_path)
        .unwrap()
        .metadata()
        .unwrap()
        .modified()
        .unwrap();

    assert_eq!(first, second); // Check the second `cargo test` does not modify hook script
}

#[test]
fn regenerate_hook_script_on_package_update() {
    let root = cargo_project_for("package-update");

    run_cargo(&root, &["test"]).unwrap();

    let prepush_path = hook_path(&root, "pre-push");
    let script = get_hook_script(&root, "pre-push").unwrap();

    // Replace version string in hook to older version
    let before = format!("set by cargo-husky v{}", env!("CARGO_PKG_VERSION"));
    let prev_version = decrease_patch(SemVer::parse(env!("CARGO_PKG_VERSION")).unwrap());
    let after = format!("set by cargo-husky v{}", prev_version);
    let script = script.replacen(before.as_str(), after.as_str(), 1);

    let modified_before = {
        let mut f = OpenOptions::new()
            .write(true)
            .read(true)
            .truncate(true)
            .open(&prepush_path)
            .unwrap();
        write!(f, "{}", script).unwrap();
        f.metadata().unwrap().modified().unwrap()
    };

    // Remove 'target' directory to trigger compiling the package again.
    // When package is updated, the package is re-compiled. But here, package itself is not updated.
    // .git/hooks/pre-push was directly modified. So manually triggering re-compilation is necessary.
    fs::remove_dir_all(root.join("target")).unwrap();

    // Ensure modified time differs from previous
    thread::sleep(time::Duration::from_secs(1));

    run_cargo(&root, &["test"]).unwrap();

    let modified_after = File::open(&prepush_path)
        .unwrap()
        .metadata()
        .unwrap()
        .modified()
        .unwrap();
    // Modified time differs since the hook script was re-generated
    assert_ne!(modified_before, modified_after);

    // Check the version is updated in hook script
    let script = get_hook_script(&root, "pre-push").unwrap();
    assert!(script
        .lines()
        .nth(2)
        .unwrap()
        .contains(format!("set by cargo-husky v{}", env!("CARGO_PKG_VERSION")).as_str()));
}

macro_rules! another_hook_test {
    ($testcase:ident, $content:expr) => {
        #[test]
        fn $testcase() {
            let root = cargo_project_for(stringify!($testcase));
            let prepush_path = hook_path(&root, "pre-push");
            let content = $content.to_string();
            let modified_before = {
                let mut f = File::create(&prepush_path).unwrap();
                writeln!(f, "{}", content).unwrap();
                f.metadata().unwrap().modified().unwrap()
            };

            // Ensure modified time differs from previous if file were updated
            thread::sleep(time::Duration::from_secs(1));

            run_cargo(&root, &["test"]).unwrap();

            let modified_after = File::open(&prepush_path)
                .unwrap()
                .metadata()
                .unwrap()
                .modified()
                .unwrap();

            assert_eq!(modified_before, modified_after);

            let script = get_hook_script(&root, "pre-push").unwrap();
            assert_eq!(content + "\n", script);
        }
    };
}

another_hook_test!(
    another_hook_less_than_3_lines,
    "#!/bin/sh\necho 'hook put by someone else'"
);
another_hook_test!(
    another_hook_more_than_3_lines,
    "#!/bin/sh\n\n\necho 'hook put by someone else'"
);

fn copy_dir_recursive(from: &Path, to: &Path) {
    if !to.exists() {
        fs::create_dir_all(to).unwrap();
    }
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let child_from = entry.path();
        let child_to = to.join(child_from.strip_prefix(from).unwrap());
        if entry.file_type().unwrap().is_dir() {
            copy_dir_recursive(&child_from, &child_to);
        } else {
            fs::copy(child_from, child_to).unwrap();
        }
    }
}

fn setup_user_hooks_feature(root: &Path) {
    let mut cargo_toml = open_cargo_toml(&root);
    writeln!(
        cargo_toml,
        "default-features = false\nfeatures = [\"user-hooks\"]" // pre-push will be ignored
    )
    .unwrap();
}

#[test]
fn user_hooks() {
    let root = cargo_project_for("user-hooks");
    setup_user_hooks_feature(&root);

    let user_hooks = TESTDIR.join("user-hooks");
    copy_dir_recursive(&user_hooks.join(".cargo-husky"), &root.join(".cargo-husky"));

    run_cargo(&root, &["test"]).unwrap();

    assert!(!hook_path(&root, "pre-push").exists()); // Default features are ignored
    assert!(hook_path(&root, "pre-commit").is_file());
    assert!(hook_path(&root, "post-merge").is_file());

    let check_line = format!(
        "# This hook was set by cargo-husky v{}: {}",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_HOMEPAGE")
    );

    let s = get_hook_script(&root, "pre-commit").unwrap();
    assert_eq!(s.lines().nth(0), Some("#! /bin/sh"));
    assert_eq!(s.lines().nth(2), Some(check_line.as_str()));
    assert_eq!(
        s.lines().nth(4),
        Some("# This is a user script for pre-commit hook with shebang")
    );

    let s = get_hook_script(&root, "post-merge").unwrap();
    assert_eq!(s.lines().nth(0), Some("#"));
    assert_eq!(s.lines().nth(2), Some(check_line.as_str()));
    assert_eq!(
        s.lines().nth(3),
        Some("# Script without shebang (I'm not sure this is useful)")
    );
}

fn assert_user_hooks_error(root: &Path) {
    match run_cargo(&root, &["test"]) {
        Ok(out) => assert!(
            false,
            "`cargo test` has unexpectedly successfully done: {:?}",
            out
        ),
        Err(err) => assert!(
            format!("{}", err).contains("User hooks directory is not found or no executable file is found in the directory"),
            "Unexpected output on `cargo test`: {}",
            err
        ),
    }
}

#[test]
fn user_hooks_dir_not_found() {
    let root = cargo_project_for("user-hooks-dir-not-found");
    setup_user_hooks_feature(&root);
    assert_user_hooks_error(&root);
}

#[test]
fn user_hooks_dir_is_empty() {
    for (idx, dir_path) in [
        PathBuf::from(".cargo-husky"),
        Path::new(".cargo-husky").join("hooks"),
    ]
    .iter()
    .enumerate()
    {
        let root = cargo_project_for(&format!("user-hooks-dir-empty-{}", idx));
        setup_user_hooks_feature(&root);

        fs::create_dir_all(&dir_path).unwrap();

        assert_user_hooks_error(&root);
    }
}

#[test]
#[cfg(not(target_os = "windows"))]
fn user_hooks_dir_only_contains_non_executable_file() {
    let root = cargo_project_for("user-hooks-dir-without-executables");
    setup_user_hooks_feature(&root);

    let mut p = root.join(".cargo-husky");
    p.push("hooks");
    fs::create_dir_all(&p).unwrap();
    let f1 = p.join("non-executable-file1");
    writeln!(File::create(&f1).unwrap(), "this\nis\nnormal\ntest\nfile").unwrap();
    assert!(f1.exists());
    let f2 = p.join("non-executable-file2");
    writeln!(
        File::create(&f2).unwrap(),
        "this\nis\nalso\nnormal\ntest\nfile"
    )
    .unwrap();
    assert!(f2.exists());

    assert_user_hooks_error(&root);
}

#[test]
#[cfg(not(target_os = "windows"))]
fn copied_user_hooks_are_executable() {
    use std::os::unix::fs::PermissionsExt;

    let root = cargo_project_for("copied-user-hooks-are-executable");
    setup_user_hooks_feature(&root);

    let mut p = root.join(".cargo-husky");

    let user_hooks = TESTDIR.join("user-hooks");
    copy_dir_recursive(&user_hooks.join(".cargo-husky"), &p);

    p.push("hooks");
    p.push("non-executable-file.txt");
    writeln!(File::create(p).unwrap(), "foo\nbar\npiyo").unwrap();

    run_cargo(&root, &["test"]).unwrap();

    for name in &["pre-commit", "post-merge"] {
        let hook = File::open(hook_path(&root, name)).unwrap();
        let mode = hook.metadata().unwrap().permissions().mode();
        assert_eq!(mode & 0o555, 0o555);
    }

    assert!(!hook_path(&root, "non-executable-file.txt").exists());
}

#[test]
fn empty_script_file_not_allowed() {
    let root = cargo_project_for("empty-user-hook");
    setup_user_hooks_feature(&root);

    let user_hooks = TESTDIR.join("empty-user-hook");
    copy_dir_recursive(&user_hooks.join(".cargo-husky"), &root.join(".cargo-husky"));

    let err = run_cargo(&root, &["test"]).unwrap_err();
    assert!(format!("{}", err).contains("User hook script is empty"));
}
