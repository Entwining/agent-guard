# Unreleased

A credential variable piped into a Maskey command given `--stdin` is now permitted for every `mk` command except `mk run`, not only for `mk set --stdin`, so a command that a later Maskey version gives that flag passes without a guard update. The guard does not check that the command accepts the flag; one that does not exits with a usage error without reading its standard input.
