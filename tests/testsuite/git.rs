use git2;
use std::env;
use std::fs::{self, File};
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crate::support::paths::{self, CargoPathExt};
use crate::support::sleep_ms;
use crate::support::Project;
use crate::support::{basic_lib_manifest, basic_manifest, git, main_file, path2url, project};

fn disable_git_cli() -> bool {
    // mingw git on Windows does not support Windows-style file URIs.
    // Appveyor in the rust repo has that git up front in the PATH instead
    // of Git-for-Windows, which causes this to fail.
    env::var("CARGO_TEST_DISABLE_GIT_CLI") == Ok("1".to_string())
}

#[test]
fn cargo_compile_simple_git_dep() {
    let project = project();
    let git_project = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_lib_manifest("dep1"))
            .file(
                "src/dep1.rs",
                r#"
                pub fn hello() -> &'static str {
                    "hello world"
                }
            "#,
            )
    })
    .unwrap();

    let project = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]

            git = '{}'
        "#,
                git_project.url()
            ),
        )
        .file(
            "src/main.rs",
            &main_file(r#""{}", dep1::hello()"#, &["dep1"]),
        )
        .build();

    let git_root = git_project.root();

    project
        .cargo("build")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             [COMPILING] dep1 v0.5.0 ({}#[..])\n\
             [COMPILING] foo v0.5.0 ([CWD])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) in [..]\n",
            path2url(&git_root),
            path2url(&git_root),
        ))
        .run();

    assert!(project.bin("foo").is_file());

    project
        .process(&project.bin("foo"))
        .with_stdout("hello world\n")
        .run();
}

#[test]
fn cargo_compile_git_dep_branch() {
    let project = project();
    let git_project = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_lib_manifest("dep1"))
            .file(
                "src/dep1.rs",
                r#"
                pub fn hello() -> &'static str {
                    "hello world"
                }
            "#,
            )
    })
    .unwrap();

    // Make a new branch based on the current HEAD commit
    let repo = git2::Repository::open(&git_project.root()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    let head = repo.find_commit(head).unwrap();
    repo.branch("branchy", &head, true).unwrap();

    let project = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]

            git = '{}'
            branch = "branchy"

        "#,
                git_project.url()
            ),
        )
        .file(
            "src/main.rs",
            &main_file(r#""{}", dep1::hello()"#, &["dep1"]),
        )
        .build();

    let git_root = git_project.root();

    project
        .cargo("build")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             [COMPILING] dep1 v0.5.0 ({}?branch=branchy#[..])\n\
             [COMPILING] foo v0.5.0 ([CWD])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) in [..]\n",
            path2url(&git_root),
            path2url(&git_root),
        ))
        .run();

    assert!(project.bin("foo").is_file());

    project
        .process(&project.bin("foo"))
        .with_stdout("hello world\n")
        .run();
}

#[test]
fn cargo_compile_git_dep_tag() {
    let project = project();
    let git_project = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_lib_manifest("dep1"))
            .file(
                "src/dep1.rs",
                r#"
                pub fn hello() -> &'static str {
                    "hello world"
                }
            "#,
            )
    })
    .unwrap();

    // Make a tag corresponding to the current HEAD
    let repo = git2::Repository::open(&git_project.root()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    repo.tag(
        "v0.1.0",
        &repo.find_object(head, None).unwrap(),
        &repo.signature().unwrap(),
        "make a new tag",
        false,
    )
    .unwrap();

    let project = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]

            git = '{}'
            tag = "v0.1.0"
        "#,
                git_project.url()
            ),
        )
        .file(
            "src/main.rs",
            &main_file(r#""{}", dep1::hello()"#, &["dep1"]),
        )
        .build();

    let git_root = git_project.root();

    project
        .cargo("build")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             [COMPILING] dep1 v0.5.0 ({}?tag=v0.1.0#[..])\n\
             [COMPILING] foo v0.5.0 ([CWD])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) in [..]\n",
            path2url(&git_root),
            path2url(&git_root),
        ))
        .run();

    assert!(project.bin("foo").is_file());

    project
        .process(&project.bin("foo"))
        .with_stdout("hello world\n")
        .run();

    project.cargo("build").run();
}

#[test]
fn cargo_compile_with_nested_paths() {
    let git_project = git::new("dep1", |project| {
        project
            .file(
                "Cargo.toml",
                r#"
                [project]

                name = "dep1"
                version = "0.5.0"
                authors = ["carlhuda@example.com"]

                [dependencies.dep2]

                version = "0.5.0"
                path = "vendor/dep2"

                [lib]

                name = "dep1"
            "#,
            )
            .file(
                "src/dep1.rs",
                r#"
                extern crate dep2;

                pub fn hello() -> &'static str {
                    dep2::hello()
                }
            "#,
            )
            .file("vendor/dep2/Cargo.toml", &basic_lib_manifest("dep2"))
            .file(
                "vendor/dep2/src/dep2.rs",
                r#"
                pub fn hello() -> &'static str {
                    "hello world"
                }
            "#,
            )
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]

            version = "0.5.0"
            git = '{}'

            [[bin]]

            name = "foo"
        "#,
                git_project.url()
            ),
        )
        .file(
            "src/foo.rs",
            &main_file(r#""{}", dep1::hello()"#, &["dep1"]),
        )
        .build();

    p.cargo("build").run();

    assert!(p.bin("foo").is_file());

    p.process(&p.bin("foo")).with_stdout("hello world\n").run();
}

#[test]
fn cargo_compile_with_malformed_nested_paths() {
    let git_project = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_lib_manifest("dep1"))
            .file(
                "src/dep1.rs",
                r#"
                pub fn hello() -> &'static str {
                    "hello world"
                }
            "#,
            )
            .file("vendor/dep2/Cargo.toml", "!INVALID!")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]

            version = "0.5.0"
            git = '{}'

            [[bin]]

            name = "foo"
        "#,
                git_project.url()
            ),
        )
        .file(
            "src/foo.rs",
            &main_file(r#""{}", dep1::hello()"#, &["dep1"]),
        )
        .build();

    p.cargo("build").run();

    assert!(p.bin("foo").is_file());

    p.process(&p.bin("foo")).with_stdout("hello world\n").run();
}

