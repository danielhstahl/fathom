# Theming, colour-blind safety, and colour that is not load-bearing

Status: built by `reco-2fv` (stage **e5.6**). Code: `crates/fathom-tui/src/theme.rs`,
`crates/fathom-tui/src/theme/{builtin,file}.rs`, `crates/fathom-tui/src/colour.rs`,
`crates/fathom-cli/src/theme.rs`. Surfaces: `--theme`, `fathom tui --show-theme [--audit]`,
`fathom tui --list-themes`, `Face::*` semantic tokens, `Theme::resolve` in the sink.
Contract: ADR-006 §10.2 (colour never carries a fact alone), §6.7 (`--plain`), and `e5.5`'s
probe (`docs/concepts/timeline.md` §… — `crates/fathom-tui/src/terminal.rs`).

Fathom shows a reviewer a *judgement*: this episode is riskier than that one, this line was added,
this one was deleted, this violation is marked on a line that was also added. Every one of those
sentences is currently rendered in a colour, and a colour is the least reliable channel in the
terminal — it is invisible to roughly one in twelve male reviewers, it is invisible to anybody whose
terminal is configured against their monitor, and it is invisible to anybody who has set `NO_COLOR`
because Fathom's own palette hurt them.

So this document is about two separable things, and keeping them separate is the design:

1. **colour is decoration that is measured**;
2. **the fact is shape and text, and is not measured because it never depended on colour**.

The second is ADR-006 §10.2 and predates this ticket. The first is what `e5.6` added.

---

## 1 The tokens, not the hex, are the API

Nothing outside `theme` names a colour. A view asks for a **face** — `Face::Added`, `Face::Failure`,
`Face::RiskHigh`, `Face::WordOld` — and the theme answers with a [`Paint`]: an ink, an optional
ground, and a shape.

```rust
// render.rs — what a call site looks like
let style = theme.style(Face::Added);
```

Twenty-four faces, one registry (`Face::ALL`), and one place each face gets its colour: the tables in
`theme/builtin.rs`. `crates/fathom-tui/tests/theming.rs` walks `crates/*/src` and fails the build if
a `Color::` literal or a `#rrggbb` appears outside `theme*`/`colour.rs`, because a colour chosen at a
call site is a colour no theme can override, no audit can measure and no fallback can fold.

Two things fell out of that rule that are worth naming: the **word-diff pair** previously painted
`white on Red`/`white on Green` (pure-hue grounds, `Color::Red` at that), and `Face::Whitespace` used
`Reverse` on a colour that was also the ink. Both are now expressed as a ground the theme chooses, so
the audit can see them.

## 2 What is measured, and where the floors come from

Every theme carries its own audit, and `--show-theme` prints it. Two floors, both stated in the
output rather than left in the source:

| floor | value | where it comes from |
| --- | --- | --- |
| contrast | **4.5:1** | WCAG 2.1 AA body text (1.4.3). A *dim* face is measured against **6.0:1**, because `DIM` is applied by the terminal to a colour Fathom cannot see, and a face that only just clears 4.5 before the terminal dims it does not clear it. |
| dichromacy separation | **ΔE ≥ 14** (truecolour), **ΔE ≥ 12** (sixteen-colour) | CIE76 ΔE in D65 Lab after a **Machado et al. (2009) severity-1.0** simulation of protanopia and deuteranopia. ΔE 2.3 is a just-noticeable difference; 14 is "clearly a different colour" for two patches side by side. The ANSI floor is lower and says so in the report: on a sixteen-colour terminal the hues belong to the reviewer's terminal, and what Fathom controls is which *slot* each face asks for. |

Against those floors the audit measures:

