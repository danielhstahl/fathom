//! Workspaces and monorepos: one scan, many packages (e2.5 / reco-j83, AC 3).
//!
//! The unit of a dependency surface is not the repository — it is the *package*, and a monorepo holds dozens.
//! A scan that flattens them into one list answers the wrong question: "did `react` change" is meaningless if
//! `apps/web` and `apps/admin` each declare their own `react`, and "a dependency was added" is either a
//! useful alert or a duplicate of one you already got, depending on which member it happened in.
//!
//! So everything here is built in memory (paths as strings, bytes as vectors): `scan` takes inputs, and the
//! interesting properties — which manifest owns which lock, which directory is a member, what a workspace glob
//! claims, which directories are ignored — are properties of the *path structure*, not of the filesystem.
//! Building the trees in code keeps the assertions next to the trees they describe, and keeps a `cargo test`
//! from needing to write files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use fathom_analyze::deps::{self, Content, Input, Member, Options, Status, Surface};

/// Scan a `(path, content)` list as one repository.
fn scan_tree(tree: &[(&str, &str)]) -> Surface {
    let inputs: Vec<Input<'_>> = tree
        .iter()
        .map(|(path, content)| Input {
            path,
            content: Content::Bytes(content.as_bytes()),
        })
        .collect();
    deps::scan(inputs, &Options::default())
}

fn member<'a>(surface: &'a Surface, dir: &str) -> &'a Member {
    surface
        .members
        .iter()
        .find(|member| member.dir == dir)
        .unwrap_or_else(|| {
            panic!(
                "`{dir}` is not a member; members are {:?}",
                surface
                    .members
                    .iter()
                    .map(|member| member.dir.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

/// An npm workspace: a root that declares members, two apps that each have their own manifest, one of them
/// with a lockfile, and a `node_modules` tree full of `package.json` files that are not this repo's business.
fn npm_workspace() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "package.json",
            r#"{
  "name": "monorepo-root",
  "private": true,
  "workspaces": ["apps/*", "packages/*"],
  "devDependencies": { "prettier": "^3.0.0" }
}"#,
        ),
        (
            "apps/web/package.json",
            r#"{
  "name": "@acme/web",
  "version": "1.0.0",
  "dependencies": { "react": "^18.2.0", "@acme/ui": "workspace:*" }
}"#,
        ),
        (
            "apps/web/package-lock.json",
            r#"{
  "name": "@acme/web",
  "lockfileVersion": 3,
  "packages": {
    "": { "dependencies": { "react": "^18.2.0" } },
    "node_modules/react": {
      "version": "18.2.0",
      "resolved": "https://registry.npmjs.org/react/-/react-18.2.0.tgz",
      "integrity": "sha512-/XJ"
    }
  }
}"#,
        ),
        (
            "apps/admin/package.json",
            r#"{
  "name": "@acme/admin",
  "version": "0.4.0",
  "dependencies": { "react": "^17.0.2" }
}"#,
        ),
        (
            "packages/ui/package.json",
            r#"{ "name": "@acme/ui", "version": "2.0.0", "peerDependencies": { "react": ">=17" } }"#,
        ),
        // Vendored copies: recognized by name, ignored by rule. A scan that read these would report every
        // transitive package as a member of the repository.
        (
            "node_modules/react/package.json",
            r#"{ "name": "react", "version": "18.2.0" }"#,
        ),
        (
            "apps/web/node_modules/has-package.json",
            r#"{ "name": "junk" }"#,
        ),
        (
            "packages/ui/node_modules/tslib/package.json",
            r#"{ "name": "tslib", "version": "2.6.2" }"#,
        ),
    ]
}

#[test]
fn each_workspace_member_is_its_own_member_with_its_own_lockfile() {
    let surface = scan_tree(&npm_workspace());
    assert_eq!(
        surface.members.len(),
        4,
        "expected the root plus three workspace members: {:?}",
        surface
            .members
            .iter()
            .map(|m| (&m.dir, &m.manifest))
            .collect::<Vec<_>>()
    );
    assert_eq!(member(&surface, "").name.as_deref(), Some("monorepo-root"));
    assert_eq!(
        member(&surface, "apps/web").name.as_deref(),
        Some("@acme/web")
    );
    assert_eq!(
        member(&surface, "apps/admin").name.as_deref(),
        Some("@acme/admin")
    );
    assert_eq!(
        member(&surface, "packages/ui").name.as_deref(),
        Some("@acme/ui")
    );

    // The lockfile belongs to the member that declares the package it locks, not to the repository root.
    assert_eq!(
        member(&surface, "apps/web").locks,
        vec!["apps/web/package-lock.json".to_owned()]
    );
    assert!(member(&surface, "apps/admin").locks.is_empty());
    assert_eq!(member(&surface, "apps/web").resolved, 1);
    assert_eq!(member(&surface, "apps/admin").resolved, 0);

    // Each member counts its own requirements; the same name in two members is two requirements.
    assert_eq!(member(&surface, "apps/web").direct, 2);
    assert_eq!(member(&surface, "apps/admin").direct, 1);
    assert_eq!(surface.totals.direct, 5); // 1 root + 2 web + 1 admin + 1 ui (a peer is still a requirement)
}

