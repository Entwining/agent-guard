import { reasons } from "../reasons";
import type { Command } from "../record";
// Secrets that print without a path: environment and variable dumps, stored
// tokens, verbose HTTP traces. No target exists to check, so these name the
// command that prints them. Each is a workaround that stays until the operating
// system's read restrictions cover the store; do not extend the list.
import { programName } from "../shell/argv";
import { curlValueLetters } from "../targets/curl";
import { readers } from "../targets/programs";

const { dump: dumpReason, variable: varReason, token: tokenReason, keychain: keychainReason, trace: traceReason, secretPrint: secretReason } = reasons;

const secretName = (name: string) => /TOKEN|SECRET|KEY|PASSWORD|CREDENTIAL/.test(name.toUpperCase());

export function secretReasons(cmd: Command): string[] {
  const denials: string[] = [];
  if (cmd.program < 0) {
    if (cmd.wrappers.at(-1) === "env" && !cmd.wrappers.includes("env-S")) denials.push(dumpReason);
    return denials;
  }
  const program = cmd.argv[cmd.program]!.text;
  const name = programName(program);
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
  let display = readers.includes(name) || ["echo", "printf", "print"].includes(name);
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
    case "gh":
      if (args[0] === "auth" && args[1] === "token") denials.push(tokenReason);
      if (args[0] === "auth" && args[1] === "status" && ghShowsToken(args.slice(2))) denials.push(tokenReason);
      break;
    case "glab":
      if (args[0] === "auth" && args[1] === "status" && args.some((a) => /^(--show-token|-[^-]*t)/.test(a))) denials.push(tokenReason);
      break;
    case "security":
      if (args.some((a) => /^-[A-Za-z]*[wg]/.test(a) && !a.startsWith("--")) || ["dump-keychain", "export"].includes(args[0] ?? "")) denials.push(keychainReason);
      break;
    case "gcloud":
      if (args.includes("print-access-token") || args.includes("print-identity-token")) denials.push(secretReason);
      break;
    case "az":
      if (args[0] === "account" && args[1] === "get-access-token") denials.push(secretReason);
      break;
    case "aws":
      if (args[0] === "configure" && args[1] === "get" && secretName(args[2] ?? "")) denials.push(secretReason);
      break;
    case "npm":
      if (args[0] === "config" && args[1] === "get" && /auth|token|password/i.test(args[2] ?? "")) denials.push(secretReason);
      break;
    case "kubectl":
      if (args[0] === "config" && args[1] === "view" && args.includes("--raw")) denials.push(secretReason);
      break;
    case "gpg":
      if (args.some((a) => /^--export-secret-(sub)?keys$/.test(a))) denials.push(secretReason);
      break;
    case "curl":
      if (curlTraces(args)) denials.push(traceReason);
      break;
    case "git":
      if (gitCredentialFill(args)) denials.push(secretReason);
      break;
  }
  // A heredoc or here-string body is standard input the command prints.
  if (display && cmd.redirects.some((r) => r.vars.some(secretName))) denials.push(varReason);
  return denials;
}

// git credential fill prints the stored credential; the subcommand follows any global options.
function gitCredentialFill(args: string[]): boolean {
  let i = 0;
  while (i < args.length && args[i]!.startsWith("-")) i += ["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path"].includes(args[i]!) ? 2 : 1;
  return args[i] === "credential" && args[i + 1] === "fill";
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
// so any trace is denied. A value letter takes the rest of its cluster, so the
// v in -uvictor:secret is part of a user name.
function curlTraces(args: string[]): boolean {
  for (let i = 0; i < args.length; i++) {
    const word = args[i]!;
    if (word === "--") break;
    if (/^--(verbose|trace|trace-ascii)(=|$)/.test(word)) return true;
    // A separate value is not an option.
    if (/^--(data|data-ascii|data-binary|data-urlencode|json|form|header|upload-file|config)$/.test(word)) i++;
    if (!/^-[^-]/.test(word)) continue;
    for (let k = 1; k < word.length; k++) {
      if (word[k] === "v") return true;
      if (!curlValueLetters.includes(word[k]!)) continue;
      if (k === word.length - 1) i++;
      break;
    }
  }
  return false;
}

// Text the front end could not structure: interpreter code, case bodies, or
// source the parser rejects. Match the commands that would expose a secret.
export function secretSignatures(fragment: string): string[] {
  const denials: string[] = [];
  for (const segment of fragment.replace(/&&|\|\||\n/g, ";").split(";")) {
    if (
      /(^|[^A-Za-z0-9_-])printenv([^A-Za-z0-9_-]|$)/.test(segment) ||
      /^\s*\(*\s*(env|export|set|typeset|declare)\s*($|[|>)])/.test(segment) ||
      /(declare|typeset|export)\s+-[a-z]*[px]/.test(segment)
    )
      denials.push(dumpReason);
    if (/curl\s.*\s(-[A-Za-z]*v[A-Za-z]*|--verbose|--trace(-ascii)?)(\s|=|$)/s.test(segment)) denials.push(traceReason);
    if (/gh\s+auth\s+token/.test(segment)) denials.push(tokenReason);
    if (/security\s.*\s-[wg](\s|$)/s.test(segment)) denials.push(keychainReason);
    if (/(ECHO|PRINTF|PRINT)\s.*\$\{?[A-Z0-9_]*(TOKEN|SECRET|KEY|PASSWORD|CREDENTIAL)/s.test(segment.toUpperCase())) denials.push(varReason);
  }
  return denials;
}
