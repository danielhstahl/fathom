//! The repository-scale Java corpus the same-package scan is measured on (`e3.1b`, `reco-dlp.1`).
//!
//! `e0.12` turned s1's finding 5 into an acceptance criterion: *"Java same-package resolution is
//! re-measured on a corpus of at least 1 000 files with a published false-positive rate and a stated
//! budget"*. Nobody is going to hand-annotate 1 000 Java files, so this corpus is **machine-authored,
//! and that is stated in the report rather than hidden** — what makes the measurement worth anything
//! is that the generator plants every dependency it expects and nothing else, so "false positive"
//! means *the extractor claimed an edge the file does not contain* and not *the annotator missed one*.
//!
//! The construction is the interesting part, because a same-package scan only fails on collisions:
//!
//! * **~60 type names, reused across every package.** `Builder`, `Result`, `Config`, `Registry` and
//!   the rest of the names s1 named as the risk. A pool of 60 across 40 packages of 30 types each
//!   guarantees that a simple name means several different files, which is the only condition under
//!   which an unscoped scan can be wrong.
//! * **Every file references two same-package types declared in *other* files** — those are the
//!   planted edges, and missing one is a recall miss.
//! * **Every file also imports a type by name from another package**, and roughly half of those names
//!   also exist in the importing file's own package. Per JLS §6.5.5 the import shadows the
//!   same-package name, so an extractor that resolves the bare name locally invents an edge. This is
//!   the false-positive bait, and it is the case the scoped scan exists for.
//! * **Every fifth file reaches a nested type through a qualifier** (`Outer.Inner`). That is a real
//!   same-package dependency on `Outer.java`, and the check it pays for is that a *type-shaped* head
//!   is followed while a package-shaped one is not: the corpus also writes `cache.Token` style heads
//!   whose lower-case left half is a package, and a scan that resolved those locally would report an
//!   edge to a file the source never named.

use std::collections::BTreeSet;

/// The type-name pool. Sixty names, deliberately full of the shapes that collide in real Java.
const NAMES: [&str; 60] = [
    "Builder",
    "Result",
    "Config",
    "Registry",
    "Store",
    "Runner",
    "Context",
    "Factory",
    "Provider",
    "Handler",
    "Loader",
    "Writer",
    "Reader",
    "Policy",
    "Session",
    "Client",
    "Server",
    "Token",
    "Session",
    "Account",
    "Address",
    "Amount",
    "Cursor",
    "Digest",
    "Edge",
    "Entry",
    "Frame",
    "Graph",
    "Index",
    "Job",
    "Ledger",
    "Matrix",
    "Node",
    "Order",
    "Parser",
    "Queue",
    "Router",
    "Scheduler",
    "Token",
    "Trace",
    "Ubiquitous",
    "Vertex",
    "Watcher",
    "Adapter",
    "Binding",
    "Cache",
    "Document",
    "Emitter",
    "Formatter",
    "Gateway",
    "Hook",
    "Invoker",
    "Joiner",
    "Kernel",
    "Layout",
    "Merger",
    "Normalizer",
    "Observer",
    "Publisher",
    "Sink",
];

const PACKAGE_WORDS: [&str; 40] = [
    "storefront",
    "ledger",
    "identity",
    "catalog",
    "shipping",
    "billing",
    "index",
    "reporting",
    "ingest",
    "export",
    "search",
    "graph",
    "policy",
    "runtime",
    "scheduler",
    "notify",
    "audit",
    "vault",
    "media",
    "tenant",
    "workflow",
    "quota",
    "channel",
    "device",
    "region",
    "pricing",
    "invoice",
    "cart",
    "checkout",
    "fulfil",
    "returns",
    "pricing2",
    "tax",
    "vendor",
    "warehouse",
    "worker",
    "queue",
    "stream",
    "archive",
    "cache",
];

/// The directory name of package `n`: `storefront0`, `ledger1`, … The word repeats; the number makes
/// it unique, which is the only thing a generated corpus has to guarantee.
fn package_dir(n: usize, packages: usize) -> String {
    format!(
        "{}{}",
        PACKAGE_WORDS[n % PACKAGE_WORDS.len()],
        n % packages.max(1)
    )
}

