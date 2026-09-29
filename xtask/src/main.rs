//! Development tasks, run with `cargo xtask <command>` (see `.cargo/config.toml`).
//!
//! - `bench-project` — write a large project file for performance testing.
//! - `i18n-sync` — compare `locales/en.yml` with upstream `lang/en.json`.
//!
//! This crate is development-only and is never shipped with the app.

mod bench;
mod i18n;

fn usage() {
    eprintln!(
        "cargo xtask <command>

Commands:
  bench-project --notes <N> [-o bench.json] [--no-mix]
      Write a project with roughly N spam notes plus a tumour line and a funnel.

  i18n-sync --upstream <dir-or-en.json> [--repo <dir>] [--write]
      Check the app's English catalog against the upstream language file.
      Without --write it only reports; --write applies upstream wording."
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("bench-project") => bench::run(&args[1..]),
        Some("i18n-sync") => i18n::run(&args[1..]),
        _ => {
            usage();
            1
        }
    };
    std::process::exit(code);
}
