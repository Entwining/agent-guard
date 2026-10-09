use crate::{CoverageGap, DenialRule};

// Public sentences carry the safe alternatives; diagnostic effect text
// stays internal and must not replace them in consumer output.
const APPDATA: &str = "This reads a protected macOS app-data directory. Name a specific non-sensitive file under ~/Library/Application Support instead, or ask the user to inspect the protected file and share the needed fact.";
const BROAD: &str = "A scan rooted at the home directory or ~/Library reaches every app-data entry. Scope the scan to a project path.";
const FILE: &str = "This reads a credential or environment file. If a client the guard models only needs to use the file, pass it through that program's own option, such as `--env-file`, `--kubeconfig`, or `ssh -i`; the guard does not control what the client does with the contents. Otherwise read a non-sensitive config file, or ask the user to inspect the file and share only the fact needed.";
const CODE_FILE: &str = "This inline code names a credential or environment file. To write text that mentions the file, use the Write or Edit tool; to run code that needs its values, pass the file through a modelled runtime's option, such as `node --env-file=.env`, though the guard does not control what the runtime does with the contents; otherwise ask the user to inspect the file and share only the fact needed.";
const HIDDEN_SEARCH: &str = "A recursive search that includes hidden files can read credentials. Use default rg on a project path, or search an exact non-sensitive file without recursive or hidden-file flags.";
const DUMP: &str = "This dumps environment or shell variables, including secrets. Name the non-sensitive variable needed and read only that variable.";
const VARIABLE: &str = "This prints the value of a credential variable. Ask the user for the specific non-sensitive fact needed, or let the authorized client consume the credential without printing it.";
pub(super) const TOKEN: &str = "This prints a Git hosting token. Use auth status without token-display flags; if authentication needs repair, ask the user to update the credential in their terminal.";
const KEYCHAIN: &str = "This extracts a password from the macOS Keychain. State the intended use and run the authorized client that consumes it without printing it.";
pub(super) const SECRET_PRINT: &str = "This prints a stored secret or access token. Run the command that uses the credential without printing it, or ask the user to run it in their own terminal and share only the non-secret fact needed.";
const TRACE: &str = "curl verbose or trace output can print HTTP headers including Authorization. Drop -v and --trace; use a normal curl request for the needed result.";
const UPLOAD: &str = "This sends the contents of a credential file. Send only the required non-sensitive fields explicitly, and let the client obtain authentication from its normal credential source.";
const SSH: &str = "This reads private material in the named .ssh directory or its filesystem alias. Search public material in the project or request the exact public key or client-config path; ask the user to inspect private material locally if a specific non-sensitive fact is needed.";
const GREP_SSH: &str = "Grep would search private material in the named .ssh directory or its filesystem alias. Narrow the search to a project directory or an exact public key, client config, allowed_signers, or known_hosts file.";

impl DenialRule {
    pub fn message(self) -> &'static str {
        match self {
            Self::ResourceChange => {
                "This relocates protected credential, environment or SSH content to an unprotected name. Keep both names protected, use a non-sensitive source, or ask the user to perform the change in their own terminal and share only the non-sensitive result."
            }
            Self::AppData => APPDATA,
            Self::Broad => BROAD,
            Self::File => FILE,
            Self::CodeFile => CODE_FILE,
            Self::HiddenSearch => HIDDEN_SEARCH,
            Self::Dump => DUMP,
            Self::Variable => VARIABLE,
            Self::Token => TOKEN,
            Self::Keychain => KEYCHAIN,
            Self::StoredSecret => SECRET_PRINT,
            Self::Trace => TRACE,
            Self::Upload => UPLOAD,
            Self::Ssh => SSH,
            Self::GrepSsh => GREP_SSH,
        }
    }
}

pub(crate) fn refusal_message(cause: &CoverageGap) -> &'static str {
    match cause {
        CoverageGap::InodeAlias => {
            "The guard cannot inspect an inode-addressed /.vol path. Name the file by its ordinary file path, then recheck the call."
        }
        CoverageGap::InspectionBudget => {
            "This command exceeds the guard's inspection budget. Split loops, function calls or brace alternatives into smaller commands with explicit public paths, then recheck each command."
        }
        CoverageGap::IdentityBound => {
            "The guard cannot resolve this path through bounded or cyclic aliases. Name the file by an ordinary absolute path without the cyclic or deep alias chain, then recheck the call."
        }
        CoverageGap::InputByteLimit => {
            "This event exceeds the input byte limit. Split the call into smaller requests, then recheck each request."
        }
        CoverageGap::NestingLimit => {
            "Shell nesting exceeds the supported depth of 64. Shorten the nesting or split the command, then recheck each command."
        }
        CoverageGap::AbsoluteCwdRequired => {
            "The event requires an absolute cwd. Use an absolute cwd, then recheck the call."
        }
        CoverageGap::InvalidEncoding => {
            "The event is not valid UTF-8. Encode the request as UTF-8, then recheck the call."
        }
        CoverageGap::UnsupportedShellSyntax => {
            "The agent guard cannot inspect this shell syntax. Rewrite it as a Bash-compatible command with explicit paths, or run a narrower command that the guard can inspect."
        }
        CoverageGap::ExecutionOwnerUnavailable => {
            "The agent guard cannot verify the execution owner for this operation. Have the owner establish and verify the execution boundary, then recheck the call."
        }
        _ => {
            "The agent guard cannot inspect this unsupported or unresolved operation. Replace the unsupported construct with a Bash-compatible command naming an explicit public path, then recheck it."
        }
    }
}
