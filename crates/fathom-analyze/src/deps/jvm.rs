//! The JVM surface: Maven `pom.xml`, Gradle build files and Gradle dependency locks (e2.5 / reco-j83).
//!
//! Feature-gated behind `deps-jvm`.
//!
//! # "Best-effort, read-only" — and what that costs
//!
//! The ticket asks for Maven and Gradle best-effort, and the honest reason it can only be best-effort is
//! that a real answer needs to *execute build logic*: a POM resolves `${revision}` through a parent chain
//! and property interpolation, and a Gradle build is a program. Fathom runs neither — no `mvn`, no
//! `gradle`, no child process on repository content (ADR-001 §8.2, and `fathom-ingest`'s no-subprocess
//! test makes that a checked property of the whole tool). So:
//!
//! * a POM is read as written, with `${property}` left verbatim in the requirement and recorded in
//!   `properties` when it came from `<properties>` — a reviewer sees `spring-core ${spring.version}`, and
//!   the raw diff is one keystroke away;
//! * a Gradle build file is scanned for the dependency-declaration forms, including the map form and
//!   `platform(...)`/`enforcedPlatform(...)` and `project(':x')` workspace edges;
//! * the parent POM's own coordinates are captured (`parent` in `properties`) because "a new parent" is a
//!   supply-chain event even when no `<dependency>` line changed.
//!
//! A file that fails to parse is still reported — [`UnparsedReason`](crate::deps::model::UnparsedReason)
//! with the raw preview — never dropped.

use crate::deps::Failure;
use crate::deps::model::{
    Body, DepKind, Dependency, Format, Lock, LockPackage, Manifest, Source, UnparsedReason,
};
use crate::deps::text::Notes;
use crate::deps::xml::{self, Element};

/// Parse one JVM format.
pub fn parse(format: Format, text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    match format {
        Format::PomXml => pom(text, notes),
        Format::BuildGradle => gradle(text, notes),
        Format::GradleLockfile => gradle_lockfile(text, notes),
        other => Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!("{:?} is not a JVM format", other),
        )),
    }
}

/// Configurations that mean "this is a dependency of the project".
const GRADLE_CONFIGURATIONS: [&str; 24] = [
    "api",
    "implementation",
    "compileOnly",
    "compileOnlyApi",
    "runtimeOnly",
    "annotationProcessor",
    "testImplementation",
    "testCompileOnly",
    "testRuntimeOnly",
    "androidTestImplementation",
    "kapt",
    "ksp",
    "classpath",
    "coreImplementation",
    "detektPlugins",
    "errorprone",
    "lintChecks",
    "provided",
    "testAnnotationProcessor",
    "testFixturesApi",
    "testFixturesImplementation",
    "optional",
    "compile",
    "providedCompile",
];

