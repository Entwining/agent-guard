import type { Command, Effect, Target, Word } from "../record";
// How each program obtains the paths it touches and what it does with them.
// A program with no entry reads every path it is handed.
import { curlTargets } from "./curl";
import { dockerTargets } from "./docker";
import { gitTargets } from "./git";
import { searchTargets } from "./search";
import { sshTargets } from "./ssh";
import { tarTargets } from "./tar";
import { wgetTargets } from "./wget";

type Walk = Target["walk"];

interface Make {
  via?: Target["via"];
  base?: string | undefined;
  glob?: boolean;
  expands?: boolean;
  sends?: boolean;
  walk?: Walk;
  search?: boolean;
  quoted?: boolean; // the path is literal: no ~ expansion
}

export interface Context {
  cmd: Command;
  words: Word[];
  walk: Walk;
  claimed: Set<Word>;
  make: (path: string, word: Word | undefined, effect: Effect, options?: Make) => Target;
}

export interface ProgramSpec {
  operands?: Effect; // default "read": a program the table does not model reads every path it is handed
  walk?: Walk | { recursive: RegExp }; // default "visible"; { recursive } is "visible" only when an option matches
  options?: Record<string, Effect>; // effect of an option's value, glued with = or separate
  last?: Effect; // the final operand
  sends?: boolean; // reads leave the machine
  remote?: RegExp; // an operand on another machine is a name; with `sends`, reads leave only when an operand is remote
  cwd?: "cwd" | "scan"; // a command with no path operand lists its working directory
  targets?: (ctx: Context) => Target[];
}

export const DEFAULT_EFFECT: Effect = "read";

// Programs that read only the paths they are handed, so a command that names another directory does not read the working directory.
export const readers = (
  "cat head tail less more bat sed awk jq yq base64 xxd od strings diff openssl plutil cp tee tar source . sort " +
  "uniq cut nl fold rev paste comm join iconv hexdump hd zcat gzcat bzcat xzcat ag ack tac column pr vim vi nvim view perl ruby dd scp rsync zip ed ex hg svn sh bash zsh dash ksh wget php zgrep zless zmore"
).split(" ");
export const dataPrograms = "echo printf print : true false export set unset typeset declare local".split(" ");
// Metadata, counts and digests: no content reaches the output.
const meta = "stat test [ chmod chown chgrp chflags touch rm rmdir mkdir mv ln wc file shasum sha1sum sha256sum md5 md5sum cksum realpath readlink basename dirname".split(" ");
const noWalk = new Set("stat test [ mkdir mv".split(" "));

// The value of these options is consumed by the program, not read into the output.
const intoDirectory: Record<string, Effect> = { "-t": "write", "--target-directory": "write" };
export const globalOptions: Record<string, Effect> = { "--exclude": "name", "--exclude-dir": "name", "--include": "name" };

const remote = /^(rsync:\/\/|([^/@:]+@)?[^/@:]+:)/;
const lsRecursive = /^(--recursive$|-[^-]*R)/;
const anyRecursive = /^(--recursive$|-[^-]*[rR])/;

function ddTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  for (const word of words) {
    claimed.add(word);
    if (word.text.startsWith("if=")) targets.push(make(word.text.slice(3), word, "read", { via: "option" }));
    else if (word.text.startsWith("of=")) targets.push(make(word.text.slice(3), word, "write", { via: "option" }));
  }
  return targets;
}

const findExpression = /^(-|\(|!)/;
const findExec = ["-exec", "-execdir", "-ok", "-okdir"];

// The paths before find's first expression word, including the one `-f` names.
export function findRoots(words: Word[]): Word[] {
  const roots: Word[] = [];
  let i = 0;
  while (words[i]) {
    if (words[i]!.text === "-f" && words[i + 1]) roots.push(words[(i += 2) - 1]!);
    else if (words[i]!.text === "--" || /^-[HLPEXxdsO]/.test(words[i]!.text)) i++;
    else break;
  }
  for (; words[i] && !findExpression.test(words[i]!.text); i++) roots.push(words[i]!);
  return roots;
}

function findTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  for (const root of findRoots(words)) {
    claimed.add(root);
    targets.push(make(root.text, root, "list", { via: "operand", walk: "hidden" }));
  }
  for (const word of words) if (!findExec.includes(word.text)) claimed.add(word);
  return targets;
}

