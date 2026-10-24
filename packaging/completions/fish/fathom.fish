# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_fathom_global_optspecs
    string join \n plain no-alt-screen fresh mouse lines theme= demo progress no-progress no-watch detectors= enable-detector= disable-detector= events= h/help V/version
end

function __fish_fathom_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_fathom_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_fathom_using_subcommand
    set -l cmd (__fish_fathom_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c fathom -n "__fish_fathom_needs_command" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_needs_command" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_needs_command" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_needs_command" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_needs_command" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_needs_command" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_needs_command" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_needs_command" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_needs_command" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_needs_command" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_needs_command" -l demo -d 'Review a miniature repository that ships inside this binary, then say what to do next (`e5.9`)'
complete -c fathom -n "__fish_fathom_needs_command" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_needs_command" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_needs_command" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_needs_command" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "index" -d 'Index the repository so every other verb has something to read'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "show" -d 'Show one episode in full'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "episodes" -d 'List the episodes — the rail\'s rows, headless (`reco-11b`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "attribution" -d 'Report or correct who Fathom thinks wrote a commit (`e4.1`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "review" -d 'The review ledger: sign-offs that outlive the history they were written about (`e4.6`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "analyze" -d 'Print one episode\'s delta document — the model the TUI, the export and an agent all read'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "narrative" -d 'Read one episode: the narrative every claim in it cites its evidence from (`e4.7`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "drift" -d 'What the plan said against what the session did — the contradictions, both sides linked (`e4.8`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "why" -d 'Print one episode\'s risk score with every feature, its weight and its contribution (`e4.4`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "stats" -d 'Report repo scale, index state and cache hit rate'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "check" -d 'Run the checks CI cares about; findings above the threshold exit 7'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "doctor" -d 'Describe this build, its environment and its index, with a remedy per finding'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "export" -d 'Write a review artefact for a pull request or an audit'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "mcp" -d 'Speak MCP to a coding agent'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "watch" -d 'Watch the repository and index as it changes'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "ambient" -d 'The glance pane for the terminal beside the agent: latest episode, queue, index, one reason (`reco-8bx`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "hooks" -d 'Report on the harness event stream: lag, offset, rejections, replay (`e6.1`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "daemon" -d 'The one process per repository that watches, tails the harness stream and indexes (`e6.3`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "status" -d 'Who is keeping this index current, what it costs, and what is left (`e6.3`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "tui" -d 'Print the terminal UI\'s keymap, or open the UI itself'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "open" -d 'Open an episode\'s file at its line in `$EDITOR` — jump out, do not read around it (`e5.8`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "difftool" -d 'Show one file\'s two sides in your own difftool — difftastic, meld, kdiff3, `vim -d` (`e5.8`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "cmp" -d 'Compare any two episodes or revisions: the composed delta over a range (`e5.3`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "search" -d 'Search the whole index from one box, in the shell\'s own ranking (`e5.2`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "architecture" -d 'The architecture scorecard: five dimensions, one number, and the drift since your last review (`e3.9`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "ratchet" -d 'The ratchet: the committed allowance ledger, which tightens on its own and widens only when told to (`e3.10`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "context" -d 'What this repository knows about a path or an episode: the recorded decisions and the open questions (`e6.5`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "note" -d 'The review memory: notes, decisions, questions and follow-ups, committed under `.fathom/reviews/` (`e6.5`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "completions" -d 'Print the shell\'s own copy of this grammar: completion scripts and man pages (`e7.4`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "events" -d 'Read the analysis event stream a run wrote: what happened, in order (`e6.7`)'
complete -c fathom -n "__fish_fathom_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand index" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand index" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand index" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand index" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand index" -l stats -d 'Print the per-analyzer cache table (ADR-004 §4.8)'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l json -d 'Print the run report as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l refresh -d 'Look up every stored unit, and recompute the ones that hit too'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l rebuild -d 'Throw the epoch away and rebuild every analysis from scratch'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l watch -d 'Index once, then keep watching and index as the repository changes'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l committed-only -d 'Do not analyse uncommitted paths (ADR-004 §6.4)'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l prune -d 'Remove the commit rows no ref can reach — what a rebase, an `--amend` or a force-push leaves behind'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l no-graph -d 'Do not write the module graph (`e3.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l dry-run -d 'With `--prune`: print what would be removed and remove nothing'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand index" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand index" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand index" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand show" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand show" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand show" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand show" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand show" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand show" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand show" -l json -d 'Print the episode as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand show" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand show" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand show" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand show" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand show" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand show" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand show" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand show" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand show" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand show" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l limit -d 'How many rows to print, newest first; `0` is every row the filters select' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l offset -d 'Skip this many rows before printing, for paging with `--limit`' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l author -d 'Only episodes by this author, exactly as git recorded the name; repeatable' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l kind -d 'Only episodes whose author kind is this — `agent`, `human` or `unknown`; repeatable' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l state -d 'Only episodes in this review state: `unread`, `read`, `accepted`, `flagged`, `dismissed`, `snoozed`' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l path -d 'Only episodes that touched a path containing this fragment' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l since -d 'Only episodes whose last commit is at or after this date — `7d`, `2w`, `today`, `yesterday`, `2026-10-01`: the same grammar `fathom search since:` takes' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l until -d 'Only episodes whose last commit is before this date, in the same grammar as `--since`' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l json -d 'Print the list as one JSON value on stdout (`fathom.cli.episodes/1`), and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l include-dead -d 'Include episodes no live commit belongs to — the ghosts a rebase, an amend or a force-push left'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand episodes" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -f -a "correct" -d 'Store a correction: these identities were written by an agent / by a person'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -f -a "list" -d 'List the stored corrections, which are the keys the classifier honours'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -f -a "forget" -d 'Forget one stored correction'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and not __fish_seen_subcommand_from correct list forget help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l root -d 'The repository to write to' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l kind -d '`agent`, `human` or `unknown`' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l agent -d 'Which agent the identity is, when `--kind agent`' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l note -d 'Why the label was overruled, for whoever reads it next' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l identity -d 'Correct one identity key directly (`identity:<address>`), instead of deriving them from a commit' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l json -d 'Print the result as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from correct" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l json -d 'Print the corrections as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from list" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l root -d 'The repository to write to' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l json -d 'Print the result as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from forget" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from help" -f -a "correct" -d 'Store a correction: these identities were written by an agent / by a person'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from help" -f -a "list" -d 'List the stored corrections, which are the keys the classifier honours'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from help" -f -a "forget" -d 'Forget one stored correction'
complete -c fathom -n "__fish_fathom_using_subcommand attribution; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l json -d 'Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -f -a "status" -d 'The verdict: is every episode covered by a sign-off that still matches (exit `7` when it is not)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -f -a "sign" -d 'Sign an episode off, appending one entry to `.fathom/reviews/ledger.md`'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -f -a "sync" -d 'Render the queue\'s acts that the ledger has never seen'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -f -a "list" -d 'List the ledger\'s entries with their verdicts'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -f -a "verify" -d 'Re-hash every entry against its own `digest:` line — tampering, and nothing else'
complete -c fathom -n "__fish_fathom_using_subcommand review; and not __fish_seen_subcommand_from status sign sync list verify help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l json -d 'Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from status" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l decision -d 'What the decision is; `accepted` is the one that means *reviewed*' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l note -d 'A note, committed with the entry and folded onto one line' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l identity -d 'Who is signing, when git config and `fathom.toml` do not say' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l agent -d 'Record the act as an agent\'s verdict on someone\'s behalf, not as a human acceptance' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l json -d 'Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sign" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l dry-run -d 'Report what would be rendered, write nothing'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l json -d 'Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from sync" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l state -d 'Show only entries in this state: `fresh`, `stale`, `orphan` or `unknown`' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l json -d 'Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from list" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l json -d 'Print the verdict and every entry as one JSON line (machine contract `fathom.cli.review/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from verify" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from help" -f -a "status" -d 'The verdict: is every episode covered by a sign-off that still matches (exit `7` when it is not)'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from help" -f -a "sign" -d 'Sign an episode off, appending one entry to `.fathom/reviews/ledger.md`'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from help" -f -a "sync" -d 'Render the queue\'s acts that the ledger has never seen'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from help" -f -a "list" -d 'List the ledger\'s entries with their verdicts'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from help" -f -a "verify" -d 'Re-hash every entry against its own `digest:` line — tampering, and nothing else'
complete -c fathom -n "__fish_fathom_using_subcommand review; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l json -d 'Print the delta document as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l schema -d 'Print `schemas/episode-delta.schema.json`, the JSON Schema for the document, and stop'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l check -d 'Check the checked-in schema against what this build generates; the CI form of `--schema`'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l write -d 'Write the generated schema to `schemas/`, replacing the checked-in copy'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand analyze" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l top -d 'How many claims the *Read this first* paragraph holds (1 to 6)' -r
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l json -d 'Print the claims, citations and weights as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l markdown -d 'Print the Markdown artefact body — the exact text `fathom export` will carry'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l explain -d 'Print the ranking beside the prose: every claim\'s weight, so the order can be argued with'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l attribution -d 'Print whether a model wrote any of this, and the five gates a polish path must clear. Needs no index and no episode'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand narrative" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l session -d 'Compare one harness session only — its id, or `lineage:refs/heads/main` for the episodes no session was named for' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l window -d 'How many episodes past a plan item to look for the change that did it' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l min-overlap -d 'The token overlap at which a plan item counts as addressed (0.05 to 1.0)' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l max -d 'The cap on the report' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l dismiss -d 'A suppression key already argued about; its contradiction is counted, not re-reported. Repeatable' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l off -d 'Switch one kind off, in the published spelling (`plan-unimplemented`, …). Repeatable' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l all -d 'Read every episode in the index instead of the newest ones. A long read, and a longer report'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l json -d 'Print the report as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l fail-on-findings -d 'Exit 7 when anything was found, so a CI step can gate on it'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand drift" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l calibrate -d 'Print the calibration report — precision and recall at the top of the queue, as shipped and as retuned — instead of a score. With a path, that JSON set is measured; without one, the set shipped with the binary' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l weight -d 'Override one weight for this run only: `--weight findings=0.25`. Repeatable. Nothing is written to `fathom.toml`; the score says which weights were overridden' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l tau -d 'Override the flag line for this run only, 0 to 100' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l cut -d 'How deep the queue is, as a fraction, for `--calibrate`. Defaults to 0.1 — the top ten percent' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand why" -l json -d 'Print the same answer as JSON, for a machine'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l weights -d 'Print the feature names this build can score, and stop'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand why" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand why" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand why" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l json -d 'Print the same report as JSON, for a machine'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand stats" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l root -d 'The repository to check' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l base -d 'The far end of the range: what the change is compared against' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l head -d 'The near end of the range. Defaults to `[check] head`, then to `HEAD`' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l range -d 'The range as one argument, `base..head` — what a reviewer pastes into a message' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l format -d 'The output format: `human`, `json`, `sarif`, `github` (step annotations) or `markdown` (job summary)' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l out -d 'Write the artefact to this file instead of to stdout' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l summary -d 'Where to append the markdown job summary. Defaults to `$GITHUB_STEP_SUMMARY` when the runner set one' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l fail-on -d 'The severity at or above which the run exits 7: `note`, `warning`, `error` or `never`' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l severity -d 'Set one check\'s volume for this run, e.g. `--severity tests=off`. Repeatable' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l only -d 'Run only these checks. Repeatable; the others are reported as off, not omitted' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l skip -d 'Do not run these checks. Repeatable' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l require -d 'Checks that must *run*: one that cannot is a failure rather than a notice. Repeatable' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l ignore -d 'Silence one finding or family for this run: `tests`, or `layering:no_upward`. Repeatable' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l max -d 'Cap the findings listed per check (the counts stay whole). Overrides `[check] max_findings`' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand check" -l json -d 'Print the JSON document — the same thing as `--format json`'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l no-summary -d 'Do not write a job summary at all, even with `$GITHUB_STEP_SUMMARY` set'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l no-annotations -d 'Do not print GitHub step annotations beside the report'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l explain -d 'Print the policy in force — every check\'s volume, every threshold — and exit 0 without checking'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand check" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand check" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand check" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l root -d 'The repository to report on' -r
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l privacy -d 'Print the network-feature and consent table'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l json -d 'Print the same report as JSON, for a machine'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l deep -d 'Run the whole-database `integrity_check` rather than `quick_check` (ADR-004 §9.6)'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l strict -d 'Exit with the code of the worst finding instead of 0 — what CI and a pre-flight check want'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand doctor" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l format -d 'The artefact format: markdown, html or json' -r
complete -c fathom -n "__fish_fathom_using_subcommand export" -l out -d 'The file stem to write, or a file name that already carries the format\'s extension' -r
complete -c fathom -n "__fish_fathom_using_subcommand export" -l episodes -d 'The episode range to export, e.g. `HEAD~9..HEAD`, one commit, or one episode id' -r
complete -c fathom -n "__fish_fathom_using_subcommand export" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand export" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand export" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand export" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand export" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand export" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand export" -l json -d 'Print the report as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l stdout -d 'Write the artefact to stdout instead of a file; the report goes to stderr'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l strict -d 'Exit 7 while the artefact\'s checklist still has an open item — the form a CI gate wants'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand export" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand export" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand export" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -f -a "serve" -d 'Serve the tool surface over stdio, which opens no socket (`e6.4`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -f -a "tools" -d 'Print the tool surface and its bounds as this repository\'s config resolves them'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -f -a "approval-token" -d 'Mint the token that authorises `mark_reviewed` for exactly one change'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and not __fish_seen_subcommand_from serve tools approval-token help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from serve" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l root -d 'The repository whose `[mcp]` config and index to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l json -d 'The surface as one JSON document on stdout (`fathom.mcp.surface/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from tools" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l root -d 'The repository whose `[mcp]` config holds the approval secret' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from approval-token" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from help" -f -a "serve" -d 'Serve the tool surface over stdio, which opens no socket (`e6.4`)'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from help" -f -a "tools" -d 'Print the tool surface and its bounds as this repository\'s config resolves them'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from help" -f -a "approval-token" -d 'Mint the token that authorises `mark_reviewed` for exactly one change'
complete -c fathom -n "__fish_fathom_using_subcommand mcp; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l max-cycles -d 'Stop after this many looks at the filesystem' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l deadline -d 'Stop after this many seconds' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l backend -d 'Which change source to use: `auto`, `notify` or `poll`' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l interval-ms -d 'The debounced polling interval in milliseconds' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l debounce-ms -d 'The quiescence window in milliseconds: how long the filesystem must be quiet before a change is read' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l heartbeat-ms -d 'The liveness beat in milliseconds: how often the state is rewritten while nothing changes' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l chunk-units -d 'Analysis units per index chunk — the watcher\'s slice of the CPU' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l json -d 'Print one JSON object per state change on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l once -d 'Index once, then exit: what a script or a CI check wants from `watch`'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l worktree -d 'Also re-index uncommitted paths when only the index moved'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l no-status-file -d 'Do not write `.fathom/watch/status.json`'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l quiet -d 'Print nothing to stdout but errors; the state machine still goes to the status file'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand watch" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l interval-ms -d 'How often to look, in milliseconds. 400 is what meets the "under a second" promise at near-zero cost' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l deadline -d 'Stop after this many seconds' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l chunk-units -d 'Episodes to index per slice when this pane is the one keeping the index current' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l width -d 'Render at this width instead of the terminal\'s — for a pipe, a test, or a tmux status line' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l json -d 'Print the pane as one JSON object per change (machine contract `fathom.cli.ambient/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l once -d 'Print the pane once and exit; never take the screen'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l plain -d 'Draw the pane as text with no terminal taken (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l no-index -d 'Never index from the pane: read whatever the last index run left, whoever ran it'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l self-check -d 'Run to the deadline and print what the pane cost: polls, redraws and this process\'s own CPU share. Exits `11` if it blew the budget of 1 % of a core'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand ambient" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l events -d 'Read this events file instead of the configured one (ADR-005 §3.1\'s first choice)' -r
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l max -d 'With `--tail`: print at most this many records' -r
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l trailer -d 'Print the `Agent-Session:` / `Agent-Turn:` trailer pair for `<session>:<turn-index>`' -r
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l check -d 'Report the stream and change nothing. This is the default; the flag exists so a script can say what it means'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l replay -d 'Rebuild everything from byte 0 and rewrite the offset: what an index rebuild does first'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l tail -d 'Read forward from the durable offset, advancing it, and print what arrived'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l json -d 'Print the same report as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l strict -d 'Exit 7 when the stream is not healthy — the form a CI gate wants'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand hooks" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -f -a "start" -d 'Take the writer lease and watch this repository from this process, or from a detached one'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -f -a "stop" -d 'Ask the daemon to finish its pass, checkpoint and release the lease'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -f -a "status" -d 'The daemon\'s own health report (`fathom status` is the same question at the top level)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and not __fish_seen_subcommand_from start stop status help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l root -d 'The repository to watch and index' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l socket -d 'Where to listen, instead of the endpoint derived from the repository (ADR-004 §7.2)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l supervisor -d 'Who is supervising this daemon, recorded in the health document (`systemd`, `docker`, `ci`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l cpu-share-pct -d 'The CPU ceiling, as a percent of one core. 25 is ADR-004 §7.2\'s background default' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l slice -d 'Analysis units per pass while nothing is talking to the daemon' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l yield-slice -d 'Analysis units per pass while a foreground client has just been heard — the yield slice' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l queue-depth -d 'How many distinct index requests may wait before new ones are refused rather than queued' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l hook-bytes -d 'Harness-stream bytes per pass — the back-pressure knob of ADR-005 §6.5' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l backend -d 'Which change source to use: `auto`, `notify` or `poll`. The daemon is a watcher, so it takes the watcher\'s flags for the same meaning' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l interval-ms -d 'The debounced polling interval in milliseconds' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l debounce-ms -d 'The quiescence window in milliseconds' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l heartbeat-ms -d 'The liveness beat in milliseconds, for the heartbeat file and the lease' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l deadline -d 'Stop after this many seconds — the supervisor\'s `TimeoutStopSec` seam and a test\'s' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l max-cycles -d 'Stop after this many loop passes. A test seam, and the way to use the daemon as a one-shot indexer' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l json -d 'Print the health document as JSON instead of lines, and print it once the first pass has run'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l quiet -d 'Print nothing but the health document; the heartbeat file is still written'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l foreground -d 'Run in this process, so a supervisor can supervise it (`e6.3`, ADR-004 §7.1)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l detach -d 'Detach into a new session (`setsid`), so a terminal\'s Ctrl-C cannot reach the daemon later'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l require-socket -d 'Refuse to start rather than run with no IPC endpoint'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l worktree -d 'Also re-index uncommitted paths when only git\'s index moved'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l no-status-file -d 'Do not write `.fathom/watch/status.json`'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from start" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l root -d 'The repository whose daemon to stop' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l socket -d 'Ask the daemon at this endpoint instead of the derived one' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l reason -d 'Who is asking, recorded in the daemon\'s last line' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l json -d 'Print the outcome as one JSON line (`fathom.cli.daemon.stop/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from stop" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l socket -d 'Ask the daemon at this endpoint instead of the derived one' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l json -d 'Print the report as one JSON line (machine contract `fathom.cli.status/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l index -d 'Ask for a pass and wait for it, then report the state after it — the way to make `fathom status` mean "up to date" rather than "as of whenever"'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from status" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "start" -d 'Take the writer lease and watch this repository from this process, or from a detached one'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "stop" -d 'Ask the daemon to finish its pass, checkpoint and release the lease'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "status" -d 'The daemon\'s own health report (`fathom status` is the same question at the top level)'
complete -c fathom -n "__fish_fathom_using_subcommand daemon; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l socket -d 'Ask the daemon at this endpoint instead of the derived one' -r
complete -c fathom -n "__fish_fathom_using_subcommand status" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand status" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand status" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand status" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand status" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand status" -l json -d 'Print the report as one JSON line (machine contract `fathom.cli.status/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l index -d 'Ask for a pass and wait for it, then report the state after it — the way to make `fathom status` mean "up to date" rather than "as of whenever"'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand status" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand status" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand status" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l show-keys -d 'Print the merged keymap — defaults plus your `keys.overrides` — and exit, taking no terminal'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l show-session -d 'Print the session state stored for this repository, and exit, taking no terminal (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l json -d 'With `--show-session`: the rows as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l stats -d 'Print the run\'s own frame numbers — draws, events, p99, worst, over-budget — as one JSON object on stdout when the shell gives the screen back (`reco-08r`)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l show-theme -d 'Print the theme this run would paint with, and exit, taking no terminal (`e5.6`)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l audit -d 'With `--show-theme`: treat a failed contrast or separation floor as an error exit (exit 7)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l list-themes -d 'Print every theme Fathom can see — the built-ins and the files in your theme directories — and exit, taking no terminal (`e5.6`)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand tui" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand open" -l path -d 'Which of the episode\'s files to open, by path or by a unique suffix of it' -r
complete -c fathom -n "__fish_fathom_using_subcommand open" -l line -d 'Which line, overriding the line the first `@@` header says' -r
complete -c fathom -n "__fish_fathom_using_subcommand open" -l editor -d 'Which editor to use for this run, overriding `$FATHOM_EDITOR`, `$VISUAL` and `$EDITOR`' -r
complete -c fathom -n "__fish_fathom_using_subcommand open" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand open" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand open" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand open" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand open" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand open" -l print -d 'Print the command and the reason for it, and run nothing'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l list -d 'List every file the episode touched with the line each would jump to, and open nothing'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l json -d 'Print the decision as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand open" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand open" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand open" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l path -d 'Which of the episode\'s files to show, by path or by a unique suffix of it' -r
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l tool -d 'Which difftool to use, overriding `$FATHOM_DIFFTOOL` and your user config' -r
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l print -d 'Print the command and the reason for it, and run nothing. The two files stay on disk'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l list -d 'List every file the episode touched with the line each would jump to, and show nothing'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l keep -d 'Leave the two staged files behind and say where they are'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l json -d 'Print the decision as JSON on stdout, and nothing else there'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand difftool" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l save -d 'Keep this comparison under a name, so the next person says `since-the-refactor` instead of typing the endpoints' -r
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l note -d 'Why the name exists, kept with it' -r
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l json -d 'Print the comparison as one JSON line on stdout (`fathom.compare/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l list -d 'List the saved comparisons and exit'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand cmp" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand search" -l limit -d 'How many hits to print' -r
complete -c fathom -n "__fish_fathom_using_subcommand search" -l save -d 'Keep this query under a name, so the `,` palette can offer it again' -r
complete -c fathom -n "__fish_fathom_using_subcommand search" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand search" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand search" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand search" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand search" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand search" -l json -d 'Print the report as one JSON line on stdout (`fathom.search/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l list -d 'List the saved searches and exit'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand search" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand search" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand search" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l revision -d 'Score this revision rather than the newest the graph holds: a full tree oid or an unambiguous prefix' -r
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l backfill -d 'Score this many unscored revisions first, newest first, before reporting (`0`: report what is stored)' -r
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l history -d 'How many scored revisions the trend reads; bare, or `0`, is every row the index holds' -r
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l json -d 'Print the report as one JSON line on stdout (`fathom-architecture/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l refresh -d 'Forget every stored scorecard first, and re-derive what this run needs'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l read-only -d 'Do not write anything: read the stored rows and score only what this run needs'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l explain -d 'Print the knobs in force — the weights, the curves and their published constants — and exit'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand architecture" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l reason -d 'Why the baseline is being raised — required by `--update-baseline`, recorded in the file, the diff and the review memory' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l owner -d 'Who owns the new allowances; bare, the reviewer\'s own identity (`--identity`, `$FATHOM_REVIEWER`, `git config user.name`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l granted-by -d 'Who is asking, recorded beside the owner as `granted_by`; bare, the same identity as `--owner`' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l expires -d 'The last day an allowance is live, `YYYY-MM-DD`, or `never` for a row with no expiry' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l key -d 'Grant only this key (repeatable): a violation key as the report prints it, or `kind=key`' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l revision -d 'Judge this graph revision rather than the newest the index holds: an oid or an unambiguous prefix' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l json -d 'Print the report as one JSON line on stdout (`fathom-ratchet/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l update-baseline -d 'Raise the baseline: write the violations that have no allowance as new rows'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l dry-run -d 'Report what would change and write nothing — not even the automatic tightening'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l no-note -d 'Do not write a `decision` note into the review memory when the baseline is raised'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand ratchet" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand context" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand context" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand context" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand context" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand context" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand context" -l json -d 'Print the answer as one JSON line on stdout (`fathom.cli.context/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l all -d 'Include answered and withdrawn notes in the lists, not just the live ones'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand context" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand context" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand context" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l root -d 'The repository to read' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l json -d 'Print the report as one JSON line on stdout (`fathom.cli.note/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -f -a "add" -d 'Record a note, decision, question or follow-up against an episode or a path'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -f -a "list" -d 'List what is recorded, oldest first'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -f -a "show" -d 'Show one note with its evidence and its link verdict'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -f -a "answer" -d 'Answer a question or close a follow-up by appending the note that closes it'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -f -a "withdraw" -d 'Take a recorded decision back, by appending the note that retracts it'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -f -a "verify" -d 'Re-hash every note against its own `digest:` line — tampering, and nothing else'
complete -c fathom -n "__fish_fathom_using_subcommand note; and not __fish_seen_subcommand_from add list show answer withdraw verify help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l episode -d 'The episode to attach it to, or an unambiguous prefix of its id' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l path -d 'The path to attach it to, optionally `path:line`. Resolved to the newest episode that touched it when the index knows one' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l kind -d 'What kind of note this is: `decision`, `question`, `follow-up`, `annotation` (`note` and `flag` are accepted as `annotation`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l author -d 'Who is writing, when git config and `fathom.toml` do not say' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l agent -d 'Record the note as an agent\'s act on someone\'s behalf, not as a human\'s' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l about -d 'The note this one answers, by `nt-…` id' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l json -d 'Print the report as one JSON line on stdout (`fathom.cli.note/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from add" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l kind -d 'Only this kind: `decision`, `question`, `follow-up`, `annotation`' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l status -d 'Only notes in this status: `open`, `answered`, `withdrawn`' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l limit -d 'How many to print, oldest first; `0` is every note' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l json -d 'Print the report as one JSON line on stdout (`fathom.cli.note/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from list" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l json -d 'Print the report as one JSON line on stdout (`fathom.cli.note/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from show" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l author -d 'Who is answering' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l agent -d 'Record the answer as an agent\'s act on someone\'s behalf' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l kind -d 'The kind of the answering note: a `decision` is an answer that settles it, an `annotation` is an answer that informs it' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l json -d 'Print the report as one JSON line on stdout (`fathom.cli.note/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from answer" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l author -d 'Who is retracting it' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l agent -d 'Record the retraction as an agent\'s act on someone\'s behalf' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l json -d 'Print the report as one JSON line on stdout (`fathom.cli.note/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from withdraw" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l json -d 'Print the report as one JSON line on stdout (`fathom.cli.note/1`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from verify" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from help" -f -a "add" -d 'Record a note, decision, question or follow-up against an episode or a path'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from help" -f -a "list" -d 'List what is recorded, oldest first'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from help" -f -a "show" -d 'Show one note with its evidence and its link verdict'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from help" -f -a "answer" -d 'Answer a question or close a follow-up by appending the note that closes it'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from help" -f -a "withdraw" -d 'Take a recorded decision back, by appending the note that retracts it'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from help" -f -a "verify" -d 'Re-hash every note against its own `digest:` line — tampering, and nothing else'
complete -c fathom -n "__fish_fathom_using_subcommand note; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -f -a "bash" -d 'The completion script for bash, to drop in `.../bash-completion/completions/fathom`'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -f -a "zsh" -d 'The completion script for zsh, to drop in a `$fpath` directory as `_fathom`'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -f -a "fish" -d 'The completion script for fish, to drop in `.../fish/vendor_completions.d/fathom.fish`'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -f -a "man" -d 'The man pages, rendered from the same grammar'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and not __fish_seen_subcommand_from bash zsh fish man help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from bash" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from zsh" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from fish" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l out -d 'Write them under `DIR` (as `DIR/man1/*.1`) instead of printing one page on stdout' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l page -d 'Just this page — `--page fathom-check` — instead of every verb' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from man" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from help" -f -a "bash" -d 'The completion script for bash, to drop in `.../bash-completion/completions/fathom`'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from help" -f -a "zsh" -d 'The completion script for zsh, to drop in a `$fpath` directory as `_fathom`'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from help" -f -a "fish" -d 'The completion script for fish, to drop in `.../fish/vendor_completions.d/fathom.fish`'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from help" -f -a "man" -d 'The man pages, rendered from the same grammar'
complete -c fathom -n "__fish_fathom_using_subcommand completions; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l root -d 'The repository to read the stream of' -r
complete -c fathom -n "__fish_fathom_using_subcommand events" -l tail -d 'Print only the last N events' -r
complete -c fathom -n "__fish_fathom_using_subcommand events" -l on -d 'Which kinds to print, repeating or comma-separated: `analysis.*`, `detector.finished`, `*`' -r
complete -c fathom -n "__fish_fathom_using_subcommand events" -l path -d 'Read this file instead of the repository\'s configured stream' -r
complete -c fathom -n "__fish_fathom_using_subcommand events" -l theme -d 'Paint the frame with this theme: a built-in name, a theme file, or a path to one (`e5.6`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand events" -l detectors -d 'Which detectors run, moved off the file\'s answer (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand events" -l enable-detector -d 'Run one more detector, by id — the long form of `--detectors +id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand events" -l disable-detector -d 'Do not run one detector, by id — the long form of `--detectors -id` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand events" -l events -d 'Write this run\'s analysis event stream to a file, or to stderr with `-` (`e6.7`)' -r
complete -c fathom -n "__fish_fathom_using_subcommand events" -l json -d 'Print the raw JSONL instead of one line per event'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l explain -d 'Print where the stream is, whether it is switched on, and what was refused — then leave'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l plain -d 'Draw as text, with no terminal taken: for pipes, CI logs and screen readers (ADR-006 §6.7)'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l no-alt-screen -d 'Draw in place instead of taking the alternate screen, which keeps the shell\'s scrollback'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l fresh -d 'Ignore the session this repository has stored, and start from defaults (`e1.11`)'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l mouse -d 'Take the mouse: click selects, double-click drills, the wheel scrolls (`e5.5`, ADR-006 §10.1)'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l lines -d 'With `--plain`: print the frame in reading order, one block per pane, instead of side by side'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l progress -d 'print the index\'s progress row even when stderr is not a terminal (`index` and `--demo`)'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l no-progress -d 'never print the index\'s progress row, even on a terminal'
complete -c fathom -n "__fish_fathom_using_subcommand events" -l no-watch -d 'Do not watch the repository from an open frame: show what the index holds, and say so (`reco-bpc`)'
complete -c fathom -n "__fish_fathom_using_subcommand events" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fathom -n "__fish_fathom_using_subcommand events" -s V -l version -d 'Print version'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "index" -d 'Index the repository so every other verb has something to read'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "show" -d 'Show one episode in full'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "episodes" -d 'List the episodes — the rail\'s rows, headless (`reco-11b`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "attribution" -d 'Report or correct who Fathom thinks wrote a commit (`e4.1`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "review" -d 'The review ledger: sign-offs that outlive the history they were written about (`e4.6`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "analyze" -d 'Print one episode\'s delta document — the model the TUI, the export and an agent all read'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "narrative" -d 'Read one episode: the narrative every claim in it cites its evidence from (`e4.7`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "drift" -d 'What the plan said against what the session did — the contradictions, both sides linked (`e4.8`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "why" -d 'Print one episode\'s risk score with every feature, its weight and its contribution (`e4.4`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "stats" -d 'Report repo scale, index state and cache hit rate'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "check" -d 'Run the checks CI cares about; findings above the threshold exit 7'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "doctor" -d 'Describe this build, its environment and its index, with a remedy per finding'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "export" -d 'Write a review artefact for a pull request or an audit'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "mcp" -d 'Speak MCP to a coding agent'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "watch" -d 'Watch the repository and index as it changes'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "ambient" -d 'The glance pane for the terminal beside the agent: latest episode, queue, index, one reason (`reco-8bx`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "hooks" -d 'Report on the harness event stream: lag, offset, rejections, replay (`e6.1`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "daemon" -d 'The one process per repository that watches, tails the harness stream and indexes (`e6.3`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "status" -d 'Who is keeping this index current, what it costs, and what is left (`e6.3`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "tui" -d 'Print the terminal UI\'s keymap, or open the UI itself'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "open" -d 'Open an episode\'s file at its line in `$EDITOR` — jump out, do not read around it (`e5.8`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "difftool" -d 'Show one file\'s two sides in your own difftool — difftastic, meld, kdiff3, `vim -d` (`e5.8`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "cmp" -d 'Compare any two episodes or revisions: the composed delta over a range (`e5.3`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "search" -d 'Search the whole index from one box, in the shell\'s own ranking (`e5.2`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "architecture" -d 'The architecture scorecard: five dimensions, one number, and the drift since your last review (`e3.9`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "ratchet" -d 'The ratchet: the committed allowance ledger, which tightens on its own and widens only when told to (`e3.10`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "context" -d 'What this repository knows about a path or an episode: the recorded decisions and the open questions (`e6.5`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "note" -d 'The review memory: notes, decisions, questions and follow-ups, committed under `.fathom/reviews/` (`e6.5`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "completions" -d 'Print the shell\'s own copy of this grammar: completion scripts and man pages (`e7.4`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "events" -d 'Read the analysis event stream a run wrote: what happened, in order (`e6.7`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and not __fish_seen_subcommand_from index show episodes attribution review analyze narrative drift why stats check doctor export mcp watch ambient hooks daemon status tui open difftool cmp search architecture ratchet context note completions events help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from attribution" -f -a "correct" -d 'Store a correction: these identities were written by an agent / by a person'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from attribution" -f -a "list" -d 'List the stored corrections, which are the keys the classifier honours'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from attribution" -f -a "forget" -d 'Forget one stored correction'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from review" -f -a "status" -d 'The verdict: is every episode covered by a sign-off that still matches (exit `7` when it is not)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from review" -f -a "sign" -d 'Sign an episode off, appending one entry to `.fathom/reviews/ledger.md`'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from review" -f -a "sync" -d 'Render the queue\'s acts that the ledger has never seen'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from review" -f -a "list" -d 'List the ledger\'s entries with their verdicts'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from review" -f -a "verify" -d 'Re-hash every entry against its own `digest:` line — tampering, and nothing else'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from mcp" -f -a "serve" -d 'Serve the tool surface over stdio, which opens no socket (`e6.4`)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from mcp" -f -a "tools" -d 'Print the tool surface and its bounds as this repository\'s config resolves them'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from mcp" -f -a "approval-token" -d 'Mint the token that authorises `mark_reviewed` for exactly one change'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from daemon" -f -a "start" -d 'Take the writer lease and watch this repository from this process, or from a detached one'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from daemon" -f -a "stop" -d 'Ask the daemon to finish its pass, checkpoint and release the lease'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from daemon" -f -a "status" -d 'The daemon\'s own health report (`fathom status` is the same question at the top level)'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from note" -f -a "add" -d 'Record a note, decision, question or follow-up against an episode or a path'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from note" -f -a "list" -d 'List what is recorded, oldest first'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from note" -f -a "show" -d 'Show one note with its evidence and its link verdict'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from note" -f -a "answer" -d 'Answer a question or close a follow-up by appending the note that closes it'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from note" -f -a "withdraw" -d 'Take a recorded decision back, by appending the note that retracts it'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from note" -f -a "verify" -d 'Re-hash every note against its own `digest:` line — tampering, and nothing else'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from completions" -f -a "bash" -d 'The completion script for bash, to drop in `.../bash-completion/completions/fathom`'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from completions" -f -a "zsh" -d 'The completion script for zsh, to drop in a `$fpath` directory as `_fathom`'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from completions" -f -a "fish" -d 'The completion script for fish, to drop in `.../fish/vendor_completions.d/fathom.fish`'
complete -c fathom -n "__fish_fathom_using_subcommand help; and __fish_seen_subcommand_from completions" -f -a "man" -d 'The man pages, rendered from the same grammar'
