# Offline Rust slice limits

The trial accepts at most 262,144 bytes in a serialized event and 64 nested
shell substitution/group delimiters. The native checker completes a refusal
for excess input, excess nesting, a relative cwd or invalid UTF-8: checker
status 2 and runner status 3. Its reason tells the caller to split or shorten
the request, supply an absolute cwd or encode UTF-8. Filesystem probe and
operational failures retain the separate failed-check contract.

The byte cap was re-derived from release checker measurements in Batch 13.
Public literal arguments, data-heavy command substitutions used as Git
pathspecs, and repeated statements were measured from 64 KiB through 8 MiB.
The substitution shape took 0.318 s at 256 KiB, 0.631 s at 512 KiB and 1.286 s
at 1 MiB. 256 KiB was the largest measured size below 0.5 s (one fifth of the
2.5 s checker deadline); the serialized cap is slightly smaller than its
262,286-byte measurement envelope. The first cold literal launch took 0.988 s
and remains in the raw results. This finite sweep bounds the measured shapes,
not every possible input or host load. Raw results are in
`~/.cache/guard-fixtures/fixture-seat/batch13/cap-calibration.json`.

The original P1 65,536-byte cap came from parser/padding calibration rather
than full evaluation cost. The unchanged depth bound concerns simultaneous
nesting, not statement count or list position. Tests cover both input sides,
the 64/65 depth frontier, deadline polling, and native refusal status/reasons.

## Reproducers for retained limits

These are command texts for checker fixtures with a synthetic HOME and public
fixture files; they are not instructions to read real protected material.
Go remains authoritative for installed hooks until a separately accepted cutover.

| Boundary | Reproducer and observed scope |
| --- | --- |
| Shared dynamic item sources | `cat $(echo .env)`; `for f in $(printf '%s\n' .env); do cat "$f"; done`; `env $(echo cat .env)`; `bash -c "$(echo cat .env)"` are shared Go gaps. |
| Alias and sourced stdin | `alias c='cat .env'; c`; `source /dev/stdin <<< 'cat .env'` are shared gaps. |
| Filesystem mutations before reads | `ln -s .env public-link; cat public-link` does not update the preflight identity model. |
| Shell option changes | `shopt -s dotglob; cat *` is a shared gap; `setopt globdots; cat *` remains refused as unsupported executor syntax. |
| Embedded program languages | `sed 'r .env' public`; `sed 'e cat .env' public`; `awk 'BEGIN { getline < ".env" }'`; `awk 'BEGIN { system("cat .env") }'`; `vim -c 'read .env'`; `tmux new 'cat .env'`; `watch 'cat .env'` are shared gaps. |
| Client-defined command strings | `GIT_SSH_COMMAND='cat .env' git fetch`; `git -c core.pager='cat .env' log`; `git -c alias.x='!cat .env' x`; `ssh -o ProxyCommand='cat .env' host` remain shared gaps. |
| OS service reads | `defaults export com.example.app -` is a shared gap. |
| Dynamic eval | `eval "val=\$$v"` remains an unsupported dynamic rewrite. Runtime-unknown command output is a separate unresolved-code state. |
| Executor divergence | `a=(public)#` retains Bash/Zsh divergence refusal. |
| Inline mentions | `python3 -c "print('~/.ssh/id_rsa')"` is refused by both checkers although the spelling is data. |
| Redacted workdir | Codex `shell_command`/`shell` with `workdir: "__REDACTED__"` retain the relative-cwd boundary; the native checker now explains it. |
| Grep on HOME | `Grep` with path `~` retains a Go/Rust rule/reason difference. |
| Fresh temporary trees | `d=$(mktemp -d); cp public "$d/file"` keeps D74 Go parity and runtime uncertainty. |