* **every face** against the ground it will actually be seen on (its own ground if it has one, else the
  theme's background) — and a face whose ground is the terminal's own is reported as
  `terminal's own colour, not measured`, which is a **declined** claim, not a passed one. The test
  asserts the *number* of measured faces, so nothing can clear the audit by being unmeasurable;
* **the pairs a reviewer has to tell apart** — added/removed, added/warning, removed/warning,
  warning/failure, each of those against text, and word-old/word-new — under both simulations.
  Separation is measured on the **ink**: the channel Fathom always paints. Grounds are for
  readability and are measured for contrast.

Tuning the palettes is a loop against those numbers (the arithmetic lives in `colour.rs`: WCAG
luminance, sRGB→Lab, the two matrices, xterm-256 quantisation and a `nearest_ansi` argmin). The shape
of what the tuning found, in one example: the classic green/red diff pair collapses under deuteranopia
to ΔE 7 — under the floor, and under the just-noticeable difference — which is why Fathom's pair
separates in **lightness** as well as hue (`#4fdca4` against `#ff8e7d`, ΔE 33 simulated), and why the
sixteen-colour mapping asks for the *bright* ends of those slots, where the lightness gap is largest.

Two checks keep the audit from being a ceremony:

* `the_simulation_collapses_what_it_claims_to_collapse` asserts that plain green against plain red
  **does** collapse under the simulation. If the matrix were a no-op, every palette would pass.
* `--show-theme` prints its own margins — "worst face whitespace at 4.78:1 (floor 4.5)", "closest audited pair: added vs removed under protanopia at ΔE 17.1 (floor 14.0)" — because a bare "pass" tells the next person to edit a palette nothing about how much room there was, and floors get shaved that way.

* `--show-theme --audit` exits **7** (`CHECK_VIOLATIONS`, the same code `fathom check` uses) when a
  floor fails, so a palette in a dotfiles repo can be gated the way a codebase is.

## 3 The three built-ins

| name | background | for | why it is shaped like this |
| --- | --- | --- | --- |
| `dark` | `#151821` | the default | twenty-four hand-written inks: worst face `whitespace` at 4.78:1, closest audited pair added/removed at ΔE 17.1 under protanopia |
| `light` | `#f7f7f4` | paper grounds, projectors, bright rooms | not `dark` inverted, which would be a different palette with the same flaws: the inks are darker than the dark theme's and the state pair is re-separated in lightness (a mid-tone green on white is 1.6:1). Worst face `added` at 5.00:1, closest pair ΔE 15.3. |
| `ansi` | *none* | `TERM=vt100`, `screen` with a plain `termcapinfo`, a Linux virtual console | slot names only. It paints **no background**, because the terminal already has one and it is the reviewer's: repainting it would destroy their contrast and repaint `--plain`'s text too. It therefore **declines** the contrast half of the audit and keeps the separation half. |

`dark` and `light` each carry a hand-written sixteen-colour mapping (`with_basic`) used when the probe
answers `Basic`. That mapping is not a quantisation of the truecolour inks: the nearest slot to
`dark`'s mint `#4fdca4` is a cyan, which would put *added* on the same slot as *directory*. The test
that guards this is `a_sixteen_colour_terminal_gets_the_slot_mapping_not_a_quantization`, and it also
re-runs the separation audit on the folded palette, because the mapping is a theme in all but name.

One honest limitation is documented rather than fixed: the sixteen slots are tuned against a dark
ground, because a bare ANSI terminal has one, and they contain no paper-readable green. A reviewer on
a white background in a sixteen-colour terminal gets the slot table plus the shape channel; the one
who wants the other reading writes the file, and the audit will measure what they wrote. Adding a
built-in "light ANSI" was tried and **refused**: the sixteen hues are not Fathom's, and no hand choice
of slots clears the truecolour floor for all four state faces against a reference palette that is also
not Fathom's. A second built-in would have bought that floor by lowering it.

## 4 A theme is a file

`--theme` takes a built-in name, a path, or a name that resolves to `<name>.toml` in a theme
directory. The parser lives in `fathom-tui` and is **handed the text**: `fathom-tui` opens no file and
reads no variable (ADR-002 §3), so `fathom-cli` does the finding and the reading.

```toml
name = "dusk"
inherits = "dark"          # or "none" to start from the terminal's own colours
background = "#101014"
foreground = "#e8e8ee"

[faces]
added = "#3fd49a"                            # the string form …
warning = "bright-yellow bold underline"     # … ink, optional `on <ink>`, then shapes
hunk = { ink = "#79c0ff", bold = true }      # the table form
word-old = { ink = "#ffe9e5", on = "#6e2a24" }
```

A file overrides what it names and inherits the rest — six rows is six rows, not a re-paint of
twenty-four. Every problem in the document is reported at once, with the file and the key in the
sentence, because a broken theme that reports its first error is an afternoon:

```
fathom: mine.toml: [faces] adde names no face; the names are text, muted, faint, …
fathom: mine.toml: inherits = "midnight" names no theme; the built-ins are dark, light, ansi, or "none" …
fathom: mine.toml was refused, so dark is in force. `fathom tui --show-theme` prints the theme in force and its audit.
```

A theme that is *refused* never half-applies, and the fallback is named out loud rather than assumed.

## 5 Precedence, and what is deliberately not a layer

```
--theme  >  $FATHOM_THEME  >  [theme] name in the user's config  >  built-in `dark`
```

and the file lookup for a name is built-in → `$FATHOM_THEME_DIR/<name>.toml` →
`~/.config/fathom/themes/<name>.toml` → `~/.fathom/themes/<name>.toml`.

**A repository's `fathom.toml` is not a layer**, and `crates/fathom-cli/tests/theme_cli.rs` asserts
it: a `[theme]` table in a clone changes nothing. ADR-001 §4.6 already treats config from a clone as
untrusted config where consent and tokens are concerned; how somebody's screen looks is the same kind
of decision — a reviewer who has turned the contrast up to see the diff at all does not want a
checkout to turn it back down, and a cloned repository choosing the palette is a vector dressed up as
a convenience. `--list-themes` names the directories that *are* searched, so the rule is discoverable
rather than folklore.

## 6 Colour off: `NO_COLOR`, `TERM=dumb`, and what the frame still says

`NO_COLOR` set to any value — including empty, which the standard says is still "set" — outranks
`COLORTERM=truecolor`. The probe (`e5.5`) answers `ColorDepth::None`, and the sink folds the theme
through `Theme::resolve(None)`, which takes every ink to `Default`, drops every ground, and changes
no shape.

The acceptance sentence is *"with NO_COLOR all information remains distinguishable via glyphs and
labels"*, and the form of it that can actually be checked is a screenshot test:
`with_no_colour_the_frame_is_the_same_frame_and_says_the_same_things` draws the timeline into a real
`TerminalSink` twice — once as a truecolour terminal would receive it, once with `NO_COLOR` — and
asserts that

* some cell **was** coloured in the first frame (so the comparison proves something);
* **no** cell is coloured in the second;
* the two frames' text are **byte-identical**, which is the strongest form of "colour is decoration":
  if the text differed, something was travelling in the colour channel;
* the carriers are present: `+`, `−`, the risk ladder `●`/`○`, `⚑`, `✔`, and `?` for an author Fathom
  could not place.

The same folding is what `TERM=dumb` and `--plain` reach: `--plain` never enters the alternate screen
and emits no escape sequences at all (ADR-006 §6.7), and the theme is not consulted for a text render
— which is why `--theme` cannot change one, and `no_colour_still_prints_the_frame…` says so.

## 7 Where the depth folding happens, and why there

`Theme::resolve(depth)` folds a palette to what the terminal was answered: truecolour stays
truecolour, `Ansi256` maps each colour to its nearest palette entry, `Basic` swaps in the theme's
hand-written slot mapping, `None` strips colour.

It happens in `TerminalSink::draw`, once per frame, against the probe the sink is about to paint
into — not in the CLI at startup. `--show-theme` therefore audits the palette **as chosen** and prints
the terminal's answer beside it (`terminal: xterm · 16 colours · not UTF-8`), which is what lets a
reviewer see both "what I asked for" and "what this terminal will get" in one screen. The help overlay
(`?`) names the theme, the terminal and the colour depth for the same reason: "my theme did not arrive"
should be answerable in the terminal where it failed.

## 8 Deliberately not decided here

* A `--colour-scheme` for the *export* artefacts (HTML/Markdown). The export has its own stylesheet
  question, and it is `e6`'s.
* Theme files that ship inside a repository, the way `fathom.toml` does. See §5 for why this one is
  closed rather than open.
* Tritanopia and anomalous trichromacy as floors. The simulations run all three (protanopia,
  deuteranopia, tritanopia) and the first two are the floors, at ~8 % and ~1 % of male reviewers
  against ~0.003 %: tritanopia is printed in the report and not gated, because a floor set by a
  rarity that low is a floor that constrains every palette to serve almost nobody.