#[test]
fn cargo_compile_with_meta_package() {
    let git_project = git::new("meta-dep", |project| {
        project
            .file("dep1/Cargo.toml", &basic_lib_manifest("dep1"))
            .file(
                "dep1/src/dep1.rs",
                r#"
                pub fn hello() -> &'static str {
                    "this is dep1"
                }
            "#,
            )
            .file("dep2/Cargo.toml", &basic_lib_manifest("dep2"))
            .file(
                "dep2/src/dep2.rs",
                r#"
                pub fn hello() -> &'static str {
                    "this is dep2"
                }
            "#,
            )
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]

            version = "0.5.0"
            git = '{}'

            [dependencies.dep2]

            version = "0.5.0"
            git = '{}'

            [[bin]]

            name = "foo"
        "#,
                git_project.url(),
                git_project.url()
            ),
        )
        .file(
            "src/foo.rs",
            &main_file(
                r#""{} {}", dep1::hello(), dep2::hello()"#,
                &["dep1", "dep2"],
            ),
        )
        .build();

    p.cargo("build").run();

    assert!(p.bin("foo").is_file());

    p.process(&p.bin("foo"))
        .with_stdout("this is dep1 this is dep2\n")
        .run();
}

#[test]
fn cargo_compile_with_short_ssh_git() {
    let url = "git@github.com:a/dep";

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep]

            git = "{}"

            [[bin]]

            name = "foo"
        "#,
                url
            ),
        )
        .file(
            "src/foo.rs",
            &main_file(r#""{}", dep1::hello()"#, &["dep1"]),
        )
        .build();

    p.cargo("build")
        .with_status(101)
        .with_stdout("")
        .with_stderr(&format!(
            "\
[ERROR] failed to parse manifest at `[..]`

Caused by:
  invalid url `{}`: relative URL without a base
",
            url
        ))
        .run();
}

#[test]
fn two_revs_same_deps() {
    let bar = git::new("meta-dep", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.0.0"))
            .file("src/lib.rs", "pub fn bar() -> i32 { 1 }")
    })
    .unwrap();

    let repo = git2::Repository::open(&bar.root()).unwrap();
    let rev1 = repo.revparse_single("HEAD").unwrap().id();

    // Commit the changes and make sure we trigger a recompile
    File::create(&bar.root().join("src/lib.rs"))
        .unwrap()
        .write_all(br#"pub fn bar() -> i32 { 2 }"#)
        .unwrap();
    git::add(&repo);
    let rev2 = git::commit(&repo);

    let foo = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.0.0"
            authors = []

            [dependencies.bar]
            git = '{}'
            rev = "{}"

            [dependencies.baz]
            path = "../baz"
        "#,
                bar.url(),
                rev1
            ),
        )
        .file(
            "src/main.rs",
            r#"
            extern crate bar;
            extern crate baz;

            fn main() {
                assert_eq!(bar::bar(), 1);
                assert_eq!(baz::baz(), 2);
            }
        "#,
        )
        .build();

    let _baz = project()
        .at("baz")
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [package]
            name = "baz"
            version = "0.0.0"
            authors = []

            [dependencies.bar]
            git = '{}'
            rev = "{}"
        "#,
                bar.url(),
                rev2
            ),
        )
        .file(
            "src/lib.rs",
            r#"
            extern crate bar;
            pub fn baz() -> i32 { bar::bar() }
        "#,
        )
        .build();

    foo.cargo("build -v").run();
    assert!(foo.bin("foo").is_file());
    foo.process(&foo.bin("foo")).run();
}

#[test]
fn recompilation() {
    let git_project = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_lib_manifest("bar"))
            .file("src/bar.rs", "pub fn bar() {}")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.bar]

            version = "0.5.0"
            git = '{}'
        "#,
                git_project.url()
            ),
        )
        .file("src/main.rs", &main_file(r#""{:?}", bar::bar()"#, &["bar"]))
        .build();

    // First time around we should compile both foo and bar
    p.cargo("build")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             [COMPILING] bar v0.5.0 ({}#[..])\n\
             [COMPILING] foo v0.5.0 ([CWD])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) \
             in [..]\n",
            git_project.url(),
            git_project.url(),
        ))
        .run();

    // Don't recompile the second time
    p.cargo("build").with_stdout("").run();

    // Modify a file manually, shouldn't trigger a recompile
    File::create(&git_project.root().join("src/bar.rs"))
        .unwrap()
        .write_all(br#"pub fn bar() { println!("hello!"); }"#)
        .unwrap();

    p.cargo("build").with_stdout("").run();

    p.cargo("update")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`",
            git_project.url()
        ))
        .run();

    p.cargo("build").with_stdout("").run();

    // Commit the changes and make sure we don't trigger a recompile because the
    // lock file says not to change
    let repo = git2::Repository::open(&git_project.root()).unwrap();
    git::add(&repo);
    git::commit(&repo);

    println!("compile after commit");
    p.cargo("build").with_stdout("").run();
    p.root().move_into_the_past();

    // Update the dependency and carry on!
    p.cargo("update")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             [UPDATING] bar v0.5.0 ([..]) -> #[..]\n\
             ",
            git_project.url()
        ))
        .run();
    println!("going for the last compile");
    p.cargo("build")
        .with_stderr(&format!(
            "[COMPILING] bar v0.5.0 ({}#[..])\n\
             [COMPILING] foo v0.5.0 ([CWD])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) \
             in [..]\n",
            git_project.url(),
        ))
        .run();

    // Make sure clean only cleans one dep
    p.cargo("clean -p foo").with_stdout("").run();
    p.cargo("build")
        .with_stderr(
            "[COMPILING] foo v0.5.0 ([CWD])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) \
             in [..]",
        )
        .run();
}

#[test]
fn update_with_shared_deps() {
    let git_project = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_lib_manifest("bar"))
            .file("src/bar.rs", "pub fn bar() {}")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]
            path = "dep1"
            [dependencies.dep2]
            path = "dep2"
        "#,
        )
        .file(
            "src/main.rs",
            r#"
            #[allow(unused_extern_crates)]
            extern crate dep1;
            #[allow(unused_extern_crates)]
            extern crate dep2;
            fn main() {}
        "#,
        )
        .file(
            "dep1/Cargo.toml",
            &format!(
                r#"
            [package]
            name = "dep1"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.bar]
            version = "0.5.0"
            git = '{}'
        "#,
                git_project.url()
            ),
        )
        .file("dep1/src/lib.rs", "")
        .file(
            "dep2/Cargo.toml",
            &format!(
                r#"
            [package]
            name = "dep2"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.bar]
            version = "0.5.0"
            git = '{}'
        "#,
                git_project.url()
            ),
        )
        .file("dep2/src/lib.rs", "")
        .build();

    // First time around we should compile both foo and bar
    p.cargo("build")
        .with_stderr(&format!(
            "\
[UPDATING] git repository `{git}`
[COMPILING] bar v0.5.0 ({git}#[..])
[COMPILING] [..] v0.5.0 ([..])
[COMPILING] [..] v0.5.0 ([..])
[COMPILING] foo v0.5.0 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]\n",
            git = git_project.url(),
        ))
        .run();

    // Modify a file manually, and commit it
    File::create(&git_project.root().join("src/bar.rs"))
        .unwrap()
        .write_all(br#"pub fn bar() { println!("hello!"); }"#)
        .unwrap();
    let repo = git2::Repository::open(&git_project.root()).unwrap();
    let old_head = repo.head().unwrap().target().unwrap();
    git::add(&repo);
    git::commit(&repo);

    sleep_ms(1000);

    // By default, not transitive updates
    println!("dep1 update");
    p.cargo("update -p dep1").with_stdout("").run();

    // Don't do anything bad on a weird --precise argument
    println!("bar bad precise update");
    p.cargo("update -p bar --precise 0.1.2")
        .with_status(101)
        .with_stderr(
            "\
[UPDATING] git repository [..]
[ERROR] Unable to update [..]

Caused by:
  revspec '0.1.2' not found; [..]
",
        )
        .run();

    // Specifying a precise rev to the old rev shouldn't actually update
    // anything because we already have the rev in the db.
    println!("bar precise update");
    p.cargo("update -p bar --precise")
        .arg(&old_head.to_string())
        .with_stdout("")
        .run();

    // Updating aggressively should, however, update the repo.
    println!("dep1 aggressive update");
    p.cargo("update -p dep1 --aggressive")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             [UPDATING] bar v0.5.0 ([..]) -> #[..]\n\
             ",
            git_project.url()
        ))
        .run();

    // Make sure we still only compile one version of the git repo
    println!("build");
    p.cargo("build")
        .with_stderr(&format!(
            "\
[COMPILING] bar v0.5.0 ({git}#[..])
[COMPILING] [..] v0.5.0 ([CWD][..]dep[..])
[COMPILING] [..] v0.5.0 ([CWD][..]dep[..])
[COMPILING] foo v0.5.0 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]\n",
            git = git_project.url(),
        ))
        .run();

    // We should be able to update transitive deps
    p.cargo("update -p bar")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`",
            git_project.url()
        ))
        .run();
}

