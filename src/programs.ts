// How each program obtains the paths it touches and what it does with them.
// A program with no entry reads nothing the guard can name.
import { gitTargets } from "./git-targets";
import type { Command, Effect, Target, Word } from "./record";
import { searchTargets } from "./search-targets";

type Walk = Target["walk"];

interface Make {
  via?: Target["via"];
  base?: string | undefined;
  glob?: boolean;
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
  operands?: Effect; // default: DEFAULT_EFFECT
  walk?: Walk | { recursive: RegExp }; // default "visible"; { recursive } is "visible" only when an option matches
  last?: Effect; // the final operand
  sends?: boolean; // reads leave the machine
  remote?: RegExp; // an operand on another machine is a name; with `sends`, reads leave only when an operand is remote
  cwd?: "cwd" | "scan"; // a command with no path operand lists its working directory
  targets?: (ctx: Context) => Target[];
}

export const DEFAULT_EFFECT: Effect = "use";

export const readers = (
  "cat head tail less more bat sed awk jq yq base64 xxd od strings diff openssl plutil cp tee tar source . sort " +
  "uniq cut nl fold rev paste comm join iconv hexdump hd zcat gzcat bzcat xzcat ag ack tac column pr vim vi nvim view perl ruby dd scp rsync zip ed ex hg svn sh bash zsh dash ksh wget php zgrep zless zmore"
).split(" ");
const dataPrograms = "echo printf print : true false export set unset typeset declare local".split(" ");
const noWalk = "stat test [ mkdir mv cd pushd popd".split(" ");

const remote = /^(rsync:\/\/|([^/@:]+@)?[^/@:]+:)/;
const lsRecursive = /^(--recursive$|-[^-]*R)/;
const anyRecursive = /^(--recursive$|-[^-]*[rR])/;

// curl's short options that take a value; the rest of a cluster is that value.
export const curlValueLetters = "AbcCdDeEFHKmoPQrTtuUwxXyYz";
const curlDataOptions = /^(d|data|data-ascii|data-binary|data-urlencode|json|H|header)$/;

function curlTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  // The text after an @ or < inside a value is literal: neither the shell nor curl expands a ~ there.
  const read = (path: string, word: Word, quoted?: boolean) => {
    claimed.add(word);
    targets.push(make(path, word, "read", { via: "option", sends: true, ...(quoted && { quoted }) }));
  };
  for (let i = 0; i < words.length; i++) {
    const word = words[i]!;
    const text = word.text;
    if (text === "--") break;
    if (/^file:\/\//i.test(text)) {
      claimed.add(word);
      targets.push(make(text, word, "read", { via: "operand" }));
      continue;
    }
    let key = "";
    let value = "";
    const long = /^--(data|data-ascii|data-binary|data-urlencode|json|form|header|upload-file|config|output|dump-header)(=|$)/s.exec(text);
    if (long) {
      key = long[1]!;
      value = text.includes("=") ? text.slice(text.indexOf("=") + 1) : (words[++i]?.text ?? "");
    } else if (/^-[^-]/.test(text)) {
      for (let k = 1; k < text.length; k++) {
        if (!curlValueLetters.includes(text[k]!)) continue;
        key = text[k]!;
        value = text.slice(k + 1) || (words[++i]?.text ?? "");
        break;
      }
    }
    const from = words[i]!;
    if (curlDataOptions.test(key)) {
      if (/@./s.test(value)) read(value.slice(value.indexOf("@") + 1), from, true);
    } else if (key === "F" || key === "form")
      read(
        value
          .slice(value.indexOf("=") + 1)
          .replace(/^[@<]/, "")
          .split(";")[0]!,
        from,
        true,
      );
    else if (["T", "upload-file", "K", "config"].includes(key)) read(value, from);
    else if (["o", "output", "D", "dump-header"].includes(key) && value) {
      claimed.add(from);
      targets.push(make(value, from, "write", { via: "option" }));
    }
  }
  return targets;
}

function ddTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  for (const word of words) {
    claimed.add(word);
    if (word.text.startsWith("if=")) targets.push(make(word.text.slice(3), word, "read", { via: "option" }));
    else if (word.text.startsWith("of=")) targets.push(make(word.text.slice(3), word, "write", { via: "option" }));
  }
  return targets;
}