fn pom(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let root = xml::parse(text).map_err(Failure::malformed)?;
    if !root.is("project") {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            format!(
                "the POM's root element is `<{}>`, not `<project>`",
                root.name
            ),
        ));
    }
    let mut manifest = Manifest::default();
    let group = root.child_text("groupId").map(str::to_owned);
    let artifact = root.child_text("artifactId").map(str::to_owned);
    let version = root.child_text("version").map(str::to_owned);
    manifest.name = match (&group, &artifact) {
        (Some(group), Some(artifact)) => Some(format!("{group}:{artifact}")),
        (None, Some(artifact)) => Some(artifact.clone()),
        _ => None,
    };
    manifest.version = version;
    if let Some(packaging) = root.child_text("packaging") {
        manifest
            .properties
            .insert("packaging".to_owned(), packaging.to_owned());
    }
    if let Some(parent) = root.child("parent") {
        let coordinate = |key: &str| parent.child_text(key).map(str::to_owned);
        manifest.properties.insert(
            "parent".to_owned(),
            [
                coordinate("groupId").unwrap_or_default(),
                coordinate("artifactId").unwrap_or_default(),
                coordinate("version").unwrap_or_default(),
            ]
            .iter()
            .filter(|part| !part.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(":"),
        );
        // An inherited groupId/version is the project's own coordinate as Maven sees it.
        if manifest.version.is_none() {
            manifest.version = parent.child_text("version").map(str::to_owned);
        }
        if group.is_none() {
            manifest.name = parent
                .child_text("groupId")
                .map(|group| format!("{group}:{}", artifact.clone().unwrap_or_default()));
        }
    }
    if let Some(properties) = root.child("properties") {
        for property in &properties.children {
            let value = property.content();
            if !value.is_empty() {
                manifest
                    .properties
                    .insert(format!("property.{}", property.name), value.to_owned());
            }
        }
    }
    for modules in root.all("modules") {
        for module in modules.all("module") {
            if !module.content().is_empty() {
                manifest.workspaces.push(module.content().to_owned());
            }
        }
    }

    collect_pom_dependencies(
        root.child("dependencies"),
        DepKind::Normal,
        &mut manifest,
        notes,
    );
    if let Some(management) = root.child("dependencyManagement") {
        for dependencies in management.all("dependencies") {
            // `<dependencyManagement>` pins a version a `<dependency>` elsewhere may omit. It is not a
            // dependency of this module — nothing is compiled against it — so it is not `Provided`; calling
            // it `managed` keeps it out of the runtime surface while keeping the pin visible.
            collect_pom_dependencies(
                Some(dependencies),
                DepKind::Other("managed".to_owned()),
                &mut manifest,
                notes,
            );
        }
    }
    if let Some(profiles) = root.child("profiles") {
        for profile in profiles.all("profile") {
            let name = profile.child_text("id").unwrap_or("profile").to_owned();
            for dependencies in profile.all("dependencies") {
                collect_pom_dependencies(
                    Some(dependencies),
                    DepKind::Other(format!("profile:{name}")),
                    &mut manifest,
                    notes,
                );
            }
        }
    }
    if let Some(build) = root.child("build") {
        collect_pom_dependencies(
            build.child("dependencies"),
            DepKind::Build,
            &mut manifest,
            notes,
        );
        for plugins in build.all("plugin") {
            let coordinate = [
                plugins
                    .child_text("groupId")
                    .unwrap_or("org.apache.maven.plugins"),
                plugins.child_text("artifactId").unwrap_or_default(),
                plugins.child_text("version").unwrap_or_default(),
            ]
            .join(":");
            manifest
                .properties
                .entry("plugins".to_owned())
                .and_modify(|existing| existing.push(','))
                .or_insert_with(|| coordinate.clone());
        }
    }
    crate::deps::model::finish_manifest(manifest)
}

fn collect_pom_dependencies(
    dependencies: Option<&Element>,
    kind: DepKind,
    manifest: &mut Manifest,
    notes: &mut Notes,
) {
    let Some(dependencies) = dependencies else {
        return;
    };
    for dependency in dependencies.all("dependency") {
        let group = dependency.child_text("groupId").unwrap_or_default();
        let artifact = dependency.child_text("artifactId").unwrap_or_default();
        if artifact.is_empty() {
            notes.push("a `<dependency>` with no `<artifactId>` was skipped".to_owned());
            continue;
        }
        let version = dependency.child_text("version").unwrap_or_default();
        let scope = dependency.child_text("scope").unwrap_or_default();
        let kind = match scope {
            "test" => DepKind::Dev,
            "provided" => DepKind::Provided,
            "system" => DepKind::Provided,
            "runtime" => DepKind::Normal,
            "" => kind.clone(),
            other => DepKind::Other(format!("scope:{other}")),
        };
        let mut source = Source::registry();
        if let Some(classifier) = dependency.child_text("classifier") {
            manifest.properties.insert(
                format!("classifier.{group}:{artifact}"),
                classifier.to_owned(),
            );
        }
        if dependency
            .child_text("optional")
            .is_some_and(|value| value == "true")
        {
            manifest.requirements.push(Dependency {
                name: format!("{group}:{artifact}"),
                requirement: version.to_owned(),
                kind: DepKind::Optional,
                source,
                actual: None,
            });
            continue;
        }
        if version.contains("${") {
            notes.push(format!(
                "`{group}:{artifact}` is declared as `{version}`; the property is not resolved"
            ));
        }
        if let Some(system_path) = dependency.child_text("systemPath") {
            source = Source::Path {
                path: system_path.to_owned(),
            };
        }
        manifest.requirements.push(Dependency {
            name: format!("{group}:{artifact}"),
            requirement: version.to_owned(),
            kind,
            source,
            actual: None,
        });
    }
}

