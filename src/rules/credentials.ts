// Credential rules: keep credential files, environment and shell-variable
// dumps, stored tokens and verbose HTTP traces out of what the model reads.
// Clients that consume a credential file themselves (dotenvx, node --env-file,
// ssh -i) are not readers and stay allowed.
import { basename } from "node:path";

import { absPath, isSensitive, isSensitiveRoot, sshPrivate, sshScopeDenied } from "../paths";
import { reasons } from "../reasons";
import type { Command, Request, Word } from "../record";

const readerPrograms = new Set(
  (
    "cat head tail less more bat sed awk jq yq base64 xxd od strings diff openssl plutil cp tee tar source . sort " +
    "uniq cut nl fold rev paste comm join iconv hexdump hd zcat gzcat bzcat xzcat ag ack"
  ).split(" "),
);
const {
  file: fileReason,
  dump: dumpReason,
  variable: varReason,
  token: tokenReason,
  keychain: keychainReason,
  trace: traceReason,
  upload: uploadReason,
  ssh: sshReason,
  grepSsh: grepSshReason,
  hiddenSearch: hiddenSearchReason,
} = reasons;

const secretName = (name: string) => /TOKEN|SECRET|KEY|PASSWORD|CREDENTIAL/.test(name.toUpperCase());

export function credentialRules(req: Request): string[] {
  const denials: string[] = [];
  if (req.operation === "read" && isSensitive(req.target)) denials.push(fileReason);
  if (req.operation === "write" && sshPrivate(req.target)) denials.push(sshReason);
  if (req.operation === "search") {
    if (isSensitiveRoot(req.searchRoot)) denials.push(fileReason);
    // A positive glob selects files even past ignore rules.
    if (req.glob && !req.glob.startsWith("!") && isSensitive(`${req.searchRoot}/${basename(req.glob)}`, true)) denials.push(fileReason);
  }
  for (const cmd of req.commands) denials.push(...credentialCommand(cmd, req.home));
  for (const fragment of req.uninspectable) denials.push(...credentialSignatures(fragment));
  return denials;
}

