# Episode risk scoring: `features-3`, the twelve axes, and the calibration set that tunes them

This page moved. Its prose is now in
[Risk: two numbers, and what each one is a claim about](risk.md), which `reco-msv1.2` (`e12.2`) merged from
`risk-score.md`, `risk-scoring.md` and `risk-ranking.md` under
[the documentation merge map](../reference/doc-map.md)'s M1. Nothing was dropped, and this page is a
signpost: it restates no claim and holds no table. The axis table §2 carried is generated now —
`python3 tools/gen_risk_tables.py write` writes it from `risk::feature::SPECS`.

Where this page's sections went:

- §1 → [The question, and the three things this refuses](risk.md#the-question-and-the-three-things-this-refuses)
- §2 → [The twelve features](risk.md#the-twelve-features), and
  [Who wrote it: the axis that used to be called `authorship`](risk.md#who-wrote-it-the-axis-that-used-to-be-called-authorship)
- §3 → [The arithmetic](risk.md#the-arithmetic) · §4 → [A worked example](risk.md#a-worked-example)
- §5 → [Weights in `fathom.toml`](risk.md#weights-in-fathomtoml) · §6 → [Calibration](risk.md#calibration-fathom-why---calibrate)
- §7 → [What this score cannot see](risk.md#what-this-score-cannot-see)
- §8 → [Retuning, and what to check afterwards](risk.md#retuning-and-what-to-check-afterwards)
- §9 → [Where it lives, and what is not done](risk.md#where-it-lives-and-what-is-not-done)
