# Recipe: silence a finding

- **You get**: a detector that stops crying wolf, by the narrowest rule that actually works — and a record of
  having asked for the silence.
- **Time**: five minutes.
- **Needs**: the finding in front of you, with its suppression key.
- **Plan key**: `e4.3` (**reco-2ox**) — the catalogue and its one-key suppression.

The order of this recipe is the point. Each step is narrower than the last, and narrower is better: a
suppression that outlives the thing it was written for is a hole with a name on it, and the cost of a
false positive is not the noise, it is the next real finding getting skimmed past.

## 1 · Read the finding before you silence it

A finding carries four things, and two of them are the whole decision:

```text
src/parse/lexer.rs added 1 debug output line(s) that read as left behind: dbg!(&token);
  severity medium · confidence 0.8 · jump diff:ep-fix-debug-leftover#src/parse/lexer.rs
  suppress with  debug_leftover:src/parse/lexer.rs
```

Open the jump target. If the message is true and you do not care, that is a **dispute about your repository**
and step 2 or 3 is right. If the message is false — it is not debug output, it is a logging macro your project
defines — that is a **bug in the detector** and step 5 is right.

## 2 · Narrowest first: one finding, one place

```toml
# fathom.toml
[detectors]
suppress = ["debug_leftover:src/parse/lexer.rs"]
```

Keys are `detector:subject`, printed by every finding, and the subject is usually a path. Three properties are
load-bearing and are the reason the key is worth typing at all:

* **no episode id and no timestamp** in the key, so it survives a rebase, an amend and a squash. A key that
  moved with the history would be retyped or deleted, which is the same thing as having no suppression;
* **derived from the subject, never from the message**, because messages get reworded and a reword must not
  invalidate a suppression;
* **a glob is allowed** when you genuinely mean a family of paths (`secret_string:tests/fixtures/*`) — which
  is the widest form, so use it deliberately.

Then prove it took, rather than assuming:

```bash
fathom why <episode>          # the finding's contribution is gone, and the axis says why
fathom analyze <episode> --json | grep -c debug_leftover
```

## 3 · Whole detector, when the detector is wrong for this repository

```toml
[detectors]
suppress = ["secret_string"]
```

This is the off switch, and it is the one to use least. `secret_string` is the example not to do it to:
the asymmetry between "one more noisy finding" and "one committed credential" is the sharpest in the
catalogue ([the catalogue](../detectors/index.md#secret_string--secret-shaped-string) says which detectors
forgive a false positive and which do not).

## 4 · Tuning instead of silencing

If the detector is right in kind and wrong in size, change its knob and you keep the signal:

```toml
[detectors]
untested = { min_added_lines = 40 }      # "new logic owes a test", at a size that suits this repo
todo = { min_new = 3 }                   # one TODO in a stub-heavy codebase is not debt
max_findings = 8                         # fewer rows per detector per episode, with the remainder counted
```

The full key list, with defaults and owners, is generated on [the catalogue page](../detectors/index.md), and
`fathom config explain` — not in this build; `e0.8`'s owner, [the key spec](../concepts/config.md) is
generated from it — is what will print which layer won a value. Until then: a suppression in your user
config and a suppression in the clone are different decisions and should look different in the report,
so put repository-wide silences in the repository's `fathom.toml`.

## 5 · When the finding is simply wrong

Say so where it can be acted on, and do not leave the suppression as the only record:

1. **open an issue** with the finding text, the file's shape and the version line from `fathom --version`
   ([the channel](../concepts/feedback.md), or [the checklist for filing it](../troubleshooting.md#filing-something-worth-reading));
2. **note it in the sign-off**, because the ledger keeps reasons and `fathom review sign <episode> --note
   "secret_string false positive: fixture key"` is a record your successor can find:
   ```bash
   fathom review sign <episode> --note "secret_string: tests/fixtures/stripe_key.pem is a fixture, see #231"
   ```
3. **keep the suppression in the repository's `fathom.toml`, not your user config** if the false positive is a
   property of the repository. A suppression only you have is a suppression your team does not benefit from,
   and it will be rediscovered by the next person to run the tool.

## 6 · Audit the silences you have

A suppression list that grows without review is how a tool gets distrusted. Twice a year, or after any incident:

```bash
git log -p --follow -- fathom.toml | grep -n "suppress" -A 3   # every suppression, with the change that added it
```

Each entry came with a reason in its commit message, didn't it? If not, the ledger's
[dismiss vocabulary](../concepts/review-ledger.md#the-dismiss-vocabulary-is-closed) is the vocabulary to borrow:
a reason that cannot be counted is a reason nobody audits.

**Next**: [the catalogue](../detectors/index.md) for what each detector will and will not forgive ·
[risk score](../concepts/risk-score.md) for what a silenced finding does to the ranking — which is: it
disappears from the `findings` axis, and the queue's order changes, so silence is a tuning act with
consequences.