// The core runs this last: it follows symlinks, so it touches the filesystem.
export function credentialFilesystemRules(req: Request): string[] {
  if ((req.operation === "read" || req.operation === "write") && sshScopeDenied(req.target, req.home, false)) return [sshReason];
  if (req.operation === "search" && sshScopeDenied(req.searchRoot, req.home, true)) return [grepSshReason];
  for (const cmd of req.commands) {
    for (const redirect of cmd.redirects) {
      if ((redirect.direction === "in" || redirect.direction === "out") && redirect.target && sshScopeDenied(absPath(redirect.target, cmd.cwd, req.home), req.home, false)) return [sshReason];
    }
    if (cmd.program < 0) continue;
    const name = basename(cmd.argv[cmd.program]!.text);
    const readsPath = ["ls", "find", "tree", "rg", "grep", "ag", "ack", "fd"].includes(name);
    if (!readsPath && !readerPrograms.has(name)) continue;
    const words = cmd.argv.slice(cmd.program + 1);
    // A search with no path operand reads the working directory.
    const searches = ["rg", "ag", "ack"].includes(name) || (name === "grep" && cmd.flags.has("recursive"));
    if (searches && !cmd.flags.has("help") && !words.some((word) => word.role === "path") && sshScopeDenied(cmd.cwd, req.home, true)) return [grepSshReason];
    const filter = name === "jq" ? jqFilter(words) : undefined;
    for (const word of words) {
      if (word === filter) continue;
      if (!["arg", "path", "patfile", "option:patfile"].includes(word.role) || word.value.startsWith("-")) continue;
      if (sshScopeDenied(absPath(word.value, cmd.cwd, req.home, /^['"]/.test(word.raw)), req.home, false)) return [sshReason];
    }
  }
  return [];
}

function credentialCommand(cmd: Command, home: string): string[] {
  const denials: string[] = [];
  const sensitive = (word: string, glob = false, quoted = false) => isSensitive(absPath(word, cmd.cwd, home, quoted), glob);
  for (const r of cmd.redirects) if (r.direction === "in" && sensitive(r.target)) denials.push(fileReason);
  if (cmd.program < 0) {
    if (cmd.wrappers.at(-1) === "env" && !cmd.wrappers.includes("env-S")) denials.push(dumpReason);
    return denials;
  }
  const program = cmd.argv[cmd.program]!.text;
  const name = basename(program);
  const words = cmd.argv.slice(cmd.program + 1);
  const args = words.map((w) => w.text);
  // Shell variables exist only in the shell, so a path or a wrapper that
  // execs a program never reaches these builtins.
  if (cmd.shell && !program.includes("/")) {
    if (name === "export" && (!args.length || args.some((a) => /^-[^-]*p/.test(a)))) denials.push(dumpReason);
    if (name === "set" && !args.length) denials.push(dumpReason);
    if (name === "typeset" || name === "declare") {
      if (!args.length || (args.length === 1 && /^-[^-]*[px]/.test(args[0]!))) denials.push(dumpReason);
      for (const a of args) if (!a.startsWith("-") && !a.includes("=") && secretName(a)) denials.push(varReason);
    }
  }
  let display = readerPrograms.has(name) || ["echo", "printf", "print"].includes(name);
  const operands = args.filter((a) => !a.startsWith("-"));
  switch (name) {
    case "printenv":
      if (!operands.length) denials.push(dumpReason);
      for (const a of operands) if (secretName(a)) denials.push(varReason);
      break;
    case "echo":
    case "printf":
    case "print":
      if (name !== "echo" && args[0] === "-v") display = false;
      if (display && words.some((w) => w.vars.some(secretName))) denials.push(varReason);
      break;
    case "rg":
    case "grep":
    case "ag":
    case "ack":
      if (cmd.flags.has("help")) break;
      if ((name === "grep" && cmd.flags.has("recursive")) || (["rg", "ag"].includes(name) && cmd.flags.has("hidden"))) denials.push(hiddenSearchReason);
      for (const w of words) {
        if (w.role === "path" && absPath(w.value, cmd.cwd, home, /^['"]/.test(w.raw)) === `${home}/.ssh`) denials.push(sshReason);
      }
      if (cmd.flags.has("files")) break;
      // Search operands, pattern files and positive globs are read targets;
      // the pattern is not.
      for (const w of words) {
        const quoted = /^['"]/.test(w.raw);
        if (["path", "patfile", "option:patfile"].includes(w.role) && sensitive(w.value, w.globs, quoted)) denials.push(fileReason);
        if (w.role === "path" && isSensitiveRoot(absPath(w.value, cmd.cwd, home, quoted))) denials.push(fileReason);
        if (["glob", "option:glob"].includes(w.role) && sensitive(basename(w.value), true)) denials.push(fileReason);
      }
      break;
    case "gh":
      if (args[0] === "auth" && args[1] === "token") denials.push(tokenReason);
      if (args[0] === "auth" && args[1] === "status" && ghShowsToken(args.slice(2))) denials.push(tokenReason);
      break;
    case "glab":
      if (args[0] === "auth" && args[1] === "status" && args.some((a) => /^(--show-token|-[^-]*t)/.test(a))) denials.push(tokenReason);
      break;
    case "security":
      if (args.includes("-w") || args.includes("-g")) denials.push(keychainReason);
      break;
    case "curl":
      denials.push(...curlRules(args, sensitive));
      break;
    default:
      if (!readerPrograms.has(name)) return denials;
      const filter = name === "jq" ? jqFilter(words) : undefined;
      for (const w of words) {
        if (w === filter) continue;
        const target = /^-.*=/s.test(w.text) ? w.text.slice(w.text.indexOf("=") + 1) : w.text;
        if (target && !target.startsWith("-") && sensitive(target, w.globs, /^['"]/.test(w.raw))) denials.push(fileReason);
      }
  }
  // A heredoc or here-string body is standard input the command prints.
  if (display && cmd.redirects.some((r) => r.vars.some(secretName))) denials.push(varReason);
  return denials;
}

function jqFilter(words: Word[]): Word | undefined {
  if (words.some((word) => word.text === "-f" || word.text === "--from-file")) return undefined;
  return words.find((word) => !word.text.startsWith("-"));
}

// A token-display flag counts even when a later flag cancels it.
function ghShowsToken(args: string[]): boolean {
  for (let i = 0; i < args.length; i++) {
    const a = args[i]!;
    if (a === "--") return false;
    if (["--hostname", "--jq", "--json", "--template", "-h"].includes(a)) i++;
    else if (a === "--show-token" || a.startsWith("--show-token=") || /^-[at]*t/.test(a)) return true;
  }
  return false;
}

// curl reads ~/.curlrc and may add credentials the command line never shows,
// so any trace is denied; @file data and uploads name files it sends.
function curlRules(args: string[], sensitive: (word: string) => boolean): string[] {
  const denials: string[] = [];
  const long = /^--(data|data-ascii|data-binary|data-urlencode|json|form|header|upload-file|config)(=|$)/s;
  for (let i = 0; i < args.length; i++) {
    const word = args[i]!;
    let key = "";
    let value = "";
    if (word === "--") break;
    if (/^file:\/\//i.test(word) && sensitive(word)) denials.push(fileReason);
    if (/^--(verbose|trace|trace-ascii)(=|$)/.test(word)) denials.push(traceReason);
    else if (long.test(word)) {
      key = word.slice(2).split("=")[0]!;
      value = word.includes("=") ? word.slice(word.indexOf("=") + 1) : (args[++i] ?? "");
    } else if (/^-[^-]/.test(word)) {
      for (let k = 1; k < word.length; k++) {
        if (word[k] === "v") denials.push(traceReason);
        if (!"AbcCdDeEFHKmoPQrTtuUwxXyYz".includes(word[k]!)) continue;
        key = word[k]!;
        value = word.slice(k + 1) || (args[++i] ?? "");
        break;
      }
    }
    if (key === "d" || key.startsWith("data") || key === "json" || key === "H" || key === "header") {
      if (!/@./s.test(value)) continue;
      value = value.slice(value.indexOf("@") + 1);
    } else if (key === "F" || key === "form") {
      value = value
        .slice(value.indexOf("=") + 1)
        .replace(/^[@<]/, "")
        .split(";")[0]!;
    } else if (!["T", "upload-file", "K", "config"].includes(key)) continue;
    if (sensitive(value)) denials.push(uploadReason);
  }
  return denials;
}

const readers = [...readerPrograms].filter((p) => p !== ".").join("|");
const names = "\\.env(?:\\.[A-Za-z0-9_.-]+)?|\\.npmrc|\\.zsh_history|\\.zprofile|private-keys-v1\\.d|\\S*\\.(pem|key)|auth\\.json|\\.credentials\\.json|\\.aws/credentials|\\.ssh/\\S*";
const readerSignature = new RegExp(`(^|[^A-Za-z0-9_])(${readers})([^A-Za-z0-9_-].*)?[\\s/"'=](${names})($|[\\s/"'>|&)\`])`, "s");

// Text the front end could not structure: interpreter code, case bodies, or
// source the parser rejects. Match the commands that would expose a credential.
function credentialSignatures(fragment: string): string[] {
  const denials: string[] = [];
  for (const segment of fragment.replace(/&&|\|\||\n/g, ";").split(";")) {
    if (
      /(^|[^A-Za-z0-9_-])printenv([^A-Za-z0-9_-]|$)/.test(segment) ||
      /(^|[|(])\s*(env|export|set|typeset|declare)\s*($|[|>)])/.test(segment) ||
      /(declare|typeset|export)\s+-[a-z]*[px]/.test(segment)
    )
      denials.push(dumpReason);
    const reader = segment.match(readerSignature);
    if (reader) denials.push(fileReason);
    if (/curl\s.*\s(-[A-Za-z]*v[A-Za-z]*|--verbose|--trace(-ascii)?)(\s|=|$)/s.test(segment)) denials.push(traceReason);
    if (/gh\s+auth\s+token/.test(segment)) denials.push(tokenReason);
    if (/security\s.*\s-[wg](\s|$)/s.test(segment)) denials.push(keychainReason);
    if (/(ECHO|PRINTF|PRINT)\s.*\$\{?[A-Z0-9_]*(TOKEN|SECRET|KEY|PASSWORD|CREDENTIAL)/s.test(segment.toUpperCase())) denials.push(varReason);
  }
  return denials;
}