#[test]
fn dep_with_submodule() {
    let project = project();
    let git_project = git::new("dep1", |project| {
        project.file("Cargo.toml", &basic_manifest("dep1", "0.5.0"))
    })
    .unwrap();
    let git_project2 =
        git::new("dep2", |project| project.file("lib.rs", "pub fn dep() {}")).unwrap();

    let repo = git2::Repository::open(&git_project.root()).unwrap();
    let url = path2url(git_project2.root()).to_string();
    git::add_submodule(&repo, &url, Path::new("src"));
    git::commit(&repo);

    let project = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]

            git = '{}'
        "#,
                git_project.url()
            ),
        )
        .file(
            "src/lib.rs",
            "extern crate dep1; pub fn foo() { dep1::dep() }",
        )
        .build();

    project
        .cargo("build")
        .with_stderr(
            "\
[UPDATING] git repository [..]
[COMPILING] dep1 [..]
[COMPILING] foo [..]
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]\n",
        )
        .run();
}

#[test]
fn dep_with_bad_submodule() {
    let project = project();
    let git_project = git::new("dep1", |project| {
        project.file("Cargo.toml", &basic_manifest("dep1", "0.5.0"))
    })
    .unwrap();
    let git_project2 =
        git::new("dep2", |project| project.file("lib.rs", "pub fn dep() {}")).unwrap();

    let repo = git2::Repository::open(&git_project.root()).unwrap();
    let url = path2url(git_project2.root()).to_string();
    git::add_submodule(&repo, &url, Path::new("src"));
    git::commit(&repo);

    // now amend the first commit on git_project2 to make submodule ref point to not-found
    // commit
    let repo = git2::Repository::open(&git_project2.root()).unwrap();
    let original_submodule_ref = repo.refname_to_id("refs/heads/master").unwrap();
    let commit = repo.find_commit(original_submodule_ref).unwrap();
    commit
        .amend(
            Some("refs/heads/master"),
            None,
            None,
            None,
            Some("something something"),
            None,
        )
        .unwrap();

    let p = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]

            git = '{}'
        "#,
                git_project.url()
            ),
        )
        .file(
            "src/lib.rs",
            "extern crate dep1; pub fn foo() { dep1::dep() }",
        )
        .build();

    let expected = format!(
        "\
[UPDATING] git repository [..]
[ERROR] failed to load source for a dependency on `dep1`

Caused by:
  Unable to update {}

Caused by:
  failed to update submodule `src`

Caused by:
  object not found - no match for id [..]
",
        path2url(git_project.root())
    );

    p.cargo("build")
        .with_stderr(expected)
        .with_status(101)
        .run();
}