#[test]
fn vendored_and_generated_directories_are_not_members() {
    let surface = scan_tree(&npm_workspace());
    assert_eq!(
        surface.totals.files, 5,
        "only the five real files are recognized"
    );
    for file in &surface.files {
        assert!(
            !file.path.contains("node_modules"),
            "a vendored file was scanned: {}",
            file.path
        );
    }
    // The scan says why it skipped them, rather than quietly returning fewer files.
    assert!(
        surface
            .notes
            .iter()
            .any(|note| note.contains("node_modules")),
        "no note explains the ignored directories: {:?}",
        surface.notes
    );
}

#[test]
fn workspace_globs_claim_members_and_an_unclaimed_one_is_called_out() {
    let surface = scan_tree(&npm_workspace());
    for dir in ["apps/web", "apps/admin", "packages/ui"] {
        assert!(
            member(&surface, dir).claimed,
            "`{dir}` matches `apps/*` or `packages/*` and should be claimed"
        );
    }
    // A member outside every glob is a real finding: the tool that reads the surface will not see it, and the
    // person who added the directory is usually the last to know.
    let mut tree = npm_workspace();
    tree.push((
        "experimental/sandbox/package.json",
        r#"{ "name": "sandbox", "dependencies": { "left-pad": "1.3.0" } }"#,
    ));
    let surface = scan_tree(&tree);
    let sandbox = member(&surface, "experimental/sandbox");
    assert!(!sandbox.claimed);
    assert!(
        surface
            .notes
            .iter()
            .any(|note| note.contains("experimental/sandbox")),
        "an unclaimed member produced no note: {:?}",
        surface.notes
    );
}