/// `build.gradle`, `build.gradle.kts` and `settings.gradle`, by declaration form.
fn gradle(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let mut manifest = Manifest::default();
    let mut declarations = 0usize;
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = strip_java_comment(raw.trim());
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("include ") {
            // settings.gradle: membership, not dependencies.
            for member in rest.split(',') {
                let member = member.trim().trim_matches(['\'', '"']);
                if !member.is_empty() {
                    manifest
                        .workspaces
                        .push(member.trim_start_matches(':').to_owned());
                }
            }
            continue;
        }
        if line.starts_with("plugins") || line.starts_with("pluginManagement") {
            continue;
        }
        let Some(configuration) = GRADLE_CONFIGURATIONS
            .iter()
            .find(|configuration| line.starts_with(**configuration))
            .copied()
        else {
            continue;
        };
        let mut tail = line[configuration.len()..]
            .trim_start()
            .trim_start_matches('(');
        let kind = if configuration.starts_with("test") || configuration.starts_with("androidTest")
        {
            DepKind::Dev
        } else if configuration == "compileOnly" || configuration == "provided" {
            DepKind::Provided
        } else if configuration == "classpath" || configuration == "buildSrc" {
            DepKind::Build
        } else {
            DepKind::Normal
        };
        // `platform("g:a:v")` and `enforcedPlatform("g:a:v")` wrap the coordinate in a call of their own,
        // and BOM imports are the only way a Gradle build pins a whole version set — they are part of the
        // dependency surface even though nothing is compiled against the BOM itself.
        for wrapper in ["enforcedPlatform(", "platform(", "testFixtures(project("] {
            if let Some(rest) = tail.strip_prefix(wrapper) {
                tail = rest;
                if tail.ends_with(')') {
                    tail = &tail[..tail.len() - 1];
                }
            }
        }
        let tail = tail.trim_start_matches('(').trim_start();
        if tail.starts_with("project(") {
            let name = tail
                .trim_start_matches("project(")
                .trim_end_matches(')')
                .trim_matches(['\'', '"'])
                .to_owned();
            manifest.requirements.push(Dependency {
                name: if name.is_empty() {
                    "project".to_owned()
                } else {
                    name.clone()
                },
                requirement: "project".to_owned(),
                kind: kind.clone(),
                source: Source::Path { path: name },
                actual: None,
            });
            declarations += 1;
            continue;
        }
        // The map form first: `group: "g", name: "a", version: "v"` contains colons and would otherwise be
        // mistaken for a `g:a:v` coordinate whose parts happen to be Gradle's own keywords.
        if let Some(coordinate) = gradle_map(tail) {
            manifest.requirements.push(Dependency {
                name: coordinate.0,
                requirement: coordinate.1,
                kind,
                source: Source::registry(),
                actual: None,
            });
            declarations += 1;
            continue;
        }
        if let Some(coordinate) = coordinate(tail) {
            let (name, requirement) = collapse_gradle(coordinate);
            manifest.requirements.push(Dependency {
                name,
                requirement,
                kind,
                source: Source::registry(),
                actual: None,
            });
            declarations += 1;
            continue;
        }
        notes.at(
            index + 1,
            format!("`{configuration}` with no readable coordinate"),
        );
    }
    if declarations == 0 && manifest.workspaces.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a Gradle build file with no dependency declarations in it",
        ));
    }
    manifest
        .properties
        .insert("declarations".to_owned(), declarations.to_string());
    crate::deps::model::finish_manifest(manifest)
}

