## fathom check

**fathom check passed be5f9acf5452..ed40e856ffec: 37 finding(s) below the `never` threshold, 5 check(s) ran** — `be5f9acf5452..ed40e856ffec` · policy v1 · mode warn · fail at `never`

| check | volume | status | judged | findings |
|---|---|---|---|---|
| `layering` | error | blocked | — | 0 |

> `layering` could not run: fathom.toml declares no [layering] rules, so nothing is enforced
>
> Try: declare `[layers]` and `[layering]`, or set `[check.severity] layering = "off"` to say you meant it
| `cycles` | error | ran | 0 | 0 |
| `api` | error | ran | 625 | 29 |
| `dependencies` | warning | ran | 104 | 7 |
| `tests` | warning | ran | 200 | 1 |
| `sign-off` | warning | ran | 40 | 0 |

| severity | rule | what | where |
|---|---|---|---|
| error | `api/breaking` | breaking ExtBrowseOpts pkg/cmd/extension/browse/browse.go:22 — member type changed — was `*readmeGetter`, now `readmeLoader`; member added — `renderReadme func(string, int) (string, error)`; member added — `runProgram   func(tea.Model, ...tea.ProgramOption) error` | pkg/cmd/extension/browse/browse.go:22 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking ExtractZip internal/zip/zip.go:21 — parameter type changed — was `safepaths.Absolute`, now `*safepaths.Root` | internal/zip/zip.go:21 (api:package github.com/cli/cli/v2/internal/zip) |
| error | `api/breaking` | breaking IsOfficial pkg/extensions/official.go:48 — parameter renamed — `name` is now `ext`; parameter type changed — was `string`, now `Extension`; parameter removed — position 2 (`owner: string`) is gone | pkg/extensions/official.go:48 (api:package github.com/cli/cli/v2/pkg/extensions) |
| error | `api/breaking` | breaking Test_extEntry pkg/cmd/extension/browse/browse_test.go:193 — public item removed — fn Test_extEntry was last seen at pkg/cmd/extension/browse/browse_test.go:193 (func Test_extEntry(t *testing.T)) | pkg/cmd/extension/browse/browse_test.go:193 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking Test_extList pkg/cmd/extension/browse/browse_test.go:270 — public item removed — fn Test_extList was last seen at pkg/cmd/extension/browse/browse_test.go:270 (func Test_extList(t *testing.T)) | pkg/cmd/extension/browse/browse_test.go:270 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking Test_getExtensionRepos pkg/cmd/extension/browse/browse_test.go:68 — public item removed — fn Test_getExtensionRepos was last seen at pkg/cmd/extension/browse/browse_test.go:68 (func Test_getExtensionRepos(t *testing.T)) | pkg/cmd/extension/browse/browse_test.go:68 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking Test_getSelectedReadme pkg/cmd/extension/browse/browse_test.go:25 — public item removed — fn Test_getSelectedReadme was last seen at pkg/cmd/extension/browse/browse_test.go:25 (func Test_getSelectedReadme(t *testing.T)) | pkg/cmd/extension/browse/browse_test.go:25 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking Uploader.UploadAndAttach internal/attachments/attach.go:85 — required parameter added — `markdownDir: string` must be supplied, so every existing call site has to change | internal/attachments/attach.go:85 (api:package github.com/cli/cli/v2/internal/attachments) |
| error | `api/breaking` | breaking extEntry.Title pkg/cmd/extension/browse/browse.go:66 — public item removed — method extEntry.Title was last seen at pkg/cmd/extension/browse/browse.go:66 (func (e extEntry) Title() string) | pkg/cmd/extension/browse/browse.go:66 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.Filter pkg/cmd/extension/browse/browse.go:283 — public item removed — method extList.Filter was last seen at pkg/cmd/extension/browse/browse.go:283 (func (el *extList) Filter(text string)) | pkg/cmd/extension/browse/browse.go:283 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.FindSelected pkg/cmd/extension/browse/browse.go:270 — public item removed — method extList.FindSelected was last seen at pkg/cmd/extension/browse/browse.go:270 (func (el *extList) FindSelected() (extEntry, int)) | pkg/cmd/extension/browse/browse.go:270 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.Focus pkg/cmd/extension/browse/browse.go:236 — public item removed — method extList.Focus was last seen at pkg/cmd/extension/browse/browse.go:236 (func (el *extList) Focus()) | pkg/cmd/extension/browse/browse.go:236 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.InstallSelected pkg/cmd/extension/browse/browse.go:222 — public item removed — method extList.InstallSelected was last seen at pkg/cmd/extension/browse/browse.go:222 (func (el *extList) InstallSelected()) | pkg/cmd/extension/browse/browse.go:222 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.PageDown pkg/cmd/extension/browse/browse.go:252 — public item removed — method extList.PageDown was last seen at pkg/cmd/extension/browse/browse.go:252 (func (el *extList) PageDown()) | pkg/cmd/extension/browse/browse.go:252 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.PageUp pkg/cmd/extension/browse/browse.go:256 — public item removed — method extList.PageUp was last seen at pkg/cmd/extension/browse/browse.go:256 (func (el *extList) PageUp()) | pkg/cmd/extension/browse/browse.go:256 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.Refresh pkg/cmd/extension/browse/browse.go:240 — public item removed — method extList.Refresh was last seen at pkg/cmd/extension/browse/browse.go:240 (func (el *extList) Refresh()) | pkg/cmd/extension/browse/browse.go:240 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.RemoveSelected pkg/cmd/extension/browse/browse.go:226 — public item removed — method extList.RemoveSelected was last seen at pkg/cmd/extension/browse/browse.go:226 (func (el *extList) RemoveSelected()) | pkg/cmd/extension/browse/browse.go:226 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.Reset pkg/cmd/extension/browse/browse.go:245 — public item removed — method extList.Reset was last seen at pkg/cmd/extension/browse/browse.go:245 (func (el *extList) Reset()) | pkg/cmd/extension/browse/browse.go:245 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.ScrollDown pkg/cmd/extension/browse/browse.go:261 — public item removed — method extList.ScrollDown was last seen at pkg/cmd/extension/browse/browse.go:261 (func (el *extList) ScrollDown()) | pkg/cmd/extension/browse/browse.go:261 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.ScrollUp pkg/cmd/extension/browse/browse.go:265 — public item removed — method extList.ScrollUp was last seen at pkg/cmd/extension/browse/browse.go:265 (func (el *extList) ScrollUp()) | pkg/cmd/extension/browse/browse.go:265 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.createModal pkg/cmd/extension/browse/browse.go:133 — public item removed — method extList.createModal was last seen at pkg/cmd/extension/browse/browse.go:133 (func (el *extList) createModal() *tview.Modal) | pkg/cmd/extension/browse/browse.go:133 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.toggleInstalled pkg/cmd/extension/browse/browse.go:230 — public item removed — method extList.toggleInstalled was last seen at pkg/cmd/extension/browse/browse.go:230 (func (el *extList) toggleInstalled(ix int)) | pkg/cmd/extension/browse/browse.go:230 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking extList.toggleSelected pkg/cmd/extension/browse/browse.go:144 — public item removed — method extList.toggleSelected was last seen at pkg/cmd/extension/browse/browse.go:144 (func (el *extList) toggleSelected(verb string)) | pkg/cmd/extension/browse/browse.go:144 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking fakeGroup.Add pkg/cmd/extension/browse/browse.go:106 — public item removed — method fakeGroup.Add was last seen at pkg/cmd/extension/browse/browse.go:106 (func (w *fakeGroup) Add(int)) | pkg/cmd/extension/browse/browse.go:106 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking fakeGroup.Done pkg/cmd/extension/browse/browse.go:107 — public item removed — method fakeGroup.Done was last seen at pkg/cmd/extension/browse/browse.go:107 (func (w *fakeGroup) Done()) | pkg/cmd/extension/browse/browse.go:107 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking fakeGroup.Wait pkg/cmd/extension/browse/browse.go:108 — public item removed — method fakeGroup.Wait was last seen at pkg/cmd/extension/browse/browse.go:108 (func (w *fakeGroup) Wait()) | pkg/cmd/extension/browse/browse.go:108 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking wGroup.Add pkg/cmd/extension/browse/browse.go:99 — public item removed — method wGroup.Add was last seen at pkg/cmd/extension/browse/browse.go:99 (Add(int)) | pkg/cmd/extension/browse/browse.go:99 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking wGroup.Done pkg/cmd/extension/browse/browse.go:100 — public item removed — method wGroup.Done was last seen at pkg/cmd/extension/browse/browse.go:100 (Done()) | pkg/cmd/extension/browse/browse.go:100 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| error | `api/breaking` | breaking wGroup.Wait pkg/cmd/extension/browse/browse.go:101 — public item removed — method wGroup.Wait was last seen at pkg/cmd/extension/browse/browse.go:101 (Wait()) | pkg/cmd/extension/browse/browse.go:101 (api:package github.com/cli/cli/v2/pkg/cmd/extension/browse) |
| warning | `dependencies/new-direct` | github.com/stretchr/testify v1.12.1 is a new direct dependency (go: new in .github/skills/cli-exercise/tool/go.sum) | .github/skills/cli-exercise/tool/go.sum (hunk:@@ -1,0 +1,12 @@) |
| warning | `dependencies/new-direct` | golang.org/x/image v0.45.0 is a new direct dependency (go: new in .github/skills/cli-exercise/tool/go.sum) | .github/skills/cli-exercise/tool/go.sum (hunk:@@ -1,0 +1,12 @@) |
| warning | `dependencies/new-direct` | golang.org/x/sys v0.48.0 is a new direct dependency (go: new in .github/skills/cli-exercise/tool/go.sum) | .github/skills/cli-exercise/tool/go.sum (hunk:@@ -1,0 +1,12 @@) |
| warning | `dependencies/new-direct` | golang.org/x/term v0.45.0 is a new direct dependency (go: new in .github/skills/cli-exercise/tool/go.sum) | .github/skills/cli-exercise/tool/go.sum (hunk:@@ -1,0 +1,12 @@) |
| warning | `dependencies/new-direct` | golang.org/x/text v0.41.0 is a new direct dependency (go: new in .github/skills/cli-exercise/tool/go.sum) | .github/skills/cli-exercise/tool/go.sum (hunk:@@ -1,0 +1,12 @@) |
| warning | `dependencies/new-direct` | tuistory 0.11.0 is a new direct dependency (js: new in .github/skills/cli-exercise/scripts/package-lock.json) | .github/skills/cli-exercise/scripts/package-lock.json (hunk:@@ -1,0 +1,948 @@) |
| warning | `dependencies/new-source` | a new dependency source `registry:registry.npmjs.org` is used by this range (@clack/core 1.5.1) | .github/skills/cli-exercise/scripts/package-lock.json (hunk:@@ -1,0 +1,948 @@) |
| warning | `tests/untested-logic` | episode ep-c6b6f0af7 adds 1589 lines of new logic with 520 test lines beside it, under the gate's floor of 50% tested | internal/safeurl/safeurl.go (index:episode ep-c6b6f0af7) |

Notices:

- fathom.toml is empty, so the gate runs on its built-in defaults
- `check.risk_episodes` is 40, so the newest 40 of 200 episodes were scored for sign-off

