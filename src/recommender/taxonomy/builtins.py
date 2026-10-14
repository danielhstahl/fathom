"""Built-in taxonomy rules: the generated-file conventions every repository has.

This table is the reason `e2.1`, `e2.2`, `e2.6`, `e2.8` and `e3.2` may not carry glob
lists of their own. It is written as data — id, patterns, class, optional noise, one-line
note — because every row has to be printable by `fathom taxonomy rules`, quotable by
`fathom explain`, and diffable by the digest in `rules.table_digest`, and a dict literal
in a detector satisfies none of those.

Versioning
----------
`BUILTINS_VERSION` is the version of *this table*, and it is the same number as
`noise.builtins_version` in `fathom.toml` (the declared key for exactly this purpose). ADR-004
§6 lists "path taxonomy table change" as an invalidation event whose cascade is
"everything downstream of `taxonomy.path`", so adding a rule means bumping the version,
and a version bump is an honest, budgeted cost rather than a silent change to every
cached classification. `tests/test_taxonomy_contract.py` refuses a table change without a
version bump by pinning the digest of the table at the current version.

Ordering
--------
Within one rule id the generic pattern comes first and the specific one after it; between
families, order rarely decides anything because
:func:`recommender.taxonomy.rules.specificity` settles almost every contest. Where a
collision is genuinely possible (`node_modules/` vs `test/`), the more literal name wins,
which is the answer a reader predicts.

Deliberate omissions are decisions too, and they are written down rather than left to
chance:

* `.d.ts` is **not** generated-by-default. Half the committed `.d.ts` files in the wild
  are hand-written ambient types, and guessing wrong hides a hand-written file.
* `.obj` is **not** binary-by-default: it is a compiled object file in a C build and a 3D
  model in a game repository, and no path can tell them apart. It lands in `other`, per
  `e2.1`'s "ambiguous paths land in `other`" rule.
* `*.test.js`-style names are `test`, and so are Storybook `*.stories.*`: a story is a
  UI test surface, which is why the *generated* Storybook output is `storybook-static/`,
  not the stories.
* `schema.sql`/`*.sql` is `schema`, while an ordered *directory* is `migration`: the
  directory is the thing that gets executed once, which is what makes a migration the
  highest-consequence class (`e2.8`).
"""

from __future__ import annotations

from . import classes as C
from . import noise as N
from .rules import Rule

#: Version of this table. Bump it in the same change as any pattern addition or removal,
#: and expect `taxonomy.path`'s cache rows to be invalidated (ADR-004 §6).
BUILTINS_VERSION = 1

BUILTIN_SOURCE = "built-in taxonomy table v%d" % BUILTINS_VERSION


def _r(rule_id: str, patterns: list[str], cls: str | None = None, noise: str | None = None,
       note: str = "") -> Rule:
    return Rule(id=rule_id, patterns=tuple(patterns), cls=cls, noise=noise, note=note,
                source=BUILTIN_SOURCE)