#[test]
fn a_cargo_workspace_is_one_lock_over_many_members() {
    let tree = [
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/*\"]\nresolver = \"2\"\n",
        ),
        (
            "crates/widget/Cargo.toml",
            "[package]\nname = \"widget\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n",
        ),
        (
            "crates/gadget/Cargo.toml",
            "[package]\nname = \"gadget\"\nversion = \"0.2.0\"\n\n[dependencies]\nclap = \"4\"\n",
        ),
        (
            "crates/gadget/Cargo.lock",
            "version = 4\n\n[[package]]\nname = \"gadget\"\nversion = \"0.2.0\"\n\n[[package]]\nname = \"clap\"\nversion = \"4.5.4\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"77aa\"\n",
        ),
    ];
    let surface = scan_tree(&tree);
    assert_eq!(surface.members.len(), 3);
    assert_eq!(
        member(&surface, "").ecosystem,
        fathom_analyze::deps::Ecosystem::Cargo
    );
    // A virtual workspace manifest packages nothing, and says so without being downgraded for it.
    assert_eq!(member(&surface, "").status, Status::Parsed);
    assert_eq!(member(&surface, "crates/widget").direct, 1);
    assert_eq!(member(&surface, "crates/gadget").resolved, 2);
    assert_eq!(
        member(&surface, "crates/gadget").locks,
        vec!["crates/gadget/Cargo.lock".to_owned()]
    );
}

#[test]
fn one_repository_can_hold_every_ecosystem_at_once() {
    let tree = [
        (
            "package.json",
            r#"{ "name": "js", "dependencies": { "react": "^18" } }"#,
        ),
        (
            "services/api/pyproject.toml",
            "[project]\nname = \"api\"\nversion = \"1.0.0\"\ndependencies = [\"fastapi>=0.110\"]\n",
        ),
        (
            "services/api/poetry.lock",
            "[[package]]\nname = \"fastapi\"\nversion = \"0.110.0\"\n",
        ),
        (
            "services/go/go.mod",
            "module example.com/go\n\ngo 1.22\n\nrequire github.com/lib/pq v1.10.9\n",
        ),
        ("services/go/go.sum", "github.com/lib/pq v1.10.9 h1:ky1\n"),
        (
            "tools/rb/Gemfile",
            "source \"https://rubygems.org\"\n\ngem \"rake\", \"~> 13.0\"\n",
        ),
        (
            "tools/rb/Gemfile.lock",
            "GEM\n  remote: https://rubygems.org/\n  specs:\n    rake (13.1.0)\n\nPLATFORMS\n  ruby\n\nDEPENDENCIES\n  rake (~> 13.0)\n\nBUNDLED WITH\n   2.5.6\n",
        ),
        (
            "jvm/service/pom.xml",
            "<project><artifactId>service</artifactId><version>1.0.0</version>\
             <dependencies><dependency><groupId>junit</groupId><artifactId>junit</artifactId>\
             <version>4.13.2</version><scope>test</scope></dependency></dependencies></project>",
        ),
    ];
    let surface = scan_tree(&tree);
    let by_ecosystem = &surface.totals.by_ecosystem;
    for ecosystem in ["js", "python", "go", "ruby", "jvm"] {
        assert!(
            by_ecosystem.contains_key(ecosystem),
            "`{ecosystem}` is missing from {by_ecosystem:?}"
        );
    }
    assert_eq!(surface.members.len(), 5);
    assert_eq!(surface.totals.files, 8);
    assert_eq!(surface.totals.manifests, 5);
    assert_eq!(surface.totals.locks, 3);
    assert!(surface.complete);
    assert_eq!(
        surface.totals.parsed + surface.totals.partial + surface.totals.unparsed,
        surface.totals.files
    );
}

#[test]
fn a_lockfile_with_no_manifest_still_shows_up() {
    // The committed lockfile with no manifest is a real situation — a submodule, a half-deleted migration —
    // and the resolved surface it describes is the part a reviewer most needs to see.
    let tree = [(
        "stray/package-lock.json",
        r#"{ "name": "stray", "lockfileVersion": 3, "packages": { "node_modules/react": { "version": "18.2.0" } } }"#,
    )];
    let surface = scan_tree(&tree);
    assert_eq!(surface.totals.files, 1);
    assert_eq!(surface.totals.locks, 1);
    assert_eq!(surface.totals.resolved, 1);
    assert!(
        !surface.notes.is_empty(),
        "an orphan lockfile should be explained somewhere"
    );
}

#[test]
fn a_many_member_repository_scans_in_one_pass() {
    // Fifty members is not exotic; the point is that membership does not degrade with size and that the same
    // directory name in two ecosystems stays two members.
    let mut tree: Vec<(String, String)> = Vec::new();
    for index in 0..25 {
        tree.push((
            format!("apps/app{index}/package.json"),
            format!(
                "{{ \"name\": \"app{index}\", \"dependencies\": {{ \"react\": \"^{index}\" }} }}"
            ),
        ));
        tree.push((
            format!("services/service{index}/pyproject.toml"),
            format!(
                "[project]\nname = \"service{index}\"\nversion = \"1.0.{index}\"\ndependencies = [\"requests>=2.31\"]\n"
            ),
        ));
    }
    let borrowed: Vec<(&str, &str)> = tree
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect();
    let surface = scan_tree(&borrowed);
    assert_eq!(surface.members.len(), 50);
    assert_eq!(surface.totals.files, 50);
    assert_eq!(surface.totals.direct, 50);
    assert!(surface.members.iter().all(|member| member.direct == 1));
    // Nothing in this tree declares workspaces, so nothing is claimed — and every member is reported as such.
    assert!(surface.members.iter().all(|member| !member.claimed));
}

#[test]
fn a_config_file_is_not_a_manifest_but_changes_the_shape_of_the_scan() {
    // `pnpm-workspace.yaml` and `go.work` declare membership without declaring dependencies.
    let tree = [
        (
            "pnpm-workspace.yaml",
            "packages:\n  - 'apps/*'\n  - 'packages/*'\n",
        ),
        (
            "apps/web/package.json",
            r#"{ "name": "web", "dependencies": { "react": "^18" } }"#,
        ),
        ("go.work", "go 1.22\n\nuse (\n\t./api\n\t./worker\n)\n"),
        ("api/go.mod", "module example.com/api\n\ngo 1.22\n"),
        ("worker/go.mod", "module example.com/worker\n\ngo 1.22\n"),
    ];
    let surface = scan_tree(&tree);
    assert_eq!(surface.totals.configs, 2);
    for dir in ["apps/web", "api", "worker"] {
        assert!(
            member(&surface, dir).claimed,
            "`{dir}` is named by a workspace config and should be claimed"
        );
    }
}