// A cluster of tar's short options, such as `-czf`, or the first word without a dash.
const tarCluster = (word: Word, i: number) => !word.text.startsWith("--") && (word.text.startsWith("-") || i === 0);

// The archive is the value of -f or --file.
function tarArchive(words: Word[]): Word | undefined {
  for (const [i, word] of words.entries()) if (word.text === "--file" || (tarCluster(word, i) && /^-?[^Cf]*f/.test(word.text))) return words[i + 1];
  return words.find((word) => word.text.startsWith("--file="));
}

// A directory given to -C is entered, and later operands are relative to it.
function tarTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  let base: string | undefined;
  const creates = words.some((word, i) => word.text === "--create" || (tarCluster(word, i) && /^-?[^CfT]*c/.test(word.text)));
  const archive = tarArchive(words);
  if (archive) {
    claimed.add(archive);
    targets.push(make(archive.text.replace(/^--file=/, ""), archive, creates ? "write" : "read", { via: "option" }));
  }
  for (const [i, word] of words.entries()) {
    const text = word.text;
    if (word === archive) continue;
    if (/^--exclude=/.test(text)) {
      claimed.add(word);
      continue;
    }
    const enters = ["-C", "--directory"].includes(words[i - 1]?.text ?? "") || text.startsWith("--directory=");
    if (!enters) {
      if (base !== undefined && !text.startsWith("-")) {
        claimed.add(word);
        targets.push(make(text, word, "read", { via: "operand", base }));
      }
      continue;
    }
    claimed.add(word);
    const dir = text.startsWith("--directory=") ? text.slice("--directory=".length) : text;
    const target = make(dir, word, "enter", { via: "option", base });
    targets.push(target);
    base = target.path;
  }
  return targets;
}

// URLs are names; the file that holds the request body leaves the machine, and the output document is written.
function wgetTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  for (const [i, word] of words.entries()) {
    const text = word.text;
    const long = /^--(post-file|body-file|input-file|output-document)(=|$)/.exec(text);
    const short = /^-[^-]*([iO])$/.exec(text);
    const key = long?.[1] ?? short?.[1];
    if (/^[a-z][a-z0-9+.-]*:\/\//i.test(text)) claimed.add(word);
    if (!key) continue;
    const value = text.includes("=") ? word : words[i + 1];
    if (!value) continue;
    claimed.add(value);
    const path = value === word ? text.slice(text.indexOf("=") + 1) : value.text;
    const write = key === "output-document" || key === "O";
    targets.push(make(path, value, write ? "write" : "read", { via: "option", sends: key === "post-file" || key === "body-file" }));
  }
  return targets;
}

const findExpression = /^(-|\(|!)/;
const findExec = ["-exec", "-execdir", "-ok", "-okdir"];

// The paths before find's first expression word.
export function findRoots(words: Word[]): Word[] {
  let i = 0;
  while (words[i] && /^-[HLPEXxdsO]/.test(words[i]!.text)) i++;
  const roots: Word[] = [];
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
for (const name of readers) specs.set(name, { operands: "read" });
for (const name of dataPrograms) specs.set(name, { operands: "name", walk: "none" });
for (const name of noWalk) specs.set(name, { walk: "none" });
specs.set("jq", { operands: "read", targets: filterTargets([]) });
specs.set("yq", { operands: "read", targets: filterTargets(["eval", "e", "eval-all", "ea"]) });
specs.set("gh", { operands: "read" });
specs.set("ls", { operands: "list", walk: { recursive: lsRecursive }, cwd: "cwd" });
specs.set("tree", { operands: "list", cwd: "scan" });
specs.set("du", { cwd: "scan" });
specs.set("cp", { operands: "read", last: "write", walk: { recursive: anyRecursive } });
specs.set("dd", { walk: "none", targets: ddTargets });
specs.set("tar", { operands: "read", targets: tarTargets });
specs.set("tee", { operands: "write" });
specs.set("install", { last: "write" });
specs.set("scp", { operands: "read", last: "write", sends: true, remote });
specs.set("rsync", { operands: "read", last: "write", sends: true, remote });
specs.set("wget", { operands: "read", targets: wgetTargets });
specs.set("curl", { targets: curlTargets });
specs.set("git", { targets: gitTargets });
for (const name of ["rg", "grep", "ag", "ack"]) specs.set(name, { operands: "read", targets: (ctx) => searchTargets(name, ctx) });
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