/// `g:a:v`, `g:a:v:classifier`, or nothing at all.
fn coordinate(text: &str) -> Option<&str> {
    let text = text
        .trim()
        .trim_end_matches([')', ','])
        .trim_matches(['\'', '"']);
    let parts: Vec<&str> = text.split(':').collect();
    (parts.len() >= 3).then_some(text)
}

/// Split a two-or-three-part Gradle coordinate into (name, version).
///
/// `com.acme:billing:2.1` names `com.acme:billing` at `2.1`. The configuration (`implementation`,
/// `testRuntimeOnly`) is already carried in the `Dependency`'s kind, so the splitter does not repeat it.
fn collapse_gradle(coordinate: &str) -> (String, String) {
    let parts: Vec<&str> = coordinate.split(':').collect();
    let name = parts[..parts.len() - 1].join(":");
    let version = parts[parts.len() - 1].to_owned();
    (name, version)
}

/// The named-argument form Gradle accepts.
fn gradle_map(text: &str) -> Option<(String, String)> {
    let mut group = None;
    let mut name = None;
    let mut version = None;
    for part in text.split(',') {
        let Some((key, value)) = part.split_once(':') else {
            continue;
        };
        let value = value
            .trim()
            .trim_end_matches([')', ','])
            .trim_matches(['\'', '"']);
        match key.trim() {
            "group" => group = Some(value.to_owned()),
            "name" | "module" => name = Some(value.to_owned()),
            "version" => version = Some(value.to_owned()),
            _ => {}
        }
    }
    let name = name?;
    Some((
        group.map_or_else(|| name.clone(), |group| format!("{group}:{name}")),
        version.unwrap_or_default(),
    ))
}

/// Gradle's dependency lockfiles: `<configuration>=g:a:v`, one per line, plus `empty <configuration>`.
fn gradle_lockfile(text: &str, notes: &mut Notes) -> Result<Body, Failure> {
    let mut lock = Lock {
        version: None,
        packages: Vec::new(),
        importers: Vec::new(),
        records_edges: false,
        // Gradle's lock records one row per (configuration, coordinate) and no graph at all.
        complete: false,
        properties: Default::default(),
    };
    let mut configurations: Vec<String> = Vec::new();
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("#") {
            continue;
        }
        if line == line.trim_start_matches('#') {
            // no comment prefix on this line
        }
        if let Some(rest) = line.strip_prefix("empty ") {
            configurations.push(rest.to_owned());
            continue;
        }
        let Some((coordinate, configuration)) = line.split_once('=') else {
            notes.at(
                index + 1,
                format!(
                    "`{}` is not `configuration=g:a:v`",
                    crate::deps::text::clip(line, 40)
                ),
            );
            continue;
        };
        let parts: Vec<&str> = coordinate.split(':').collect();
        if parts.len() < 3 {
            notes.at(index + 1, format!("`{coordinate}` is not a coordinate"));
            continue;
        }
        configurations.push(configuration.to_owned());
        lock.packages.push(LockPackage {
            name: format!("{}:{}", parts[0], parts[1]),
            version: parts[2].to_owned(),
            source: Source::registry(),
            // A fourth part is a classifier (`g:a:v:apk`), not a digest; Gradle's lockfile carries no
            // integrity information at all, so `integrity` stays `None` and says so.
            integrity: None,
            license: None,
            edges: Vec::new(),
            // The configuration is the only place a Gradle lock says what a row is for.
            dev: configuration.starts_with("test") || configuration.starts_with("androidTest"),
            direct: None,
        });
    }
    if lock.packages.is_empty() && configurations.is_empty() {
        return Err(Failure::new(
            UnparsedReason::UnknownShape,
            "a Gradle lock file with no rows",
        ));
    }
    configurations.sort();
    configurations.dedup();
    lock.properties
        .insert("configurations".to_owned(), configurations.join(","));
    crate::deps::model::sort_packages(&mut lock.packages);
    Ok(Body::Lock(lock))
}

fn strip_java_comment(line: &str) -> String {
    for marker in ["//", "/*"] {
        if let Some(index) = line.find(marker) {
            return line[..index].to_owned();
        }
    }
    line.to_owned()
}