/// One generated corpus: the files, and the same-package dependencies they really contain.
pub struct Generated {
    /// `(corpus-relative path, bytes)`.
    pub files: Vec<(String, Vec<u8>)>,
    /// The planted same-package dependencies, spelled `file • target • symbol` — the same shape the
    /// scorer uses, so the comparison is a set difference and nothing more.
    pub planted: BTreeSet<String>,
    /// Packages in the corpus.
    pub packages: usize,
    /// How many planted references were *shadowed* by a same-name import in the same file: the bait.
    pub shadowed: usize,
    /// How many files bind a same-package type name as a method type variable — a mention with no
    /// dependency behind it, and the number of edges a scan that ignored bound names would invent.
    pub bound_baits: usize,
    /// How many distinct corpus files declare the same simple type name: the collision pressure.
    pub colliding_names: usize,
}

/// Author a corpus of at least `files` Java files (`packages × per_package` rounded up).
#[allow(clippy::vec_init_then_push)]
pub fn author(files: usize) -> Generated {
    let per_package = 30usize;
    let packages = files.div_ceil(per_package).max(2);
    let pool = NAMES.len();
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut planted: BTreeSet<String> = BTreeSet::new();
    let mut declarations: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut shadowed = 0usize;
    let mut bound_baits = 0usize;

    for package in 0..packages {
        let sub = format!(
            "{}{}",
            PACKAGE_WORDS[package % PACKAGE_WORDS.len()],
            package
        );
        let dir = format!("com/acme/{sub}");
        let package_name = format!("com.acme.{sub}");
        // A window of `per_package` names from the 60-name pool, offset per package so neighbouring
        // packages overlap by a predictable amount and every name is declared in several packages.
        let names: Vec<String> = (0..per_package)
            .map(|i| NAMES[(package * 7 + i * 3) % pool].to_string())
            .collect();
        // The window can repeat a name inside one package (illegal Java, and a real resolver would
        // not care); deduplicate so each package's declarations are a set, as Java demands.
        let names: Vec<String> = {
            let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
            let mut list: Vec<String> = Vec::new();
            for name in names {
                if seen.insert(name.clone()) {
                    list.push(name);
                }
            }
            // Top the package back up to `per_package` with names it does not have yet.
            let mut filler = 0usize;
            while list.len() < per_package {
                let candidate = NAMES[(package + filler * 11 + 1) % pool].to_string();
                filler += 1;
                if seen.insert(candidate.clone()) {
                    list.push(candidate);
                }
            }
            list
        };
        declarations.insert(package_name.clone(), names.clone());

        for (index, name) in names.iter().enumerate() {
            let path = format!("{dir}/{name}.java");
            // Two same-package types from *other* files: the planted dependencies.
            let first = names[(index + 1) % names.len()].clone();
            let second = names[(index + 3) % names.len()].clone();
            // A qualified reach into a nested type, every fifth file.
            let outer = (index % 5 == 0).then(|| names[(index + 2) % names.len()].clone());
            // An on-demand import plus a bare reference to a name this package also declares, every
            // eleventh file. A same-package type beats an on-demand import (JLS §6.5.5), so this is a
            // planted dependency even though a wildcard import is in sight: it is the test that the
            // shadow rule excludes *single-type* imports and nothing else. Whether the other package
            // declares the same name is beside the point — the same-package type wins either way.
            let on_demand = (index % 11 == 0).then(|| names[(index + 5) % names.len()].clone());
            // Every name this file will reference as its own, planted or otherwise: neither the
            // import name nor the bound type variable below may collide with one, or two plants land
            // on the same name and the extractor is charged for a dependency the source cannot mean.
            let own: BTreeSet<String> = [&first, &second]
                .into_iter()
                .cloned()
                .chain(outer.clone())
                .chain(on_demand.clone())
                .collect();

            // A *package*-qualified reference, every seventh file: the head is a package name, and
            // the rule that a lower-case head is not a type is what stops it becoming an edge.
            let across =
                (index % 7 == 0).then(|| format!("{{OTHER}}.{}", NAMES[(index * 3 + 1) % pool]));
            // The nastiest plant in the corpus, every thirteenth file: a method whose *type variable*
            // is spelled like another file of the same package (`<Amount>`), and which then uses that
            // name once. Per JLS §6.5.5 the type variable wins inside its own method, so the file
            // contains the word `Amount` and has no dependency on `Amount.java` — an edge-free
            // mention of a name this package declares, which is the one bait a scoped scan cannot
            // dodge by looking at imports. Every other package file that declares the name is a file
            // the naive scan could resolve to, so the bait is counted (`bound_bait_edges`) and the
            // rate is checkable rather than asserted.
            let bound = (index % 13 == 0)
                .then(|| names[(index + 13) % names.len()].clone())
                .filter(|candidate| !own.contains(candidate));
            // An import from a neighbouring package, taken — half the time — from the names *this*
            // package declares, which is the shadow case: a single-type import beats a same-package
            // type of the same spelling, so an extractor that resolves the bare name locally invents
            // an edge against a file the source never meant.
            let other_package = (package + 1 + (index % 3)) % packages;
            let other_dir = package_dir(other_package, packages);
            // The package the on-demand import comes from: a third one, so a file's two imports of
            // the same shape never come from the same place.
            let on_demand_dir = on_demand
                .as_ref()
                .map(|_| package_dir((package + 2 + index) % packages, packages));
            let across = across.map(|head| head.replace("{OTHER}", &other_dir));
            let mut other_name = if index % 2 == 0 {
                names[(index + 7) % names.len()].clone()
            } else {
                NAMES[(other_package * 5 + index) % pool].to_string()
            };
            // ...but never one of this file's planted dependencies. A file cannot both import `X` from
            // elsewhere and mean its own `X.java`; planting one would score a correct extractor as a
            // missed edge, and a corpus that punishes correctness is worse than a smaller one.
            let mut step = 0usize;
            while own.contains(&other_name) && step < pool {
                other_name = NAMES[(other_package * 5 + index + step + 1) % pool].to_string();
                step += 1;
            }
            let shadow = names.iter().any(|candidate| candidate == &other_name);
            if shadow {
                shadowed += 1;
            }

            let mut body = String::new();
            body.push_str(&format!("package {package_name};\n\n"));
            if let Some(on_demand_dir) = on_demand_dir {
                body.push_str(&format!("import com.acme.{on_demand_dir}.*;\n"));
            }
            body.push_str(&format!("import com.acme.{other_dir}.{other_name};\n\n"));
            body.push_str(&format!("public class {name} {{\n"));
            body.push_str(&format!(
                "    public static {name} of({first} a, {second} b) {{\n        return null;\n    }}\n"
            ));
            body.push_str(&format!(
                "    public int use({other_name} x) {{\n        return x.hashCode();\n    }}\n"
            ));
            body.push_str(&format!(
                "    public {first} again() {{\n        {first} v = new {first}();\n        return v;\n    }}\n"
            ));
            if let Some(outer) = &outer {
                body.push_str(&format!(
                    "    public int nested({outer}.Inner inner) {{\n        return 0;\n    }}\n"
                ));
            }
            if let Some(across) = &across {
                body.push_str(&format!(
                    "    public int across({across} value) {{\n        return 0;\n    }}\n"
                ));
            }
            if let Some(on_demand) = &on_demand {
                body.push_str(&format!(
                    "    public int onDemand({on_demand} value) {{\n        return value.hashCode();\n    }}\n"
                ));
            }
            // The bulk of a real file: fields and accessors, which reference no type the file has not
            // already named. Without them the generated files average 360 bytes, and a corpus of
            // postage stamps measures the `mib_per_second` canary's known blind spot (small files)
            // instead of the extractor; with them it averages closer to the 1-3 kB a Java file really
            // is, which is the shape the floor was set against.
            for field in 0..6 {
                body.push_str(&format!(
                    "    private int field{field};\n\n    /** The {field}th tracked value. */\n    public int field{field}() {{\n        return this.field{field};\n    }}\n\n    public void setField{field}(int value) {{\n        this.field{field} = value;\n    }}\n"
                ));
            }
            if let Some(bound) = &bound {
                body.push_str(&format!(
                    "    public <{bound}> int bound({bound} value) {{\n        return value.hashCode();\n    }}\n"
                ));
                bound_baits += 1;
            }
            body.push_str("}\n");
            planted.insert(format!(
                "{path} \u{2022} {dir}/{first}.java \u{2022} {first}"
            ));
            planted.insert(format!(
                "{path} \u{2022} {dir}/{second}.java \u{2022} {second}"
            ));
            if let Some(outer) = &outer {
                planted.insert(format!(
                    "{path} \u{2022} {dir}/{outer}.java \u{2022} {outer}"
                ));
            }
            if let Some(on_demand) = &on_demand {
                planted.insert(format!(
                    "{path} \u{2022} {dir}/{on_demand}.java \u{2022} {on_demand}"
                ));
            }
            // The import is a dependency too — on the *other* package's file, never this one's.
            let _ = other_package;
            out.push((path, body.into_bytes()));
        }
    }

    let mut per_name: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for names in declarations.values() {
        for name in names {
            *per_name.entry(name.clone()).or_default() += 1;
        }
    }
    let colliding_names = per_name.values().filter(|count| **count > 1).count();

    Generated {
        files: out,
        planted,
        packages,
        shadowed,
        bound_baits,
        colliding_names,
    }
}
