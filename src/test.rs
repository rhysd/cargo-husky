use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

lazy_static! {
    static ref TMPDIR_ROOT: PathBuf = {
        let mut tmp = env::temp_dir();
        tmp.push("cargo-husky-test");
        ensure_empty_dir(tmp.as_path());

        unsafe {
            ::libc::atexit(cleanup_tmpdir);
        }

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

fn cargo_project_for(name: &str) -> PathBuf {
    let dir = tmpdir_for(name);
    let out = Command::new("cargo")
        .arg("init")
        .current_dir(&dir)
        .output()
        .unwrap();

    let mut cargo_toml = open_cargo_toml(dir.as_path());
    writeln!(
        cargo_toml,
        "cargo-husky = \"{}\"\n\n[patch.crates-io]\ncargo-husky = {{ path = \"{}\" }}",
        env!("CARGO_PKG_VERSION"),
        fs::canonicalize(file!())
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_string_lossy(),
    ).unwrap();

    assert!(out.status.success());
    dir
}

#[test]
fn test_default() {
    let root = cargo_project_for("default");
    let out = Command::new("cargo")
        .arg("test")
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(out.status.success());
    let mut hooks_dir = root.clone();
    hooks_dir.push(".git");
    hooks_dir.push("hooks");
    let prepush = hooks_dir.join("pre-push");

    assert!(prepush.exists());

    let mut f = File::open(prepush).unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    assert_eq!(s.lines().nth(0).unwrap(), "#!/bin/sh");
    assert!(s.lines().any(|l| {
        l.contains(
            format!(
                "This hook was set by cargo-husky v{}",
                env!("CARGO_PKG_VERSION")
            ).as_str(),
        )
    }));
    assert!(s.lines().any(|l| l == "cargo test"));
    assert!(s.lines().all(|l| l != "cargo clippy"));

    let precommit = hooks_dir.join("pre-commit");
    assert!(!precommit.exists());
}
