# Unreleased

The guard now denies a call only when it recognizes a protected target together with an operation that exposes it, such as a read, a listing, a walk or a move, or a command that prints a secret. A program it does not know, shell syntax it does not model and a target it cannot resolve no longer block the call: the guard checks the rest of the command and leaves those parts to [operating system read restrictions](https://github.com/Entwining/agent-guard/blob/main/docs/setup.md#operating-system-read-restrictions). Ordinary commands are refused far less often as a result. A check that fails, receives a malformed event or runs out of time still blocks the call.

This narrows what the guard catches. These calls were refused in v0.8.18 and are now permitted; if you relied on the guard for them, add the paths to your runtime's read restrictions:

- arguments of a program outside the guard's program table, which are now names rather than files, as in `python3 script.py .env` and `chezmoi cat ~/.aws/credentials`, and the operands of `cargo run` and words after its `--`;
- interpreter source given with `-c` or `-e`, in a here-document or through a pipe, which is no longer scanned for file names or secret output, so `python3 -c "open('.env').read()"` passes; a Perl one-liner that runs its arguments as a command, such as `perl -e 'exec @ARGV' cat .env`, is still checked as that command;
- shell options that change expansion, such as `setopt globdots`, dynamic rewrites such as `eval "val=\$$v"`, words that Bash and Zsh expand differently, inode-addressed `/.vol` paths and paths reached only through bounded or cyclic aliases;
- commands that exceed the inspection budget: a loop that has not settled stops being repeated, and the statements after it are still checked.

Recursive searches of a project are permitted, including `grep -r`, `rg --hidden`, `grep -r --include='*.json'` and `find . -type f -exec cat {} +`, so one can read an untracked `.env` in the project. `hidden_search` now refuses only a recursive search that walks a credential directory, such as `find ~/.gnupg -name '*.ts' -exec cat {} +`, and its advice suggests listing the directory with `ls` or reading a non-secret file in it by name. A walk whose name filter names a credential or environment file, such as `grep -r --include='*.pem'`, is still refused.

One named file in another app's container can be read, written, changed or inspected, as in `cat ~/Library/Containers/com.example/Data/notes.txt`, which raises at most one permission prompt for that app. Listings, walks and wildcards inside a container, and container paths the shell computes at run time, are still refused, as are `ls`, `rg`, `grep -r`, `zgrep`, `diff` and editors such as `vim` on a single container path and shell commands whose working directory is inside a container; use `ls -ld` or `grep` without `-r` there. A key or environment file inside a container, such as `.env` or `server.pem`, is refused as a credential read, and its denial now reports `credential_file` rather than `app_data`.

Listing an `.ssh` directory and reading file metadata in it, as in `ls -la ~/.ssh`, are permitted. Reading or searching its private material is still refused.

`curl -v` and `--trace` are refused only when the command line passes a credential input, such as an `Authorization` header, a cookie or `-u user:pass`; `curl -v https://example.com` is permitted. `env | cut -d= -f1` and other environment dumps are still refused, and the advice now suggests `compgen -e` in Bash to list variable names without values.

Each denial now ends with the path or command that triggered it, as in ``Triggered by `~/.aws/credentials`.``

The reason IDs `inline_credential_file`, `unsupported_shell_syntax`, `executor_divergence`, `identity_bound`, `inode_alias` and `inspection_budget` are retired, because the calls they refused now pass. Update any tooling that matches these IDs.

The [known limits](https://github.com/Entwining/agent-guard/blob/main/docs/setup.md#known-limits) list what the guard leaves unchecked.