// The first operand is the filter, unless -f names a file that holds it; yq names its evaluation command before the filter.
const filterTargets =
  (commands: string[]) =>
  ({ words, claimed }: Context): Target[] => {
    const operands = words.filter((word) => !word.text.startsWith("-"));
    const filter = words.some((word) => word.text === "-f" || word.text === "--from-file") ? undefined : operands[commands.includes(operands[0]?.text ?? "") ? 1 : 0];
    if (filter) claimed.add(filter);
    return [];
  };

export const specs = new Map<string, ProgramSpec>();
for (const name of readers) specs.set(name, {});
for (const name of dataPrograms) specs.set(name, { operands: "name", walk: "none" });
for (const name of meta) specs.set(name, { operands: "meta", ...(noWalk.has(name) && { walk: "none" as const }) });
specs.set("cd", { operands: "name", walk: "none" });
for (const name of ["pushd", "popd"]) specs.set(name, { operands: "enter", walk: "none" });
specs.set("jq", { targets: filterTargets([]) });
specs.set("yq", { targets: filterTargets(["eval", "e", "eval-all", "ea"]) });
specs.set("gh", {});
specs.set("ls", { operands: "list", walk: { recursive: lsRecursive }, cwd: "cwd" });
specs.set("tree", { operands: "list", cwd: "scan" });
specs.set("du", { operands: "list", cwd: "scan" });
specs.set("cp", { options: intoDirectory, last: "write", walk: { recursive: anyRecursive } });
specs.set("dd", { walk: "none", targets: ddTargets });
specs.set("tar", { targets: tarTargets });
specs.set("tee", { operands: "write" });
specs.set("install", { options: intoDirectory, last: "write" });
specs.set("scp", { last: "write", sends: true, remote, targets: sshTargets("scp") });
specs.set("sftp", { targets: sshTargets("sftp") });
specs.set("rsync", { options: { "--files-from": "read", "--exclude-from": "read", "--include-from": "read" }, last: "write", sends: true, remote });
specs.set("wget", {
  operands: "name",
  options: { "--ca-certificate": "use", "--ca-directory": "use", "--certificate": "use", "--private-key": "use", "--crl-file": "use", "--random-file": "use" },
  targets: wgetTargets,
});
// The value of these options is a certificate, key or list file that curl uses itself.
const curlUsedFiles =
  "--cacert --capath --cert --key -E --netrc-file --crlfile --egd-file --knownhosts --proxy-cacert --proxy-capath --proxy-cert --proxy-crlfile --proxy-key --random-file --pubkey --pinnedpubkey --proxy-pinnedpubkey --unix-socket";
specs.set("curl", { operands: "name", options: Object.fromEntries(curlUsedFiles.split(" ").map((option) => [option, "use" as const])), targets: curlTargets });
specs.set("git", { targets: gitTargets });
specs.set("docker", { operands: "name", options: { "--env-file": "use" }, targets: dockerTargets });
specs.set("node", { options: { "--env-file": "use", "--env-file-if-exists": "use" } });
for (const name of ["bun", "deno"]) specs.set(name, { options: { "--env-file": "use" } });
specs.set("kubectl", { options: { "--kubeconfig": "use" } });
specs.set("ssh", { operands: "name", targets: sshTargets("ssh") });
specs.set("ssh-add", { operands: "use" });
specs.set("ssh-keygen", { options: { "-f": "use" } });
specs.set("dotenvx", { options: { "-f": "use", "--file": "use", "--env-file": "use" } });
for (const name of ["npm", "pnpm", "yarn"]) specs.set(name, { options: { "--userconfig": "use" } });
for (const name of ["rg", "grep", "ag", "ack"]) specs.set(name, { targets: (ctx) => searchTargets(name, ctx) });
specs.set("find", { operands: "list", cwd: "scan", targets: findTargets });
specs.set("fd", { operands: "list", cwd: "scan" });

export function specFor(name: string): ProgramSpec {
  return specs.get(name) ?? {};
}

export function walkOf(spec: ProgramSpec, name: string, words: Word[]): Walk {
  const { walk } = spec;
  if (typeof walk === "object") return words.some((word) => walk.recursive.test(word.text)) ? "visible" : "none";
  if (name === "git" && words.some((word) => word.text === "config")) return "none";
  return walk ?? "visible";
}