#: The table, in family order. Every pattern is repository-relative and forward-slashed
#: (the same normalisation every matcher in this repository uses,
#: `recommender.config.globs.normalise`).
BUILTIN_RULES: list[Rule] = [
    # ---------------------------------------------------------------- vendored
    _r("dep.npm", ["**/node_modules/**"], C.PathClass.VENDORED,
       note="installed JavaScript dependencies; committed only in unusual vendoring setups"),
    _r("dep.vendor", ["**/vendor/**"], C.PathClass.VENDORED,
       note="`vendor/` means vendored in Go, PHP, Ruby and Python vendor-tree layouts alike"),
    _r("dep.third_party", ["**/third_party/**", "**/thirdparty/**", "**/extern/**"],
       C.PathClass.VENDORED),
    _r("dep.python_env", ["**/.venv/**", "**/venv/**", "**/site-packages/**",
                          "**/__pypackages__/**"], C.PathClass.VENDORED),
    _r("dep.bower", ["**/bower_components/**", "**/web_modules/**"], C.PathClass.VENDORED),
    _r("dep.cargo_registry", ["**/.cargo/registry/**"], C.PathClass.VENDORED),

    # ---------------------------------------------------------------- lockfiles
    _r("lock.node", ["**/package-lock.json", "**/npm-shrinkwrap.json", "**/yarn.lock",
                     "**/pnpm-lock.yaml", "**/bun.lockb", "**/.pnp.cjs",
                     "**/.pnp.data.json"], C.PathClass.LOCKFILE),
    _r("lock.rust", ["**/Cargo.lock"], C.PathClass.LOCKFILE),
    _r("lock.go", ["**/go.sum"], C.PathClass.LOCKFILE),
    _r("lock.python", ["**/poetry.lock", "**/Pipfile.lock", "**/uv.lock", "**/pdm.lock",
                       "**/requirements.lock", "**/conda-lock.yml"], C.PathClass.LOCKFILE),
    _r("lock.ruby", ["**/Gemfile.lock"], C.PathClass.LOCKFILE),
    _r("lock.php", ["**/composer.lock"], C.PathClass.LOCKFILE),
    _r("lock.jvm", ["**/gradle/verification-metadata.xml"], C.PathClass.LOCKFILE),
    _r("lock.dotnet", ["**/packages.lock.json"], C.PathClass.LOCKFILE),
    _r("lock.elixir", ["**/mix.lock"], C.PathClass.LOCKFILE),
    _r("lock.dart", ["**/pubspec.lock"], C.PathClass.LOCKFILE),
    _r("lock.nix", ["**/flake.lock"], C.PathClass.LOCKFILE),
    _r("lock.deno", ["**/deno.lock"], C.PathClass.LOCKFILE),
    _r("lock.buf", ["**/buf.lock"], C.PathClass.LOCKFILE),
    _r("lock.terraform", ["**/*.terraform.lock.hcl"], C.PathClass.LOCKFILE),

    # ---------------------------------------------------------------- generated
    # Directory conventions first: a named output directory is the strongest claim a
    # path can make about being derived, and it must beat a source extension inside it.
    _r("gen.dir", ["**/dist/**", "**/build/**", "**/target/**", "**/obj/**",
                   "**/.next/**", "**/.nuxt/**", "**/.output/**", "**/.docusaurus/**",
                   "**/storybook-static/**",
                   "**/generated/**", "**/generated-sources/**", "**/src-gen/**",
                   "**/__generated__/**"], C.PathClass.GENERATED,
       note="committed build output and codegen directories; `generated/**` is the "
            "convention every generator in the ticket's list writes into; `obj/**` is "
            ".NET's intermediate output and cannot be confused with a `.obj` file"),
    _r("gen.python", ["**/__pycache__/**", "**/*.pyc", "**/*.pyo", "**/*.egg-info/**",
                      "**/.mypy_cache/**", "**/.pytest_cache/**", "**/.ruff_cache/**",
                      "**/.tox/**"], C.PathClass.GENERATED),
    _r("gen.coverage", ["**/htmlcov/**", "**/coverage/lcov.info", "**/lcov.info",
                        "**/.coverage", "**/coverage.xml", "**/*.log"], C.PathClass.GENERATED,
       note="measured output, not source — the same argument `fathom.toml` makes for "
            "`benchmarks/reports/**`; a committed `.log` is a transcript of a run"),
    _r("gen.docs", ["**/_build/**", "**/apidocs/**", "**/javadoc/**", "**/docz/**"],
       C.PathClass.GENERATED),
    _r("gen.protobuf", ["**/*.pb.go", "**/*_pb.go", "**/*.pyi.pb.go", "**/*_pb2.py",
                        "**/*_pb2_grpc.py", "**/*.pb.cc", "**/*.pb.h", "**/*_pb.dart",
                        "**/*.pb.dart", "**/*_pb.rb", "**/*.pb.kt", "**/*_pb.kt",
                        "**/*.pb.swift", "**/*.pb.ts", "**/*.pb.mjs"], C.PathClass.GENERATED,
       note="protobuf/gRPC stubs in every language the compiler emits"),
    _r("gen.grpc", ["**/*.grpc.pb.go", "**/*_grpc.pb.go", "**/*.grpc.web.js",
                    "**/*.grpc-web.d.ts"], C.PathClass.GENERATED),
    _r("gen.flatbuffers", ["**/*.bfbs", "**/*.generated.h"], C.PathClass.GENERATED),
    _r("gen.thrift", ["**/gen-py/**", "**/gen-cpp/**", "**/gen-js/**", "**/gen-java/**",
                      "**/gen-go/**", "**/*-remote.py", "**/*-remote.js"],
       C.PathClass.GENERATED),
    _r("gen.rust", ["**/*_generated.rs", "**/*.generated.rs", "**/*.rs.in"],
       C.PathClass.GENERATED,
       note="`*_generated.rs` is the bindgen/build.rs convention named in the ticket"),
    _r("gen.dart", ["**/*.g.dart", "**/*.gr.dart", "**/*.freezed.dart", "**/*.mocks.dart"],
       C.PathClass.GENERATED),
    _r("gen.openapi", ["**/*.gen.ts", "**/*.gen.tsx", "**/*.gen.js", "**/*.openapi.ts",
                       "**/openapi-codegen/**"], C.PathClass.GENERATED,
       note="openapi-typescript / orval / openapi-generator output; the *specification* "
            "is `schema.openapi`, which is a different claim"),
    _r("gen.dotnet", ["**/*.designer.cs", "**/*.generated.cs", "**/*.g.cs"],
       C.PathClass.GENERATED),
    _r("gen.sorbet", ["**/*.rbi", "**/sorbet/rbi/**"], C.PathClass.GENERATED),
    _r("gen.rails", ["**/db/schema.rb", "**/db/structure.sql"], C.PathClass.GENERATED,
       note="a *dump* the ORM rewrites, not a hand-written migration"),
    _r("gen.snapshot", ["**/*.snap", "**/__snapshots__/**", "**/*.snapshot"],
       C.PathClass.GENERATED,
       note="jest/vitest/insta snapshots: committed on purpose, reviewed almost never"),
    _r("gen.bundle", ["**/*.min.js", "**/*.min.css", "**/*.min.js.map", "**/*.bundle.js",
                      "**/*.js.map", "**/*.css.map", "**/*.map"], C.PathClass.GENERATED,
       note="minified and source-mapped bundler output"),
    _r("gen.protobuf_spec_dump", ["**/*.proto.txt"], C.PathClass.GENERATED),

    # ---------------------------------------------------------------- source
    # Generic extension rules: lowest specificity on purpose, so anything more specific
    # in this table — or one line in a `fathom.toml` — outranks them.
    _r("src.python", ["**/*.py", "**/*.pyi", "**/*.ipynb"], C.PathClass.SOURCE),
    _r("src.js", ["**/*.js", "**/*.jsx", "**/*.mjs", "**/*.cjs"], C.PathClass.SOURCE),
    _r("src.ts", ["**/*.ts", "**/*.tsx"], C.PathClass.SOURCE),
    _r("src.rust", ["**/*.rs"], C.PathClass.SOURCE),
    _r("src.go", ["**/*.go"], C.PathClass.SOURCE),
    _r("src.jvm", ["**/*.java", "**/*.kt", "**/*.kts", "**/*.scala", "**/*.groovy",
                   "**/*.clj", "**/*.cljs"], C.PathClass.SOURCE),
    _r("src.dotnet", ["**/*.cs", "**/*.fs", "**/*.vb"], C.PathClass.SOURCE),
    _r("src.native", ["**/*.c", "**/*.h", "**/*.cc", "**/*.cpp", "**/*.cxx", "**/*.hpp",
                      "**/*.hh", "**/*.hxx", "**/*.m", "**/*.mm"], C.PathClass.SOURCE),
    _r("src.swift", ["**/*.swift"], C.PathClass.SOURCE),
    _r("src.web", ["**/*.vue", "**/*.svelte", "**/*.astro"], C.PathClass.SOURCE),
    _r("src.other", ["**/*.rb", "**/*.php", "**/*.dart", "**/*.ex", "**/*.exs", "**/*.erl",
                     "**/*.hrl", "**/*.lua", "**/*.pl", "**/*.pm", "**/*.r", "**/*.jl",
                     "**/*.zig", "**/*.nim", "**/*.cr", "**/*.d", "**/*.hs", "**/*.ml",
                     "**/*.mli", "**/*.sol", "**/*.v", "**/*.odin"], C.PathClass.SOURCE),
    _r("src.shell", ["**/*.sh", "**/*.bash", "**/*.zsh", "**/*.fish", "**/*.ps1"],
       C.PathClass.SOURCE),
    _r("src.styles", ["**/*.css", "**/*.scss", "**/*.sass", "**/*.less", "**/*.styl"],
       C.PathClass.SOURCE,
       note="hand-written styling is code; the bundler's `*.min.css` outranks it"),
    _r("src.markup", ["**/*.html", "**/*.htm"], C.PathClass.SOURCE),
    _r("src.templates", ["**/*.erb", "**/*.haml", "**/*.slim", "**/*.ejs", "**/*.pug",
                         "**/*.mustache", "**/*.tmpl", "**/*.gohtml", "**/*.j2",
                         "**/*.twig", "**/*.liquid", "**/*.hbs"], C.PathClass.SOURCE,
       note="view templates carry logic a reviewer reads as code"),
    _r("src.grammar", ["**/*.y", "**/*.yy", "**/*.l", "**/*.ll", "**/*.g4", "**/*.g",
                       "**/*.acc"], C.PathClass.SOURCE,
       note="yacc/bison/flex/ANTLR grammars: the hand-written input, not the generated parser"),
    _r("src.shaders", ["**/*.glsl", "**/*.hlsl", "**/*.vert", "**/*.frag", "**/*.comp",
                       "**/*.metal", "**/*.wgsl", "**/*.shader"], C.PathClass.SOURCE),

    # ---------------------------------------------------------------- test
    _r("test.dir", ["tests/**", "**/tests/**", "**/test/**", "**/spec/**",
                    "**/__tests__/**", "**/e2e/**", "**/cypress/**", "**/playwright/**",
                    "**/benches/**", "**/fixtures/**"], C.PathClass.TEST,
       note="`fixtures/` counts as test surface: a fixture is reviewed with the test that "
            "loads it"),
    _r("test.python", ["**/test_*.py", "**/*_test.py", "**/conftest.py"], C.PathClass.TEST),
    _r("test.go", ["**/*_test.go"], C.PathClass.TEST),
    _r("test.js", ["**/*.test.js", "**/*.test.jsx", "**/*.test.ts", "**/*.test.tsx",
                   "**/*.test.mjs", "**/*.spec.js", "**/*.spec.jsx", "**/*.spec.ts",
                   "**/*.spec.tsx", "**/*.spec.mjs"], C.PathClass.TEST),
    _r("test.jvm", ["**/*Test.java", "**/*Tests.java", "**/*IT.java", "**/*Spec.groovy",
                    "**/*Test.kt"], C.PathClass.TEST),
    _r("test.ruby", ["**/*_spec.rb", "**/*_test.rb"], C.PathClass.TEST),
    _r("test.bdd", ["**/*.feature"], C.PathClass.TEST),
    _r("test.storybook", ["**/*.stories.tsx", "**/*.stories.ts", "**/*.stories.jsx",
                          "**/*.stories.js", "**/*.stories.mdx", "**/*.story.tsx",
                          "**/.storybook/**"], C.PathClass.TEST,
       note="a story is a UI test surface; the *built* storybook is `gen.dir`"),

    # ---------------------------------------------------------------- docs
    _r("docs.markup", ["**/*.md", "**/*.mdx", "**/*.rst", "**/*.adoc", "**/*.txt"],
       C.PathClass.DOCS),
    _r("docs.dir", ["docs/**", "**/docs/**", "**/doc/**", "**/guides/**", "**/rfcs/**",
                    "**/adr/**", "**/notes/**"], C.PathClass.DOCS),
    _r("docs.named", ["**/README", "**/CHANGELOG", "**/LICENSE", "**/COPYING", "**/NOTICE",
                      "**/SECURITY", "**/CODE_OF_CONDUCT"], C.PathClass.DOCS),
    _r("docs.changelog", ["**/CHANGELOG.md", "**/CHANGES.md", "**/HISTORY.md",
                          "**/RELEASES.md"], C.PathClass.DOCS),

    # ---------------------------------------------------------------- config
    _r("cfg.data", ["**/*.json", "**/*.yaml", "**/*.yml", "**/*.toml", "**/*.ini",
                    "**/*.cfg", "**/*.conf", "**/*.properties", "**/*.xml"],
       C.PathClass.CONFIG,
       note="the floor for structured files: every more specific rule below outranks it"),
    _r("cfg.dotfiles", ["**/.gitignore", "**/.gitattributes", "**/.dockerignore",
                        "**/.editorconfig", "**/.env", "**/.env.*", "**/.npmrc",
                        "**/.pypirc", "**/.tool-versions", "**/.yamllint", "**/.shellcheckrc",
                        "**/.stylelintrc", "**/.markdownlint*", "**/.bazelrc",
                        "**/.bazelversion", "**/.browserslistrc"], C.PathClass.CONFIG),
    _r("cfg.ci", [".github/**", "**/.github/**", "**/.gitlab-ci.yml", "**/.circleci/**",
                  "**/.buildkite/**", "**/.woodpecker/**", "**/azure-pipelines.yml",
                  "**/.travis.yml", "**/Jenkinsfile", "**/.renovaterc", "**/renovate.json"],
       C.PathClass.CONFIG,
       note="CI is configuration a reviewer has to read, and `non_code.patterns` (`e2.8`) "
            "additionally marks it as an operator surface"),
    _r("cfg.build", ["**/Makefile", "**/makefile", "**/GNUmakefile", "**/Makefile.in",
                     "**/Makefile.am", "**/configure.ac", "**/configure", "**/*.m4",
                     "**/CMakeLists.txt",
                     "**/*.mk", "**/meson.build", "**/BUILD", "**/BUILD.bazel",
                     "**/WORKSPACE", "**/*.bzl", "**/pom.xml", "**/build.gradle",
                     "**/build.gradle.kts", "**/settings.gradle", "**/settings.gradle.kts",
                     "**/gradle.properties", "**/build.sbt", "**/setup.py", "**/setup.cfg",
                     "**/pyproject.toml", "**/Cargo.toml", "**/go.mod", "**/go.work",
                     "**/Package.swift", "**/composer.json", "**/Gemfile", "**/*.gemspec",
                     "**/mix.exs", "**/pubspec.yaml"],
       C.PathClass.CONFIG),
    _r("cfg.dir", ["config/**", "configs/**"], C.PathClass.CONFIG,
       note="the *root* `config/` directory, which is where Rails, Django, Symfony, Spring "
            "and Elixir/Phoenix keep configuration; a `src/**/config/` module is source "
            "like any other module, so the pattern is deliberately root-anchored"),
    _r("cfg.dotnet", ["**/*.csproj", "**/*.fsproj", "**/*.sln", "**/NuGet.config"],
       C.PathClass.CONFIG),
    _r("cfg.node", ["**/package.json", "**/package.json5", "**/tsconfig.json",
                    "**/tsconfig.*.json", "**/jsconfig.json", "**/webpack.config.js",
                    "**/vite.config.ts", "**/rollup.config.js", "**/babel.config.js",
                    "**/jest.config.js", "**/vitest.config.ts", "**/next.config.js",
                    "**/eslint.config.js", "**/.eslintrc.json", "**/.prettierrc",
                    "**/tailwind.config.js", "**/postcss.config.js", "**/nx.json",
                    "**/turbo.json", "**/*.config.js", "**/*.config.ts", "**/*.config.mjs"],
       C.PathClass.CONFIG),
    _r("cfg.python", ["**/requirements.txt", "**/requirements-dev.txt", "**/tox.ini",
                      "**/pytest.ini", "**/mypy.ini", "**/ruff.toml", "**/.flake8",
                      "**/Pipfile", "**/poetry.toml", "**/alembic.ini"], C.PathClass.CONFIG),
    _r("cfg.container", ["**/Dockerfile*", "**/Containerfile*", "**/docker-compose.yml",
                         "**/docker-compose.yaml", "**/compose.yml", "**/Procfile",
                         "**/*.service", "**/supervisord.conf"], C.PathClass.CONFIG),
    _r("cfg.iac", ["**/*.tf", "**/*.tfvars", "**/*.hcl", "**/kustomization.yaml",
                   "**/Chart.yaml", "**/values.yaml", "**/helmfile.yaml"],
       C.PathClass.CONFIG),
    _r("cfg.buf", ["**/buf.yaml", "**/buf.gen.yaml"], C.PathClass.CONFIG),
    _r("cfg.fixtures_lock", ["**/golangci.yml", "**/.golangci.yml"], C.PathClass.CONFIG),

    # ---------------------------------------------------------------- schema
    _r("schema.protobuf", ["**/*.proto"], C.PathClass.SCHEMA),
    _r("schema.graphql", ["**/*.graphql", "**/*.gql", "**/*.graphqls"], C.PathClass.SCHEMA),
    _r("schema.avro", ["**/*.avsc"], C.PathClass.SCHEMA),
    _r("schema.thrift", ["**/*.thrift"], C.PathClass.SCHEMA),
    _r("schema.capnp", ["**/*.capnp"], C.PathClass.SCHEMA),
    _r("schema.flatbuffers", ["**/*.fbs"], C.PathClass.SCHEMA),
    _r("schema.openapi", ["**/openapi.json", "**/openapi.yaml", "**/openapi.yml",
                          "**/swagger.json", "**/*.openapi.yaml", "**/*.openapi.json",
                          "**/asyncapi.yaml", "**/asyncapi.json"], C.PathClass.SCHEMA,
       note="the specification, which is a contract; the client it generates is "
            "`gen.openapi`"),
    _r("schema.json", ["**/*.schema.json", "**/*.jsonschema"], C.PathClass.SCHEMA),
    _r("schema.prisma", ["**/schema.prisma"], C.PathClass.SCHEMA),
    _r("schema.sql", ["**/*.sql"], C.PathClass.SCHEMA,
       note="DDL and query files as a reviewed contract; an ordered directory of them is "
            "`mig.dir`, which is the thing that gets executed once"),
    _r("schema.xsd", ["**/*.xsd", "**/*.rng"], C.PathClass.SCHEMA),

    # ---------------------------------------------------------------- migration
    _r("mig.dir", ["**/migrations/**", "**/migration/**", "**/migrate/**",
                   "**/db/migrate/**", "**/db/migration/**", "**/db/migrations/**",
                   "**/alembic/versions/**", "**/flyway/**", "**/liquibase/**"],
       C.PathClass.MIGRATION,
       note="ordered, executed-once changes to durable state — the class `e2.8` turns "
            "into a deploy-coordination finding"),
    _r("mig.reversible", ["**/*.up.sql", "**/*.down.sql"], C.PathClass.MIGRATION,
       note="forward/back pairs: a `.up.sql` with no `.down.sql` is forward-only"),

    # ---------------------------------------------------------------- asset
    _r("asset.image", ["**/*.png", "**/*.jpg", "**/*.jpeg", "**/*.gif", "**/*.webp",
                       "**/*.bmp", "**/*.ico", "**/*.tiff", "**/*.avif", "**/*.heic",
                       "**/*.svg"], C.PathClass.ASSET,
       note="`.svg` is text-diffable, so the class says `asset` and `is_diffable` says "
            "the diff viewer may render it"),
    _r("asset.font", ["**/*.woff", "**/*.woff2", "**/*.ttf", "**/*.otf", "**/*.eot"],
       C.PathClass.ASSET),
    _r("asset.media", ["**/*.mp3", "**/*.wav", "**/*.ogg", "**/*.flac", "**/*.aac",
                       "**/*.mp4", "**/*.webm", "**/*.mov", "**/*.avi", "**/*.mkv",
                       "**/*.m4a"], C.PathClass.ASSET),
    _r("asset.design", ["**/*.psd", "**/*.sketch", "**/*.fig", "**/*.xd", "**/*.ai",
                        "**/*.eps", "**/*.xcf"], C.PathClass.ASSET),
    _r("asset.model", ["**/*.glb", "**/*.gltf", "**/*.fbx", "**/*.usdz", "**/*.stl"],
       C.PathClass.ASSET),
    _r("asset.data", ["**/*.csv", "**/*.tsv", "**/*.parquet", "**/*.avro", "**/*.xlsx",
                      "**/*.xls", "**/*.jsonl", "**/*.ndjson"], C.PathClass.ASSET,
       note="data files: reviewed as data, counted, never diffed as code"),
    _r("asset.document", ["**/*.pdf", "**/*.epub", "**/*.docx", "**/*.pptx"],
       C.PathClass.ASSET),

    # ---------------------------------------------------------------- binary
    _r("bin.compiled", ["**/*.so", "**/*.dll", "**/*.dylib", "**/*.exe", "**/*.class",
                        "**/*.jar", "**/*.war", "**/*.ear", "**/*.aar", "**/*.pyd",
                        "**/*.node", "**/*.wasm"], C.PathClass.BINARY,
       note="`.obj` is missing on purpose: object file or 3D model is not knowable from a "
            "path, and `e2.1` sends ambiguous to `other` rather than guessing"),
    _r("bin.archive", ["**/*.zip", "**/*.gz", "**/*.tgz", "**/*.bz2", "**/*.xz",
                       "**/*.zst", "**/*.7z", "**/*.rar", "**/*.deb", "**/*.rpm",
                       "**/*.dmg", "**/*.iso", "**/*.apk", "**/*.ipa"], C.PathClass.BINARY),
    _r("bin.database", ["**/*.db", "**/*.sqlite", "**/*.sqlite3"], C.PathClass.BINARY),
    _r("bin.ml", ["**/*.pt", "**/*.pth", "**/*.ckpt", "**/*.onnx", "**/*.h5",
                  "**/*.safetensors", "**/*.tflite", "**/*.joblib", "**/*.pickle"],
       C.PathClass.BINARY,
       note="committed model weights: a change here is a release event, and there is no "
            "text diff for it"),
]


def builtin_rules() -> list[Rule]:
    """A fresh copy of the table (rules are frozen, so a copy is cheap and safe to sort)."""
    return list(BUILTIN_RULES)


#: Every noise level a built-in rule may claim, asserted by the contract test: a table row
#: may name a category, and `reformat` is not one a path can know.
BUILTIN_NOISE_CLAIMS = {rule.noise for rule in BUILTIN_RULES if rule.noise} or {
    N.Noise.INFORMATIVE, N.Noise.GENERATED, N.Noise.LOCKFILE, N.Noise.VENDORED}