#[test]
fn two_deps_only_update_one() {
    let project = project();
    let git1 = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_manifest("dep1", "0.5.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();
    let git2 = git::new("dep2", |project| {
        project
            .file("Cargo.toml", &basic_manifest("dep2", "0.5.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();

    let p = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]
            git = '{}'
            [dependencies.dep2]
            git = '{}'
        "#,
                git1.url(),
                git2.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    fn oid_to_short_sha(oid: git2::Oid) -> String {
        oid.to_string()[..8].to_string()
    }
    fn git_repo_head_sha(p: &Project) -> String {
        let repo = git2::Repository::open(p.root()).unwrap();
        let head = repo.head().unwrap().target().unwrap();
        oid_to_short_sha(head)
    }

    println!("dep1 head sha: {}", git_repo_head_sha(&git1));
    println!("dep2 head sha: {}", git_repo_head_sha(&git2));

    p.cargo("build")
        .with_stderr(
            "[UPDATING] git repository `[..]`\n\
             [UPDATING] git repository `[..]`\n\
             [COMPILING] [..] v0.5.0 ([..])\n\
             [COMPILING] [..] v0.5.0 ([..])\n\
             [COMPILING] foo v0.5.0 ([CWD])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) in [..]\n",
        )
        .run();

    File::create(&git1.root().join("src/lib.rs"))
        .unwrap()
        .write_all(br#"pub fn foo() {}"#)
        .unwrap();
    let repo = git2::Repository::open(&git1.root()).unwrap();
    git::add(&repo);
    let oid = git::commit(&repo);
    println!("dep1 head sha: {}", oid_to_short_sha(oid));

    p.cargo("update -p dep1")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             [UPDATING] dep1 v0.5.0 ([..]) -> #[..]\n\
             ",
            git1.url()
        ))
        .run();
}

#[test]
fn stale_cached_version() {
    let bar = git::new("meta-dep", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.0.0"))
            .file("src/lib.rs", "pub fn bar() -> i32 { 1 }")
    })
    .unwrap();

    // Update the git database in the cache with the current state of the git
    // repo
    let foo = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.0.0"
            authors = []

            [dependencies.bar]
            git = '{}'
        "#,
                bar.url()
            ),
        )
        .file(
            "src/main.rs",
            r#"
            extern crate bar;

            fn main() { assert_eq!(bar::bar(), 1) }
        "#,
        )
        .build();

    foo.cargo("build").run();
    foo.process(&foo.bin("foo")).run();

    // Update the repo, and simulate someone else updating the lock file and then
    // us pulling it down.
    File::create(&bar.root().join("src/lib.rs"))
        .unwrap()
        .write_all(br#"pub fn bar() -> i32 { 1 + 0 }"#)
        .unwrap();
    let repo = git2::Repository::open(&bar.root()).unwrap();
    git::add(&repo);
    git::commit(&repo);

    sleep_ms(1000);

    let rev = repo.revparse_single("HEAD").unwrap().id();

    File::create(&foo.root().join("Cargo.lock"))
        .unwrap()
        .write_all(
            format!(
                r#"
        [[package]]
        name = "foo"
        version = "0.0.0"
        dependencies = [
         'bar 0.0.0 (git+{url}#{hash})'
        ]

        [[package]]
        name = "bar"
        version = "0.0.0"
        source = 'git+{url}#{hash}'
    "#,
                url = bar.url(),
                hash = rev
            )
            .as_bytes(),
        )
        .unwrap();

    // Now build!
    foo.cargo("build")
        .with_stderr(&format!(
            "\
[UPDATING] git repository `{bar}`
[COMPILING] bar v0.0.0 ({bar}#[..])
[COMPILING] foo v0.0.0 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
            bar = bar.url(),
        ))
        .run();
    foo.process(&foo.bin("foo")).run();
}

#[test]
fn dep_with_changed_submodule() {
    let project = project();
    let git_project = git::new("dep1", |project| {
        project.file("Cargo.toml", &basic_manifest("dep1", "0.5.0"))
    })
    .unwrap();

    let git_project2 = git::new("dep2", |project| {
        project.file("lib.rs", "pub fn dep() -> &'static str { \"project2\" }")
    })
    .unwrap();

    let git_project3 = git::new("dep3", |project| {
        project.file("lib.rs", "pub fn dep() -> &'static str { \"project3\" }")
    })
    .unwrap();

    let repo = git2::Repository::open(&git_project.root()).unwrap();
    let mut sub = git::add_submodule(&repo, &git_project2.url().to_string(), Path::new("src"));
    git::commit(&repo);

    let p = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]
            [dependencies.dep1]
            git = '{}'
        "#,
                git_project.url()
            ),
        )
        .file(
            "src/main.rs",
            "
            extern crate dep1;
            pub fn main() { println!(\"{}\", dep1::dep()) }
        ",
        )
        .build();

    println!("first run");
    p.cargo("run")
        .with_stderr(
            "[UPDATING] git repository `[..]`\n\
             [COMPILING] dep1 v0.5.0 ([..])\n\
             [COMPILING] foo v0.5.0 ([..])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) in \
             [..]\n\
             [RUNNING] `target/debug/foo[EXE]`\n",
        )
        .with_stdout("project2\n")
        .run();

    File::create(&git_project.root().join(".gitmodules"))
        .unwrap()
        .write_all(
            format!(
                "[submodule \"src\"]\n\tpath = src\n\turl={}",
                git_project3.url()
            )
            .as_bytes(),
        )
        .unwrap();

    // Sync the submodule and reset it to the new remote.
    sub.sync().unwrap();
    {
        let subrepo = sub.open().unwrap();
        subrepo
            .remote_add_fetch("origin", "refs/heads/*:refs/heads/*")
            .unwrap();
        subrepo
            .remote_set_url("origin", &git_project3.url().to_string())
            .unwrap();
        let mut origin = subrepo.find_remote("origin").unwrap();
        origin.fetch(&[], None, None).unwrap();
        let id = subrepo.refname_to_id("refs/remotes/origin/master").unwrap();
        let obj = subrepo.find_object(id, None).unwrap();
        subrepo.reset(&obj, git2::ResetType::Hard, None).unwrap();
    }
    sub.add_to_index(true).unwrap();
    git::add(&repo);
    git::commit(&repo);

    sleep_ms(1000);
    // Update the dependency and carry on!
    println!("update");
    p.cargo("update -v")
        .with_stderr("")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             [UPDATING] dep1 v0.5.0 ([..]) -> #[..]\n\
             ",
            git_project.url()
        ))
        .run();

    println!("last run");
    p.cargo("run")
        .with_stderr(
            "[COMPILING] dep1 v0.5.0 ([..])\n\
             [COMPILING] foo v0.5.0 ([..])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) in \
             [..]\n\
             [RUNNING] `target/debug/foo[EXE]`\n",
        )
        .with_stdout("project3\n")
        .run();
}

#[test]
fn dev_deps_with_testing() {
    let p2 = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.5.0"))
            .file(
                "src/lib.rs",
                r#"
            pub fn gimme() -> &'static str { "zoidberg" }
        "#,
            )
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dev-dependencies.bar]
            version = "0.5.0"
            git = '{}'
        "#,
                p2.url()
            ),
        )
        .file(
            "src/main.rs",
            r#"
            fn main() {}

            #[cfg(test)]
            mod tests {
                extern crate bar;
                #[test] fn foo() { bar::gimme(); }
            }
        "#,
        )
        .build();

    // Generate a lock file which did not use `bar` to compile, but had to update
    // `bar` to generate the lock file
    p.cargo("build")
        .with_stderr(&format!(
            "\
[UPDATING] git repository `{bar}`
[COMPILING] foo v0.5.0 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
            bar = p2.url()
        ))
        .run();

    // Make sure we use the previous resolution of `bar` instead of updating it
    // a second time.
    p.cargo("test")
        .with_stderr(
            "\
[COMPILING] [..] v0.5.0 ([..])
[COMPILING] [..] v0.5.0 ([..]
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
[RUNNING] target/debug/deps/foo-[..][EXE]",
        )
        .with_stdout_contains("test tests::foo ... ok")
        .run();
}

