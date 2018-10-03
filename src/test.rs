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
        ensure_empty_dir(tmp.as_path());

        // unsafe {
        //     ::libc::atexit(cleanup_tmpdir);
        // }

        tmp
    };
}

#[allow(private_no_mangle_fns)]
#[no_mangle]
extern "C" fn cleanup_tmpdir() {
    if TMPDIR_ROOT.exists() {
        fs::create_dir_all(TMPDIR_ROOT.as_path()).unwrap();
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
    ensure_empty_dir(tmp.as_path());
    tmp
}

fn open_cargo_toml(repo_dir: &Path) -> fs::File {
    OpenOptions::new()
        .write(true)
        .append(true)
        .open(repo_dir.join("Cargo.toml"))
        .unwrap()
}

fn run_cargo<I, S, P>(project_root: P, args: I) -> Output
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
    assert!(
        out.status.success(),
        "cargo failed: {}",
        str::from_utf8(out.stderr.as_slice()).unwrap()
    );
    out
}

fn cargo_project_for(name: &str) -> PathBuf {
    let dir = tmpdir_for(name);
    run_cargo(&dir, &["init", "--lib"]);

    let mut cargo_toml = open_cargo_toml(dir.as_path());
    writeln!(
        cargo_toml,
        "\n\n[patch.crates-io]\ncargo-husky = {{ path = \"{}\" }}\n\n[dev-dependencies.cargo-husky]\nversion = \"{}\"",
        fs::canonicalize(file!())
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_string_lossy(),
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
    run_cargo(&root, &["test"]);
    let script = get_hook_script(root.as_path(), "pre-push").unwrap();

    assert_eq!(script.lines().nth(0).unwrap(), "#!/bin/sh");
    assert!(
        script
            .lines()
            .nth(2)
            .unwrap()
            .contains(format!("set by cargo-husky v{}", env!("CARGO_PKG_VERSION")).as_str())
    );
    assert_eq!(script.lines().filter(|l| *l == "cargo test").count(), 1);
    assert!(script.lines().all(|l| l != "cargo clippy"));

    assert_eq!(get_hook_script(root.as_path(), "pre-commit"), None);
}

#[test]
fn change_features() {
    let root = cargo_project_for("features");
    let mut cargo_toml = open_cargo_toml(root.as_path());
    writeln!(
        cargo_toml,
        "default-features = false\nfeatures = [\"precommit-hook\", \"run-cargo-clippy\"]"
    );
    run_cargo(&root, &["test"]);

    assert_eq!(get_hook_script(root.as_path(), "pre-push"), None);

    let script = get_hook_script(root.as_path(), "pre-commit").unwrap();
    assert!(script.lines().all(|l| l != "cargo test"));
    assert_eq!(script.lines().filter(|l| *l == "cargo clippy").count(), 1);
}

#[test]
fn hook_not_updated_twice() {
    let root = cargo_project_for("not-update-twice");
    run_cargo(&root, &["test"]);

    let prepush_path = hook_path(root.as_path(), "pre-push");

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

    run_cargo(&root, &["test"]);
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

    run_cargo(&root, &["test"]);

    let prepush_path = hook_path(root.as_path(), "pre-push");
    let script = get_hook_script(root.as_path(), "pre-push").unwrap();

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
        write!(f, "{}", script);
        f.metadata().unwrap().modified().unwrap()
    };

    // Remove 'target' directory to trigger compiling the package again.
    // When package is updated, the package is re-compiled. But here, package itself is not updated.
    // .git/hooks/pre-push was directly modified. So manually triggering re-compilation is necessary.
    fs::remove_dir_all(root.join("target")).unwrap();

    // Ensure modified time differs from previous
    thread::sleep(time::Duration::from_secs(1));

    run_cargo(&root, &["test"]);

    let modified_after = File::open(&prepush_path)
        .unwrap()
        .metadata()
        .unwrap()
        .modified()
        .unwrap();
    // Modified time differs since the hook script was re-generated
    assert_ne!(modified_before, modified_after);

    // Check the version is updated in hook script
    let script = get_hook_script(root.as_path(), "pre-push").unwrap();
    assert!(
        script
            .lines()
            .nth(2)
            .unwrap()
            .contains(format!("set by cargo-husky v{}", env!("CARGO_PKG_VERSION")).as_str())
    );
}

// TODO: Test foregin hook script is already there which has less than 2 lines
// TODO: Test foregin hook script is already there which has more than 2 lines
