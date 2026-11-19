# Unreleased

Interpreter code no longer refuses identifiers that look like protected names. v0.8.17 refused `python3 -c`, `node -e` and here-document source that builds a command with `.env("A", "1")`, or that holds a JavaScript template such as `` `${at()} keydown ${e.key}` ``. Source is now read in the interpreter's language: a name with a slash counts anywhere, while a name without one counts only inside a string or a comment, and not when it is called as `name(`. `open(".env")`, `os.system("cat .env")`, `f"{name}.pem"`, a comment such as `# cat .env`, and a protected path in prose such as `"no ~/.claude here"` are still refused.

Interpreter source piped into the interpreter is now checked. v0.8.17 refused `python3 - <<'EOF'` with `open('.env')` in it but allowed the same here-document as `cat <<'EOF' | python3`; both are now refused, as is `printenv` run from piped source. This applies when the interpreter takes its program from standard input, with no script, module or `-c`/`-e` code; text piped into `python3 script.py` or `python3 -m json.tool` is still that program's data.

`gh search` and `gh api` arguments that only describe what GitHub searches or returns are no longer read as files: `gh search code --filename .env.local`, `gh search commits .npmrc`, `gh api repos/OWNER/REPO/contents/.env` and `gh api -f 'note=@.env'` are allowed. A file that `gh api` sends with `--input FILE` or `-F key=@FILE` is still refused, now with the `credential_upload` reason. Other `gh` commands, and `gh search` or `gh api` with an option they do not document, are checked as before.

`mkcert` host names such as `server.key` are allowed. Its `-cert-file`, `-key-file` and `-p12-file` values are checked as files it writes and `-csr` as a file it reads, and options after the first host name are host names, as mkcert reads them.

Writing into an `.ssh` directory, as in `mkcert -key-file ~/.ssh/id_rsa`, `echo > ~/.ssh/id_rsa` or `cp -t ~/.ssh notes.txt`, is still refused, now with the new reason ID `ssh_private_write` instead of `ssh_private` or `credential_file`, and the refusal suggests writing the file under a project or temporary directory and asking the user to install it. Update any tooling that matches the old IDs for these writes.

The [known limits](https://github.com/Entwining/agent-guard/blob/main/docs/setup.md#known-limits) list the string forms the interpreter scan does not read.