#[test]
fn git_build_cmd_freshness() {
    let foo = git::new("foo", |project| {
        project
            .file(
                "Cargo.toml",
                r#"
            [package]
            name = "foo"
            version = "0.0.0"
            authors = []
            build = "build.rs"
        "#,
            )
            .file("build.rs", "fn main() {}")
            .file("src/lib.rs", "pub fn bar() -> i32 { 1 }")
            .file(".gitignore", "src/bar.rs")
    })
    .unwrap();
    foo.root().move_into_the_past();

    sleep_ms(1000);

    foo.cargo("build")
        .with_stderr(
            "\
[COMPILING] foo v0.0.0 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();

    // Smoke test to make sure it doesn't compile again
    println!("first pass");
    foo.cargo("build").with_stdout("").run();

    // Modify an ignored file and make sure we don't rebuild
    println!("second pass");
    File::create(&foo.root().join("src/bar.rs")).unwrap();
    foo.cargo("build").with_stdout("").run();
}

#[test]
fn git_name_not_always_needed() {
    let p2 = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.5.0"))
            .file(
                "src/lib.rs",
                r#"
            pub fn gimme() -> &'static str { "zoidberg" }
        "#,
            )
    })
    .unwrap();

    let repo = git2::Repository::open(&p2.root()).unwrap();
    let mut cfg = repo.config().unwrap();
    let _ = cfg.remove("user.name");
    let _ = cfg.remove("user.email");

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []

            [dev-dependencies.bar]
            git = '{}'
        "#,
                p2.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    // Generate a lock file which did not use `bar` to compile, but had to update
    // `bar` to generate the lock file
    p.cargo("build")
        .with_stderr(&format!(
            "\
[UPDATING] git repository `{bar}`
[COMPILING] foo v0.5.0 ([CWD])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
            bar = p2.url()
        ))
        .run();
}

#[test]
fn git_repo_changing_no_rebuild() {
    let bar = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.5.0"))
            .file("src/lib.rs", "pub fn bar() -> i32 { 1 }")
    })
    .unwrap();

    // Lock p1 to the first rev in the git repo
    let p1 = project()
        .at("p1")
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "p1"
            version = "0.5.0"
            authors = []
            build = 'build.rs'
            [dependencies.bar]
            git = '{}'
        "#,
                bar.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .file("build.rs", "fn main() {}")
        .build();
    p1.root().move_into_the_past();
    p1.cargo("build")
        .with_stderr(&format!(
            "\
[UPDATING] git repository `{bar}`
[COMPILING] [..]
[COMPILING] [..]
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
            bar = bar.url()
        ))
        .run();

    // Make a commit to lock p2 to a different rev
    File::create(&bar.root().join("src/lib.rs"))
        .unwrap()
        .write_all(br#"pub fn bar() -> i32 { 2 }"#)
        .unwrap();
    let repo = git2::Repository::open(&bar.root()).unwrap();
    git::add(&repo);
    git::commit(&repo);

    // Lock p2 to the second rev
    let p2 = project()
        .at("p2")
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "p2"
            version = "0.5.0"
            authors = []
            [dependencies.bar]
            git = '{}'
        "#,
                bar.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();
    p2.cargo("build")
        .with_stderr(&format!(
            "\
[UPDATING] git repository `{bar}`
[COMPILING] [..]
[COMPILING] [..]
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
            bar = bar.url()
        ))
        .run();

    // And now for the real test! Make sure that p1 doesn't get rebuilt
    // even though the git repo has changed.
    p1.cargo("build").with_stdout("").run();
}

#[test]
fn git_dep_build_cmd() {
    let p = git::new("foo", |project| {
        project
            .file(
                "Cargo.toml",
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.bar]

            version = "0.5.0"
            path = "bar"

            [[bin]]

            name = "foo"
        "#,
            )
            .file("src/foo.rs", &main_file(r#""{}", bar::gimme()"#, &["bar"]))
            .file(
                "bar/Cargo.toml",
                r#"
            [project]

            name = "bar"
            version = "0.5.0"
            authors = ["wycats@example.com"]
            build = "build.rs"

            [lib]
            name = "bar"
            path = "src/bar.rs"
        "#,
            )
            .file(
                "bar/src/bar.rs.in",
                r#"
            pub fn gimme() -> i32 { 0 }
        "#,
            )
            .file(
                "bar/build.rs",
                r#"
            use std::fs;
            fn main() {
                fs::copy("src/bar.rs.in", "src/bar.rs").unwrap();
            }
        "#,
            )
    })
    .unwrap();

    p.root().join("bar").move_into_the_past();

    p.cargo("build").run();

    p.process(&p.bin("foo")).with_stdout("0\n").run();

    // Touching bar.rs.in should cause the `build` command to run again.
    fs::File::create(&p.root().join("bar/src/bar.rs.in"))
        .unwrap()
        .write_all(b"pub fn gimme() -> i32 { 1 }")
        .unwrap();

    p.cargo("build").run();

    p.process(&p.bin("foo")).with_stdout("1\n").run();
}

#[test]
fn fetch_downloads() {
    let bar = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.5.0"))
            .file("src/lib.rs", "pub fn bar() -> i32 { 1 }")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []
            [dependencies.bar]
            git = '{}'
        "#,
                bar.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();
    p.cargo("fetch")
        .with_stderr(&format!(
            "[UPDATING] git repository `{url}`",
            url = bar.url()
        ))
        .run();

    p.cargo("fetch").with_stdout("").run();
}

#[test]
fn warnings_in_git_dep() {
    let bar = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.5.0"))
            .file("src/lib.rs", "fn unused() {}")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []
            [dependencies.bar]
            git = '{}'
        "#,
                bar.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build")
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             [COMPILING] bar v0.5.0 ({}#[..])\n\
             [COMPILING] foo v0.5.0 ([CWD])\n\
             [FINISHED] dev [unoptimized + debuginfo] target(s) in [..]\n",
            bar.url(),
            bar.url(),
        ))
        .run();
}

