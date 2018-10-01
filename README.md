Husky for Cargo
===============

**This package is under construction and not ready for production until version 1.0.0 is released.**

[cargo-husky][] is a crate for Rust project managed by [cargo][]. In short, cargo-husky is a Rust
version of [husky][].

cargo-husky is a development tool to set Git hook automatically on `cargo test`. By hooking `pre-push`
and run `cargo test` automatically, it prevents broken codes from being pushed to a remote repository.

## Usage

Please add `cargo-husky` crate to `dev-dependencies` section of your project's `Cargo.toml`.

```toml
[dev-dependencies]
cargo-husky = "x.y.z"
```

Then run tests in your project directory.

```
$ cargo test
```

Check Git hook is generated at `.git/hooks/pre-push`.

## How It Works

cargo-husky sets Git hook automatically on running tests by using [build scripts of cargo][build scripts].

Build scripts are intended to be used for building third-party non-Rust code such as C libraries.
They are automatically run on compiling crates.

If `cargo-husky` crate is added to `dev-dependencies` section, it is compiled at running tests.
At the timing, [build script](./build.rs) is run and sets Git hook automatically.

cargo-husky puts Git hook file only once for the same version. When cargo-husky is updated to a new
version, it overwrites the existing hook.

## License

[MIT](./LICENSE.txt)

[cargo-husky]: https://crates.io/crates/cargo-husky
[cargo]: https://github.com/rust-lang/cargo
[husky]: https://github.com/typicode/husky
[build scripts]: https://doc.rust-lang.org/cargo/reference/build-scripts.html
