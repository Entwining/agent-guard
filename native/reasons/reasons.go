package reasons

const (
	Syntax       = "The agent guard cannot inspect this shell syntax. Rewrite it as a Bash-compatible command with explicit paths, or run a narrower command that the guard can inspect."
	Appdata      = "This reads a protected macOS app-data directory. Name a specific non-sensitive file under ~/Library/Application Support instead, or ask the user to inspect the protected file and share the needed fact."
	Broad        = "A scan rooted at the home directory or ~/Library reaches every app-data entry. Scope the scan to a project path."
	File         = "This reads a credential or environment file. If a client the guard models only needs to use the file, pass it through that program's own option, such as `--env-file`, `--kubeconfig`, or `ssh -i`; the guard does not control what the client does with the contents. Otherwise read a non-sensitive config file, or ask the user to inspect the file and share only the fact needed."
	CodeFile     = "This inline code names a credential or environment file. To write text that mentions the file, use the Write or Edit tool; to run code that needs its values, pass the file through a modelled runtime's option, such as `node --env-file=.env`, though the guard does not control what the runtime does with the contents; otherwise ask the user to inspect the file and share only the fact needed."
	HiddenSearch = "A recursive search that includes hidden files can read credentials. Use default rg on a project path, or search an exact non-sensitive file without recursive or hidden-file flags."
	Dump         = "This dumps environment or shell variables, including secrets. Name the non-sensitive variable needed and read only that variable."
	Variable     = "This prints the value of a credential variable. Ask the user for the specific non-sensitive fact needed, or let the authorized client consume the credential without printing it."
	Token        = "This prints a Git hosting token. Use auth status without token-display flags; if authentication needs repair, ask the user to update the credential in their terminal."
	Keychain     = "This extracts a password from the macOS Keychain. State the intended use and run the authorized client that consumes it without printing it."
	SecretPrint  = "This prints a stored secret or access token. Run the command that uses the credential without printing it, or ask the user to run it in their own terminal and share only the non-secret fact needed."
	Trace        = "curl verbose or trace output can print HTTP headers including Authorization. Drop -v and --trace; use a normal curl request for the needed result."
	Upload       = "This sends the contents of a credential file. Send only the required non-sensitive fields explicitly, and let the client obtain authentication from its normal credential source."
	Ssh          = "This reads private material under ~/.ssh. Search public material in the project or request the exact public key or client-config path; ask the user to inspect private material locally if a specific non-sensitive fact is needed."
	GrepSsh      = "Grep would search private ~/.ssh material. Narrow the search to a project directory or an exact public key, client config, allowed_signers, or known_hosts file."
	Symlink      = "The agent guard could not complete its symlink check. Name the direct non-sensitive file outside protected trees, or ask the user to inspect the target locally."
	Replace      = "rg -r means --replace. Drop -r; use -n for line numbers, or spell --replace VALUE for an intentional replacement."
	Include      = "rg has no --include flag. Filter files with -g GLOB (for example -g '*.ts') or a type filter such as -t ts."
	Bre          = "rg regex is not grep BRE: a\\|b matches a literal pipe. Write alternation as a|b; for a literal pipe, use [|] or -F."
)

var All = map[string]string{
	"syntax":       Syntax,
	"appdata":      Appdata,
	"broad":        Broad,
	"file":         File,
	"codeFile":     CodeFile,
	"hiddenSearch": HiddenSearch,
	"dump":         Dump,
	"variable":     Variable,
	"token":        Token,
	"keychain":     Keychain,
	"secretPrint":  SecretPrint,
	"trace":        Trace,
	"upload":       Upload,
	"ssh":          Ssh,
	"grepSsh":      GrepSsh,
	"symlink":      Symlink,
	"replace":      Replace,
	"include":      Include,
	"bre":          Bre,
}