#[test]
fn update_ambiguous() {
    let bar1 = git::new("bar1", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.5.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();
    let bar2 = git::new("bar2", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.6.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();
    let baz = git::new("baz", |project| {
        project
            .file(
                "Cargo.toml",
                &format!(
                    r#"
            [package]
            name = "baz"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.bar]
            git = '{}'
        "#,
                    bar2.url()
                ),
            )
            .file("src/lib.rs", "")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []
            [dependencies.bar]
            git = '{}'
            [dependencies.baz]
            git = '{}'
        "#,
                bar1.url(),
                baz.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("generate-lockfile").run();
    p.cargo("update -p bar")
        .with_status(101)
        .with_stderr(
            "\
[ERROR] There are multiple `bar` packages in your project, and the specification `bar` \
is ambiguous.
Please re-run this command with `-p <spec>` where `<spec>` is one of the \
following:
  bar:0.[..].0
  bar:0.[..].0
",
        )
        .run();
}

#[test]
fn update_one_dep_in_repo_with_many_deps() {
    let bar = git::new("bar", |project| {
        project
            .file("Cargo.toml", &basic_manifest("bar", "0.5.0"))
            .file("src/lib.rs", "")
            .file("a/Cargo.toml", &basic_manifest("a", "0.5.0"))
            .file("a/src/lib.rs", "")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []
            [dependencies.bar]
            git = '{}'
            [dependencies.a]
            git = '{}'
        "#,
                bar.url(),
                bar.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("generate-lockfile").run();
    p.cargo("update -p bar")
        .with_stderr(&format!("[UPDATING] git repository `{}`", bar.url()))
        .run();
}

#[test]
fn switch_deps_does_not_update_transitive() {
    let transitive = git::new("transitive", |project| {
        project
            .file("Cargo.toml", &basic_manifest("transitive", "0.5.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();
    let dep1 = git::new("dep1", |project| {
        project
            .file(
                "Cargo.toml",
                &format!(
                    r#"
            [package]
            name = "dep"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.transitive]
            git = '{}'
        "#,
                    transitive.url()
                ),
            )
            .file("src/lib.rs", "")
    })
    .unwrap();
    let dep2 = git::new("dep2", |project| {
        project
            .file(
                "Cargo.toml",
                &format!(
                    r#"
            [package]
            name = "dep"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.transitive]
            git = '{}'
        "#,
                    transitive.url()
                ),
            )
            .file("src/lib.rs", "")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []
            [dependencies.dep]
            git = '{}'
        "#,
                dep1.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build")
        .with_stderr(&format!(
            "\
[UPDATING] git repository `{}`
[UPDATING] git repository `{}`
[COMPILING] transitive [..]
[COMPILING] dep [..]
[COMPILING] foo [..]
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
            dep1.url(),
            transitive.url()
        ))
        .run();

    // Update the dependency to point to the second repository, but this
    // shouldn't update the transitive dependency which is the same.
    File::create(&p.root().join("Cargo.toml"))
        .unwrap()
        .write_all(
            format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []
            [dependencies.dep]
            git = '{}'
    "#,
                dep2.url()
            )
            .as_bytes(),
        )
        .unwrap();

    p.cargo("build")
        .with_stderr(&format!(
            "\
[UPDATING] git repository `{}`
[COMPILING] dep [..]
[COMPILING] foo [..]
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
            dep2.url()
        ))
        .run();
}

#[test]
fn update_one_source_updates_all_packages_in_that_git_source() {
    let dep = git::new("dep", |project| {
        project
            .file(
                "Cargo.toml",
                r#"
            [package]
            name = "dep"
            version = "0.5.0"
            authors = []

            [dependencies.a]
            path = "a"
        "#,
            )
            .file("src/lib.rs", "")
            .file("a/Cargo.toml", &basic_manifest("a", "0.5.0"))
            .file("a/src/lib.rs", "")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []
            [dependencies.dep]
            git = '{}'
        "#,
                dep.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    p.cargo("build").run();

    let repo = git2::Repository::open(&dep.root()).unwrap();
    let rev1 = repo.revparse_single("HEAD").unwrap().id();

    // Just be sure to change a file
    File::create(&dep.root().join("src/lib.rs"))
        .unwrap()
        .write_all(br#"pub fn bar() -> i32 { 2 }"#)
        .unwrap();
    git::add(&repo);
    git::commit(&repo);

    p.cargo("update -p dep").run();
    let mut lockfile = String::new();
    File::open(&p.root().join("Cargo.lock"))
        .unwrap()
        .read_to_string(&mut lockfile)
        .unwrap();
    assert!(
        !lockfile.contains(&rev1.to_string()),
        "{} in {}",
        rev1,
        lockfile
    );
}

#[test]
fn switch_sources() {
    let a1 = git::new("a1", |project| {
        project
            .file("Cargo.toml", &basic_manifest("a", "0.5.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();
    let a2 = git::new("a2", |project| {
        project
            .file("Cargo.toml", &basic_manifest("a", "0.5.1"))
            .file("src/lib.rs", "")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []
            [dependencies.b]
            path = "b"
        "#,
        )
        .file("src/main.rs", "fn main() {}")
        .file(
            "b/Cargo.toml",
            &format!(
                r#"
            [project]
            name = "b"
            version = "0.5.0"
            authors = []
            [dependencies.a]
            git = '{}'
        "#,
                a1.url()
            ),
        )
        .file("b/src/lib.rs", "pub fn main() {}")
        .build();

    p.cargo("build")
        .with_stderr(
            "\
[UPDATING] git repository `file://[..]a1`
[COMPILING] a v0.5.0 ([..]a1#[..]
[COMPILING] b v0.5.0 ([..])
[COMPILING] foo v0.5.0 ([..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();

    File::create(&p.root().join("b/Cargo.toml"))
        .unwrap()
        .write_all(
            format!(
                r#"
        [project]
        name = "b"
        version = "0.5.0"
        authors = []
        [dependencies.a]
        git = '{}'
    "#,
                a2.url()
            )
            .as_bytes(),
        )
        .unwrap();

    p.cargo("build")
        .with_stderr(
            "\
[UPDATING] git repository `file://[..]a2`
[COMPILING] a v0.5.1 ([..]a2#[..]
[COMPILING] b v0.5.0 ([..])
[COMPILING] foo v0.5.0 ([..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

#[test]
fn dont_require_submodules_are_checked_out() {
    let p = project().build();
    let git1 = git::new("dep1", |p| {
        p.file(
            "Cargo.toml",
            r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []
            build = "build.rs"
        "#,
        )
        .file("build.rs", "fn main() {}")
        .file("src/lib.rs", "")
        .file("a/foo", "")
    })
    .unwrap();
    let git2 = git::new("dep2", |p| p).unwrap();

    let repo = git2::Repository::open(&git1.root()).unwrap();
    let url = path2url(git2.root()).to_string();
    git::add_submodule(&repo, &url, Path::new("a/submodule"));
    git::commit(&repo);

    git2::Repository::init(&p.root()).unwrap();
    let url = path2url(git1.root()).to_string();
    let dst = paths::home().join("foo");
    git2::Repository::clone(&url, &dst).unwrap();

    git1.cargo("build -v").cwd(&dst).run();
}

#[test]
fn doctest_same_name() {
    let a2 = git::new("a2", |p| {
        p.file("Cargo.toml", &basic_manifest("a", "0.5.0"))
            .file("src/lib.rs", "pub fn a2() {}")
    })
    .unwrap();

    let a1 = git::new("a1", |p| {
        p.file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "a"
            version = "0.5.0"
            authors = []
            [dependencies]
            a = {{ git = '{}' }}
        "#,
                a2.url()
            ),
        )
        .file("src/lib.rs", "extern crate a; pub fn a1() {}")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies]
            a = {{ git = '{}' }}
        "#,
                a1.url()
            ),
        )
        .file(
            "src/lib.rs",
            r#"
            #[macro_use]
            extern crate a;
        "#,
        )
        .build();

    p.cargo("test -v").run();
}

#[test]
fn lints_are_suppressed() {
    let a = git::new("a", |p| {
        p.file("Cargo.toml", &basic_manifest("a", "0.5.0")).file(
            "src/lib.rs",
            "
            use std::option;
        ",
        )
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies]
            a = {{ git = '{}' }}
        "#,
                a.url()
            ),
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build")
        .with_stderr(
            "\
[UPDATING] git repository `[..]`
[COMPILING] a v0.5.0 ([..])
[COMPILING] foo v0.0.1 ([..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

#[test]
fn denied_lints_are_allowed() {
    let a = git::new("a", |p| {
        p.file("Cargo.toml", &basic_manifest("a", "0.5.0")).file(
            "src/lib.rs",
            "
            #![deny(warnings)]
            use std::option;
        ",
        )
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies]
            a = {{ git = '{}' }}
        "#,
                a.url()
            ),
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("build")
        .with_stderr(
            "\
[UPDATING] git repository `[..]`
[COMPILING] a v0.5.0 ([..])
[COMPILING] foo v0.0.1 ([..])
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
}

#[test]
fn add_a_git_dep() {
    let git = git::new("git", |p| {
        p.file("Cargo.toml", &basic_manifest("git", "0.5.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies]
            a = {{ path = 'a' }}
            git = {{ git = '{}' }}
        "#,
                git.url()
            ),
        )
        .file("src/lib.rs", "")
        .file("a/Cargo.toml", &basic_manifest("a", "0.0.1"))
        .file("a/src/lib.rs", "")
        .build();

    p.cargo("build").run();

    File::create(p.root().join("a/Cargo.toml"))
        .unwrap()
        .write_all(
            format!(
                r#"
        [package]
        name = "a"
        version = "0.0.1"
        authors = []

        [dependencies]
        git = {{ git = '{}' }}
    "#,
                git.url()
            )
            .as_bytes(),
        )
        .unwrap();

    p.cargo("build").run();
}

#[test]
fn two_at_rev_instead_of_tag() {
    let git = git::new("git", |p| {
        p.file("Cargo.toml", &basic_manifest("git1", "0.5.0"))
            .file("src/lib.rs", "")
            .file("a/Cargo.toml", &basic_manifest("git2", "0.5.0"))
            .file("a/src/lib.rs", "")
    })
    .unwrap();

    // Make a tag corresponding to the current HEAD
    let repo = git2::Repository::open(&git.root()).unwrap();
    let head = repo.head().unwrap().target().unwrap();
    repo.tag(
        "v0.1.0",
        &repo.find_object(head, None).unwrap(),
        &repo.signature().unwrap(),
        "make a new tag",
        false,
    )
    .unwrap();

    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [package]
            name = "foo"
            version = "0.0.1"
            authors = []

            [dependencies]
            git1 = {{ git = '{0}', rev = 'v0.1.0' }}
            git2 = {{ git = '{0}', rev = 'v0.1.0' }}
        "#,
                git.url()
            ),
        )
        .file("src/lib.rs", "")
        .build();

    p.cargo("generate-lockfile").run();
    p.cargo("build -v").run();
}

#[test]
fn include_overrides_gitignore() {
    // Make sure that `package.include` takes precedence over .gitignore.
    let p = git::new("foo", |repo| {
        repo.file(
            "Cargo.toml",
            r#"
            [package]
            name = "foo"
            version = "0.5.0"
            include = ["src/lib.rs", "ignored.txt", "Cargo.toml"]
        "#,
        )
        .file(
            ".gitignore",
            r#"
            /target
            Cargo.lock
            ignored.txt
        "#,
        )
        .file("src/lib.rs", "")
        .file("ignored.txt", "")
        .file("build.rs", "fn main() {}")
    })
    .unwrap();

    p.cargo("build").run();
    p.change_file("ignored.txt", "Trigger rebuild.");
    p.cargo("build -v")
        .with_stderr(
            "\
[COMPILING] foo v0.5.0 ([..])
[RUNNING] `[..]build-script-build[..]`
[RUNNING] `rustc --crate-name foo src/lib.rs [..]`
[FINISHED] dev [unoptimized + debuginfo] target(s) in [..]
",
        )
        .run();
    p.cargo("package --list --allow-dirty")
        .with_stdout(
            "\
Cargo.toml
ignored.txt
src/lib.rs
",
        )
        .run();
}

#[test]
fn invalid_git_dependency_manifest() {
    let project = project();
    let git_project = git::new("dep1", |project| {
        project
            .file(
                "Cargo.toml",
                r#"
                [project]

                name = "dep1"
                version = "0.5.0"
                authors = ["carlhuda@example.com"]
                categories = ["algorithms"]
                categories = ["algorithms"]

                [lib]

                name = "dep1"
            "#,
            )
            .file(
                "src/dep1.rs",
                r#"
                pub fn hello() -> &'static str {
                    "hello world"
                }
            "#,
            )
    })
    .unwrap();

    let project = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]

            name = "foo"
            version = "0.5.0"
            authors = ["wycats@example.com"]

            [dependencies.dep1]

            git = '{}'
        "#,
                git_project.url()
            ),
        )
        .file(
            "src/main.rs",
            &main_file(r#""{}", dep1::hello()"#, &["dep1"]),
        )
        .build();

    let git_root = git_project.root();

    project
        .cargo("build")
        .with_status(101)
        .with_stderr(&format!(
            "[UPDATING] git repository `{}`\n\
             error: failed to load source for a dependency on `dep1`\n\
             \n\
             Caused by:\n  \
             Unable to update {}\n\
             \n\
             Caused by:\n  \
             failed to parse manifest at `[..]`\n\
             \n\
             Caused by:\n  \
             could not parse input as TOML\n\
             \n\
             Caused by:\n  \
             duplicate key: `categories` for key `project`",
            path2url(&git_root),
            path2url(&git_root),
        ))
        .run();
}

#[test]
fn failed_submodule_checkout() {
    let project = project();
    let git_project = git::new("dep1", |project| {
        project.file("Cargo.toml", &basic_manifest("dep1", "0.5.0"))
    })
    .unwrap();

    let git_project2 = git::new("dep2", |project| project.file("lib.rs", "")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let done2 = done.clone();

    let t = thread::spawn(move || {
        while !done2.load(Ordering::SeqCst) {
            if let Ok((mut socket, _)) = listener.accept() {
                drop(socket.write_all(b"foo\r\n"));
            }
        }
    });

    let repo = git2::Repository::open(&git_project2.root()).unwrap();
    let url = format!("https://{}:{}/", addr.ip(), addr.port());
    {
        let mut s = repo.submodule(&url, Path::new("bar"), false).unwrap();
        let subrepo = s.open().unwrap();
        let mut cfg = subrepo.config().unwrap();
        cfg.set_str("user.email", "foo@bar.com").unwrap();
        cfg.set_str("user.name", "Foo Bar").unwrap();
        git::commit(&subrepo);
        s.add_finalize().unwrap();
    }
    git::commit(&repo);
    drop((repo, url));

    let repo = git2::Repository::open(&git_project.root()).unwrap();
    let url = path2url(git_project2.root()).to_string();
    git::add_submodule(&repo, &url, Path::new("src"));
    git::commit(&repo);
    drop(repo);

    let project = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
            [project]
            name = "foo"
            version = "0.5.0"
            authors = []

            [dependencies]
            dep1 = {{ git = '{}' }}
        "#,
                git_project.url()
            ),
        )
        .file("src/lib.rs", "")
        .build();

    project
        .cargo("build")
        .with_status(101)
        .with_stderr_contains("  failed to update submodule `src`")
        .with_stderr_contains("  failed to update submodule `bar`")
        .run();
    project
        .cargo("build")
        .with_status(101)
        .with_stderr_contains("  failed to update submodule `src`")
        .with_stderr_contains("  failed to update submodule `bar`")
        .run();

    done.store(true, Ordering::SeqCst);
    drop(TcpStream::connect(&addr));
    t.join().unwrap();
}

#[test]
fn use_the_cli() {
    if disable_git_cli() {
        return;
    }
    let project = project();
    let git_project = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_manifest("dep1", "0.5.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();

    let project = project
        .file(
            "Cargo.toml",
            &format!(
                r#"
                    [project]
                    name = "foo"
                    version = "0.5.0"
                    authors = []

                    [dependencies]
                    dep1 = {{ git = '{}' }}
                "#,
                git_project.url()
            ),
        )
        .file("src/lib.rs", "")
        .file(
            ".cargo/config",
            "
                [net]
                git-fetch-with-cli = true
            ",
        )
        .build();

    let stderr = "\
[UPDATING] git repository `[..]`
[RUNNING] `git fetch [..]`
[COMPILING] dep1 [..]
[RUNNING] `rustc [..]`
[COMPILING] foo [..]
[RUNNING] `rustc [..]`
[FINISHED] [..]
";

    project.cargo("build -v").with_stderr(stderr).run();
}

#[test]
fn templatedir_doesnt_cause_problems() {
    let git_project2 = git::new("dep2", |project| {
        project
            .file("Cargo.toml", &basic_manifest("dep2", "0.5.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();
    let git_project = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_manifest("dep1", "0.5.0"))
            .file("src/lib.rs", "")
    })
    .unwrap();
    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
                [project]
                name = "fo"
                version = "0.5.0"
                authors = []

                [dependencies]
                dep1 = {{ git = '{}' }}
            "#,
                git_project.url()
            ),
        )
        .file("src/main.rs", "fn main() {}")
        .build();

    File::create(paths::home().join(".gitconfig"))
        .unwrap()
        .write_all(
            format!(
                r#"
                [init]
                templatedir = {}
            "#,
                git_project2
                    .url()
                    .to_file_path()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace("\\", "/")
            )
            .as_bytes(),
        )
        .unwrap();

    p.cargo("build").run();
}

#[test]
fn git_with_cli_force() {
    if disable_git_cli() {
        return;
    }
    // Supports a force-pushed repo.
    let git_project = git::new("dep1", |project| {
        project
            .file("Cargo.toml", &basic_lib_manifest("dep1"))
            .file("src/lib.rs", r#"pub fn f() { println!("one"); }"#)
    })
    .unwrap();
    let p = project()
        .file(
            "Cargo.toml",
            &format!(
                r#"
                [project]
                name = "foo"
                version = "0.0.1"
                edition = "2018"

                [dependencies]
                dep1 = {{ git = "{}" }}
                "#,
                git_project.url()
            ),
        )
        .file("src/main.rs", "fn main() { dep1::f(); }")
        .file(
            ".cargo/config",
            "
            [net]
            git-fetch-with-cli = true
            ",
        )
        .build();
    p.cargo("build").run();
    p.rename_run("foo", "foo1").with_stdout("one").run();

    // commit --amend a change that will require a force fetch.
    let repo = git2::Repository::open(&git_project.root()).unwrap();
    git_project.change_file("src/lib.rs", r#"pub fn f() { println!("two"); }"#);
    git::add(&repo);
    let id = repo.refname_to_id("HEAD").unwrap();
    let commit = repo.find_commit(id).unwrap();
    let tree_id = t!(t!(repo.index()).write_tree());
    t!(commit.amend(
        Some("HEAD"),
        None,
        None,
        None,
        None,
        Some(&t!(repo.find_tree(tree_id)))
    ));
    // Perform the fetch.
    p.cargo("update").run();
    p.cargo("build").run();
    p.rename_run("foo", "foo2").with_stdout("two").run();
}
