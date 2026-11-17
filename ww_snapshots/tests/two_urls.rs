//! One local repository taken by git under two URL spellings (a path and a symlink to it), fetched by a real Cargo
//! into a throwaway `CARGO_HOME` (no network): Cargo checks out each URL apart, and the walk finds a dependency in
//! the checkout of the URL its workspace's `Cargo.lock` names.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use ww_snapshots::cargo::{CargoSources, git_checkout_dir_name};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn run(cmd: &mut Command) {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn a_repository_fetched_under_two_urls_is_found_in_the_checkout_of_each() {
    let top = std::env::temp_dir().join(format!("ww_two_urls_{}", std::process::id()));
    let _ = fs::remove_dir_all(&top);
    let repo = top.join("repos/base");
    write(
        &repo.join("Cargo.toml"),
        "[workspace]\nmembers = [\"dep_api\"]\n",
    );
    write(
        &repo.join("dep_api/Cargo.toml"),
        "[package]\nname = \"dep_api\"\nversion = \"0.3.0\"\nedition = \"2024\"\n",
    );
    write(&repo.join("dep_api/src/lib.rs"), "");
    let git = |args: &[&str]| {
        run(Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c"])
            .args(["commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1"));
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "dep_api"]);
    let link = top.join("links/base");
    fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&repo, &link).unwrap();

    let cargo_home = top.join("cargo_home");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut consumers: Vec<(PathBuf, String)> = vec![];
    for (name, source) in [("by_path", &repo), ("by_link", &link)] {
        let url = format!("file://{}", source.display());
        let consumer = top.join(name);
        write(
            &consumer.join("Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n\n\
                 [dependencies]\ndep_api = {{ git = \"{url}\" }}\n"
            ),
        );
        write(&consumer.join("src/lib.rs"), "");
        for args in [["generate-lockfile", "-q"], ["fetch", "-q"]] {
            run(Command::new(&cargo)
                .args(args)
                .current_dir(&consumer)
                .env("CARGO_HOME", &cargo_home)
                .env("CARGO_NET_GIT_FETCH_WITH_CLI", "false"));
        }
        consumers.push((consumer, url));
    }

    let checkouts = cargo_home.join("git/checkouts");
    let mut fetched: Vec<String> = fs::read_dir(&checkouts)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    fetched.sort();
    let mut expected: Vec<String> = consumers
        .iter()
        .map(|(_, url)| git_checkout_dir_name(url))
        .collect();
    expected.sort();
    let found: Vec<PathBuf> = consumers
        .iter()
        .map(|(consumer, _)| {
            let mut sources = CargoSources::default();
            sources.entry_dir = Some(consumer.clone());
            sources.cargo_home = Some(cargo_home.clone());
            sources.locked_package_dir("dep_api").unwrap()
        })
        .collect();
    fs::remove_dir_all(&top).unwrap();
    assert_eq!(fetched, expected, "Cargo's checkout directories");
    for ((_, url), dir) in consumers.iter().zip(&found) {
        let checkout = checkouts.join(git_checkout_dir_name(url));
        assert!(dir.starts_with(&checkout), "{url}: {dir:?}");
        assert!(dir.ends_with("dep_api"), "{url}: {dir:?}");
    }
}
