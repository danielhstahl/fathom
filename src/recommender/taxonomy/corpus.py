"""The decision table: real paths from well-known repository shapes, with their answers.

`e0.9`'s first acceptance criterion is *"a decision table test covering at least 150
real-world paths across 20 well-known repo shapes\"*. The table lives in the package rather
than in the test file for two reasons: `fathom taxonomy bench` measures the lookup cost on
exactly these paths (so the perf claim is measured on the same data the correctness claim
uses), and `e2.1`'s accuracy report is required to cite a labelled fixture set, which is
this one.

Each row is `(path, expected_class, expected_noise)`, written against the **built-in**
table only — no `fathom.toml`, no user globs — so a failure says "the built-ins are wrong",
never " somebody's local config drifted". User-layer behaviour is tested separately in
`tests/test_taxonomy_contract.py`, because promoting and demoting is a different claim from
classifying.

Reading a shape's `note` matters: several rows are deliberate *documented gaps* — a path
where the honest answer from a path alone is `other` (`.obj`: object file or 3D model?), or
where a convention is intentionally not claimed (`out/`, `mvnw`, `.pbxproj`). A corpus that
contains only the easy rows is a corpus that cannot fail.
"""

from __future__ import annotations

#: shape slug -> {"note": why this shape is here, "paths": [(path, class, noise), …]}
CORPUS: dict[str, dict] = {
    "react-webpack-jest": {
        "note": "the classic JS app: webpack output, jest snapshots, storybook, CI.",
        "paths": [
            ("src/App.js", "source", "informative"),
            ("src/components/Button.jsx", "source", "informative"),
            ("src/components/Button.test.jsx", "test", "informative"),
            ("src/components/Button.stories.tsx", "test", "informative"),
            ("src/__snapshots__/App.test.jsx.snap", "generated", "generated"),
            ("dist/main.a41f2b.min.js", "generated", "generated"),
            ("node_modules/react/index.js", "vendored", "vendored"),
            ("package-lock.json", "lockfile", "lockfile"),
            ("package.json", "config", "informative"),
            ("webpack.config.js", "config", "informative"),
            ("public/logo.svg", "asset", "informative"),
            (".github/workflows/ci.yml", "config", "informative"),
            ("README.md", "docs", "informative"),
            ("build/index.html", "generated", "generated"),
        ],
    },
    "nextjs-app-router": {
        "note": "modern TS app framework: framework build dir, env files, fonts.",
        "paths": [
            ("app/page.tsx", "source", "informative"),
            ("app/layout.tsx", "source", "informative"),
            ("next.config.js", "config", "informative"),
            (".next/build-manifest.json", "generated", "generated"),
            ("public/fonts/Inter-Regular.woff2", "asset", "informative"),
            ("tsconfig.json", "config", "informative"),
            ("types/api.d.ts", "source", "informative"),
            (".env.local", "config", "informative"),
            ("README.md", "docs", "informative"),
        ],
    },
    "rust-crate": {
        "note": "a Rust binary crate: target output, build.rs, benches, codegen module.",
        "paths": [
            ("src/main.rs", "source", "informative"),
            ("src/cli.rs", "source", "informative"),
            ("build.rs", "source", "informative"),
            ("tests/cli_test.rs", "test", "informative"),
            ("benches/parse.rs", "test", "informative"),
            ("target/debug/fathom", "generated", "generated"),
            ("src/generated/api.rs", "generated", "generated"),
            ("src/api/_generated.rs", "generated", "generated"),
            ("Cargo.lock", "lockfile", "lockfile"),
            ("Cargo.toml", "config", "informative"),
            (".cargo/config.toml", "config", "informative"),
            ("out/codegen.rs", "source", "informative"),
            ("LICENSE", "docs", "informative"),
        ],
    },
    "django-app": {
        "note": "Django: migrations as code, sqlite in-tree, pytest cache, minified static.",
        "paths": [
            ("manage.py", "source", "informative"),
            ("app/views.py", "source", "informative"),
            ("app/asgi.py", "source", "informative"),
            ("app/tests/test_views.py", "test", "informative"),
            ("app/migrations/0004_add_index.py", "migration", "informative"),
            ("db.sqlite3", "binary", "informative"),
            ("requirements.txt", "config", "informative"),
            ("pyproject.toml", "config", "informative"),
            ("static/css/app.min.css", "generated", "generated"),
            ("templates/app/index.html", "source", "informative"),
            (".pytest_cache/v/cache/lastfailed", "generated", "generated"),
            ("docs/settings.rst", "docs", "informative"),
        ],
    },
    "python-ml": {
        "note": "ML/DS project: notebooks, weights, parquet, lockfile, generated plots.",
        "paths": [
            ("src/model/train.py", "source", "informative"),
            ("notebooks/explore.ipynb", "source", "informative"),
            ("notebooks/outputs/plot.png", "asset", "informative"),
            ("data/train.csv", "asset", "informative"),
            ("data/train.parquet", "asset", "informative"),
            ("models/epoch_40.pt", "binary", "informative"),
            ("models/export/onnx/model.onnx", "binary", "informative"),
            ("uv.lock", "lockfile", "lockfile"),
            ("tests/test_train.py", "test", "informative"),
            ("reports/metrics.json", "config", "informative"),
            ("src/model/__init__.py", "source", "informative"),
        ],
    },
    "go-microservice": {
        "note": "small Go service: go.sum, Dockerfile, vendored dependency.",
        "paths": [
            ("cmd/server/main.go", "source", "informative"),
            ("internal/auth/token.go", "source", "informative"),
            ("internal/auth/token_test.go", "test", "informative"),
            ("api/proto/service.proto", "schema", "informative"),
            ("api/proto/service.pb.go", "generated", "generated"),
            ("go.mod", "config", "informative"),
            ("go.sum", "lockfile", "lockfile"),
            ("vendor/github.com/x/y/y.go", "vendored", "vendored"),
            ("Dockerfile", "config", "informative"),
            (".golangci.yml", "config", "informative"),
            ("web/templates/index.tmpl", "source", "informative"),
            ("Makefile", "config", "informative"),
        ],
    },
    "kubernetes-like-monorepo": {
        "note": "the big-Go shape: third_party, vendor, generated openapi, codegen scripts.",
        "paths": [
            ("staging/src/k8s.io/api/core/v1/types.go", "source", "informative"),
            ("third_party/protobuf/github.com/x/x.pb.go", "vendored", "vendored"),
            ("vendor/k8s.io/apimachinery/pkg/apis/meta/v1/time.go", "vendored", "vendored"),
            ("pkg/controller/controller_test.go", "test", "informative"),
            ("generated/openapi_api_groups.json", "generated", "generated"),
            ("hack/update-codegen.sh", "source", "informative"),
            ("LICENSE", "docs", "informative"),
            ("SECURITY_CONTACTS", "other", "informative"),
            ("Makefile", "config", "informative"),
        ],
    },
    "rails-app": {
        "note": "Rails: root config/, db/migrate, the dumped schema.rb, minified packs.",
        "paths": [
            ("app/models/user.rb", "source", "informative"),
            ("test/models/user_test.rb", "test", "informative"),
            ("spec/models/user_spec.rb", "test", "informative"),
            ("db/migrate/20240101000000_add_users.rb", "migration", "informative"),
            ("db/schema.rb", "generated", "generated"),
            ("Gemfile", "config", "informative"),
            ("Gemfile.lock", "lockfile", "lockfile"),
            ("app/views/users/index.html.erb", "source", "informative"),
            ("config/routes.rb", "config", "informative"),
            ("config/database.yml", "config", "informative"),
            ("log/development.log", "generated", "generated"),
            ("public/404.html", "source", "informative"),
        ],
    },
    "spring-java": {
        "note": "JVM service: Maven, Flyway under resources, target/classes, wrapper scripts.",
        "paths": [
            ("src/main/java/com/x/App.java", "source", "informative"),
            ("src/test/java/com/x/AppTest.java", "test", "informative"),
            ("pom.xml", "config", "informative"),
            ("src/main/resources/application.yml", "config", "informative"),
            ("src/main/resources/db/migration/V1__init.sql", "migration", "informative"),
            ("target/classes/App.class", "generated", "generated"),
            (".mvn/wrapper/maven-wrapper.properties", "config", "informative"),
            ("build.gradle", "config", "informative"),
            ("src/main/resources/static/index.html", "source", "informative"),
            ("mvnw", "other", "informative"),
        ],
    },
    "dotnet-service": {
        "note": ".NET: csproj/sln, obj/ intermediate output, appsettings.",
        "paths": [
            ("src/Api/Program.cs", "source", "informative"),
            ("src/Api/Controllers/ValuesController.cs", "source", "informative"),
            ("tests/Api.Tests/UnitTests.cs", "test", "informative"),
            ("Api.csproj", "config", "informative"),
            ("obj/Debug/net8.0/AssemblyInfo.cs", "generated", "generated"),
            ("appsettings.json", "config", "informative"),
            ("NuGet.config", "config", "informative"),
        ],
    },
    "flutter-app": {
        "note": "Dart/Flutter: build output incl. an .apk, codegen .g.dart, assets.",
        "paths": [
            ("lib/main.dart", "source", "informative"),
            ("test/widget_test.dart", "test", "informative"),
            ("lib/models/user.g.dart", "generated", "generated"),
            ("build/app/outputs/flutter-apk/app.apk", "generated", "generated"),
            ("assets/icons/logo.png", "asset", "informative"),
            ("pubspec.yaml", "config", "informative"),
            ("pubspec.lock", "lockfile", "lockfile"),
            ("ios/Runner.xcodeproj/project.pbxproj", "other", "informative"),
        ],
    },
    "vue-vite-app": {
        "note": "Vue + Vite + Cypress: framework output, spec files, styles.",
        "paths": [
            ("src/main.ts", "source", "informative"),
            ("src/components/HelloWorld.vue", "source", "informative"),
            ("src/components/HelloWorld.spec.ts", "test", "informative"),
            ("src/styles/main.scss", "source", "informative"),
            ("src/assets/logo.png", "asset", "informative"),
            ("vite.config.ts", "config", "informative"),
            ("dist/assets/index-3f9a.js", "generated", "generated"),
            ("cypress/e2e/home.cy.ts", "test", "informative"),
            (".eslintrc.json", "config", "informative"),
        ],
    },
    "c-project-autotools": {
        "note": "C with autotools: grammars are source, generated parsers are not.",
        "paths": [
            ("src/backend/parser/parse.c", "source", "informative"),
            ("src/include/parser/gram.h", "source", "informative"),
            ("src/backend/parser/gram.y", "source", "informative"),
            ("src/backend/parser/scan.l", "source", "informative"),
            ("configure.ac", "config", "informative"),
            ("Makefile.am", "config", "informative"),
            ("contrib/pg_stat/pg_stat.c", "source", "informative"),
            ("doc/postgres.sgml", "docs", "informative"),
            (".gitignore", "config", "informative"),
        ],
    },
    "cpp-bazel-envoy-like": {
        "note": "C++/Bazel with a large protobuf API surface.",
        "paths": [
            ("source/common/http/conn_manager_impl.cc", "source", "informative"),
            ("test/common/http/conn_manager_impl_test.cc", "test", "informative"),
            ("api/envoy/api/v2/route.proto", "schema", "informative"),
            ("api/envoy/api/v2/route.pb.cc", "generated", "generated"),
            ("BUILD.bazel", "config", "informative"),
            ("source/common/http/BUILD", "config", "informative"),
            ("WORKSPACE", "config", "informative"),
            (".bazelrc", "config", "informative"),
        ],
    },
    "docs-site": {
        "note": "Docs repository: Sphinx output, and the dir-beats-extension consequence.",
        "paths": [
            ("docs/index.rst", "docs", "informative"),
            ("docs/conf.py", "docs", "informative"),
            ("docs/api.md", "docs", "informative"),
            ("docs/assets/diagram.svg", "docs", "informative"),
            ("_build/html/index.html", "generated", "generated"),
            ("mkdocs.yml", "config", "informative"),
            ("blog/2024-01-01-hello/index.mdx", "docs", "informative"),
            ("static/img/logo.png", "asset", "informative"),
            ("package.json", "config", "informative"),
            ("docusaurus.config.js", "config", "informative"),
        ],
    },
    "infra-terraform": {
        "note": "IaC: modules, tfvars, helm charts, and Terraform's lock file.",
        "paths": [
            ("modules/vpc/main.tf", "config", "informative"),
            ("envs/prod/main.tfvars", "config", "informative"),
            ("charts/app/Chart.yaml", "config", "informative"),
            ("k8s/deployment.yaml", "config", "informative"),
            ("scripts/rotate.sh", "source", "informative"),
            (".terraform.lock.hcl", "lockfile", "lockfile"),
            ("policy/main.rego", "other", "informative"),
            ("README.md", "docs", "informative"),
        ],
    },
    "data-pipeline": {
        "note": "Airflow + dbt: migrations, DDL, data files, generated reports.",
        "paths": [
            ("dags/etl/loader.py", "source", "informative"),
            ("dags/utils/deps.py", "source", "informative"),
            ("dbt/models/staging/stg_orders.sql", "schema", "informative"),
            ("dbt/models/staging/schema.yml", "config", "informative"),
            ("airflow/migrations/0012_add_run_state.py", "migration", "informative"),
            ("data/raw/events.csv", "asset", "informative"),
            ("models/mart/orders.parquet", "asset", "informative"),
            ("reports/daily.json", "config", "informative"),
            ("docs/dag_catalog.md", "docs", "informative"),
            (".env", "config", "informative"),
            ("Dockerfile", "config", "informative"),
        ],
    },
    "graphql-monorepo": {
        "note": "GraphQL codegen shape: schema, generated clients, snapshots, lockfile.",
        "paths": [
            ("packages/api/schema.graphql", "schema", "informative"),
            ("packages/api/src/resolvers.ts", "source", "informative"),
            ("packages/client/src/__generated__/types.ts", "generated", "generated"),
            ("packages/api/src/types.gen.ts", "generated", "generated"),
            ("packages/api/src/__tests__/resolvers.spec.ts", "test", "informative"),
            ("packages/api/src/__snapshots__/resolvers.snap", "generated", "generated"),
            ("codegen.ts", "source", "informative"),
            ("pnpm-lock.yaml", "lockfile", "lockfile"),
            ("schema.graphql", "schema", "informative"),
        ],
    },
    "elixir-phoenix": {
        "note": "Elixir/Phoenix: root config/, priv/repo/migrations, mix.lock.",
        "paths": [
            ("lib/app/repo.ex", "source", "informative"),
            ("lib/app_web/router.ex", "source", "informative"),
            ("test/app_web/controllers/page_controller_test.exs", "test", "informative"),
            ("priv/repo/migrations/20240101000000_add_users.exs", "migration", "informative"),
            ("assets/static/js/app.js", "source", "informative"),
            ("assets/static/css/app.css", "source", "informative"),
            ("config/config.exs", "config", "informative"),
            ("mix.exs", "config", "informative"),
            ("mix.lock", "lockfile", "lockfile"),
        ],
    },
    "fathom-this-repository": {
        "note": "this repository, classified by the built-in table alone.",
        "paths": [
            ("src/recommender/config/schema.py", "source", "informative"),
            ("src/recommender/taxonomy/engine.py", "source", "informative"),
            ("tests/test_config_contract.py", "test", "informative"),
            ("docs/concepts/thesis.md", "docs", "informative"),
            ("docs/adr/0004-storage.md", "docs", "informative"),
            ("schemas/fathom.schema.json", "schema", "informative"),
            (".github/workflows/contracts.yml", "config", "informative"),
            (".plan/plan.json", "config", "informative"),
            ("fathom.toml", "config", "informative"),
            ("deny.toml", "config", "informative"),
            ("pytest.ini", "config", "informative"),
            ("benchmarks/budgets.json", "config", "informative"),
            ("tools/check_workspace.py", "source", "informative"),
            ("README.md", "docs", "informative"),
            (".pytest_cache/v/cache/nodeids", "generated", "generated"),
        ],
    },
    "node-lib-storybook": {
        "note": "a published component library: storybook config, stories, built storybook.",
        "paths": [
            (".storybook/main.ts", "test", "informative"),
            (".storybook/preview.ts", "test", "informative"),
            ("src/Button.tsx", "source", "informative"),
            ("src/Button.stories.tsx", "test", "informative"),
            ("src/Button.test.tsx", "test", "informative"),
            ("storybook-static/index.html", "generated", "generated"),
            ("dist/bundle.js", "generated", "generated"),
            ("package.json", "config", "informative"),
        ],
    },
    "php-composer-app": {
        "note": "PHP app: composer.lock, vendor/, a front controller in public/.",
        "paths": [
            ("src/Domain/User.php", "source", "informative"),
            ("tests/Domain/UserTest.php", "test", "informative"),
            ("composer.json", "config", "informative"),
            ("composer.lock", "lockfile", "lockfile"),
            ("vendor/autoload.php", "vendored", "vendored"),
            ("public/index.php", "source", "informative"),
            ("phpunit.xml", "config", "informative"),
        ],
    },
    "game-engine-assets": {
        "note": "engine + assets: the `.obj` ambiguity, wasm, shaders, committed binaries.",
        "paths": [
            ("src/renderer/rasterizer.cpp", "source", "informative"),
            ("shaders/lighting.hlsl", "source", "informative"),
            ("assets/models/crate.obj", "other", "informative"),
            ("assets/models/crate.fbx", "asset", "informative"),
            ("assets/textures/brick.png", "asset", "informative"),
            ("web/wasm/simd.wasm", "binary", "informative"),
            ("build/windows/game.exe", "generated", "generated"),
            ("src/wasm/bindings.cpp", "source", "informative"),
        ],
    },
    "openapi-codegen-service": {
        "note": "the ticket's openapi example: a spec is a contract, its client is output.",
        "paths": [
            ("api/openapi.yaml", "schema", "informative"),
            ("src/api/client.gen.ts", "generated", "generated"),
            ("src/api/schemas.gen.ts", "generated", "generated"),
            ("src/api/http.ts", "source", "informative"),
            ("openapi.json", "schema", "informative"),
            ("generated-sources/com/x/DefaultApi.java", "generated", "generated"),
            ("src/test/java/com/x/DefaultApiTest.java", "test", "informative"),
        ],
    },
}


def shapes() -> list[str]:
    return sorted(CORPUS)


def rows() -> list[tuple[str, str, str, str]]:
    """`(shape, path, expected_class, expected_noise)` for every row in the table."""
    return [(shape, path, cls, level)
            for shape in shapes()
            for path, cls, level in CORPUS[shape]["paths"]]


def paths() -> list[str]:
    return [row[1] for row in rows()]


def counts() -> dict[str, int]:
    """Row counts per class — the shape of the table, for the contract test's summary."""
    out: dict[str, int] = {}
    for _shape, _path, cls, _level in rows():
        out[cls] = out.get(cls, 0) + 1
    return dict(sorted(out.items(), key=lambda kv: (-kv[1], kv[0])))
