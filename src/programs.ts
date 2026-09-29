// How each program obtains the paths it touches and what it does with them.
// A program with no entry reads every path it is handed.
import { dockerTargets } from "./docker-targets";
import { gitTargets } from "./git-targets";
import type { Command, Effect, Target, Word } from "./record";
import { searchTargets } from "./search-targets";
import { sshTargets } from "./ssh-targets";

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

// curl's short options that take a value; the rest of a cluster is that value.
export const curlValueLetters = "AbcCdDeEFHKmoPQrTtuUwxXyYz";
const curlDataOptions = /^(d|data|data-ascii|data-binary|data-urlencode|json|H|header|proxy-header|url-query|variable)$/;

// The options whose value is a file curl writes; `-` sends the output to standard output.
const curlWrites = ["o", "output", "D", "dump-header", "c", "cookie-jar", "etag-save", "libcurl", "stderr", "hsts", "alt-svc", "trace", "trace-ascii", "ssl-sessions"];

function curlTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  // The text after an @ or < inside a value is literal: neither the shell nor curl expands a ~ there.
  const read = (path: string, word: Word, quoted?: boolean, sends = true) => {
    claimed.add(word);
    targets.push(make(path, word, "read", { via: "option", sends, ...(quoted && { quoted }) }));
  };
  // -O and --remote-name save each download under its remote name, in the working directory or the --output-dir.
  let remoteName = false;
  let outputDir: Word | undefined;
  for (let i = 0; i < words.length; i++) {
    const word = words[i]!;
    const text = word.text;
    if (text === "--") break;
    remoteName ||= /^(--remote-name(-all)?|-[^-]*O)$/.test(text);
    if (/^--output-dir(=|$)/.test(text)) {
      outputDir = text.includes("=") ? word : words[++i];
      if (outputDir) claimed.add(outputDir);
      continue;
    }
    // curl reads `file:path` and `file://path` alike, decodes %XX, and expands `[a-z]` and `{a,b}` ranges.
    const url = /^(--url=)?(file:.*)$/is.exec(text);
    if (url) {
      claimed.add(word);
      const path = url[2]!.replace(/^file:(\/\/)?/i, "").replace(/%([0-9a-f]{2})/gi, (_, hex: string) => String.fromCharCode(parseInt(hex, 16)));
      targets.push(make(path, word, "read", { via: "operand", glob: /[[{]/.test(path) }));
      continue;
    }
    let key = "";
    let value = "";
    const long =
      /^--(data|data-ascii|data-binary|data-urlencode|json|form|header|proxy-header|url-query|variable|upload-file|config|output|dump-header|write-out|cookie|etag-compare|cookie-jar|etag-save|libcurl|stderr|hsts|alt-svc|trace|trace-ascii|ssl-sessions)(=|$)/s.exec(
        text,
      );
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
    } else if (key === "F" || key === "form") {
      const file = value.slice(value.indexOf("=") + 1).replace(/^[@<]/, "");
      // A quoted file name may hold a `;`.
      read(/^"([^"]*)"/.exec(file)?.[1] ?? file.split(";")[0]!, from, true);
    } else if (["T", "upload-file", "K", "config", "etag-compare"].includes(key)) read(value, from);
    // The response is written out with the text of a `@file` template; `@-` is standard input.
    else if ((key === "w" || key === "write-out") && /^@./.test(value) && value !== "@-") read(value.slice(1), from, true, false);
    // A cookie value with no `=` names a cookie file that curl parses itself.
    else if ((key === "b" || key === "cookie") && value && !value.includes("=")) {
      claimed.add(from);
      targets.push(make(value, from, "use", { via: "option" }));
    } else if (curlWrites.includes(key) && value) {
      claimed.add(from);
      // `-o -` sends the response to standard output, so it names no file.
      if (value !== "-") targets.push(make(value, from, "write", { via: "option" }));
    }
  }
  if (remoteName) targets.push(make(outputDir ? outputDir.text.replace(/^--output-dir=/, "") : ".", outputDir, "write", { via: "option", walk: "none" }));
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

// The archive is the value of -f or --file: the rest of a cluster such as `-cfout.tar`, or the next word.
function tarArchive(words: Word[]): { word: Word; path: string } | undefined {
  for (const [i, word] of words.entries()) {
    const text = word.text;
    if (text.startsWith("--file=")) return { word, path: text.slice("--file=".length) };
    const next = words[i + 1];
    if (text === "--file" && next) return { word: next, path: next.text };
    const cluster = tarCluster(word, i) ? /^-?[^Cf]*f(.*)$/s.exec(text) : null;
    if (cluster?.[1]) return { word, path: cluster[1] };
    if (cluster && next) return { word: next, path: next.text };
  }
  return undefined;
}

// A directory given to -C is entered, and later operands are relative to it.
function tarTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  let base: string | undefined;
  const creates = words.some((word, i) => word.text === "--create" || (tarCluster(word, i) && /^-?[^CfT]*c/.test(word.text)));
  // Extraction writes into the directory -C names, so the directory is judged as a write.
  const extracts = words.some((word, i) => /^--(extract|get)$/.test(word.text) || (tarCluster(word, i) && /^-?[^CfT]*x/.test(word.text)));
  const archive = tarArchive(words);
  if (archive) {
    claimed.add(archive.word);
    targets.push(make(archive.path, archive.word, creates ? "write" : "read", { via: "option" }));
  }
  for (const [i, word] of words.entries()) {
    const text = word.text;
    if (word === archive?.word) continue;
    if (/^--exclude=/.test(text)) {
      claimed.add(word);
      continue;
    }
    // The directory is the word after -C, --directory or --cd, or the value glued to them, as in `-C/dir` and `-xC/dir`.
    const glued = /^(?:--directory=|-[^-]*C)(.+)$/s.exec(text)?.[1];
    const enters = glued !== undefined || /^(--directory|--cd|-[^-]*C)$/.test(words[i - 1]?.text ?? "");
    if (!enters) {
      if (base !== undefined && !text.startsWith("-")) {
        claimed.add(word);
        targets.push(make(text, word, "read", { via: "operand", base }));
      }
      continue;
    }
    claimed.add(word);
    const dir = glued ?? text;
    const target = make(dir, word, extracts ? "write" : "enter", { via: "option", base });
    targets.push(target);
    base = target.path;
  }
  // Without -C the members land in the working directory, unless -O sends them to stdout.
  const toStdout = words.some((word, i) => word.text === "--to-stdout" || (tarCluster(word, i) && /^-?[^CfT]*O/.test(word.text)));
  if (extracts && base === undefined && !toStdout) targets.push(make(".", undefined, "write", { via: "option", walk: "none" }));
  return targets;
}

const wgetShort: Record<string, string> = { i: "input-file", O: "output-document", e: "execute", P: "directory-prefix", o: "output-file", a: "append-output" };
// The options whose value is written: the download, the log, or the directory the downloads go to.
const wgetWrites = ["output-document", "output-file", "append-output", "directory-prefix", "save-cookies", "warc-file", "hsts-file"];
// wgetrc command names ignore case, underscores and hyphens.
const wgetrc: Record<string, string> = {
  postfile: "post-file",
  bodyfile: "body-file",
  input: "input-file",
  outputdocument: "output-document",
  logfile: "output-file",
  dirprefix: "directory-prefix",
  loadcookies: "load-cookies",
  savecookies: "save-cookies",
  warcfile: "warc-file",
  hstsfile: "hsts-file",
};

// The file that holds the request body leaves the machine, as can a wgetrc file that names one, and the output document is written; `-e` runs a wgetrc command that can name either.
function wgetTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  // A download lands in the working directory unless -O or a directory prefix says otherwise.
  let placed = false;
  for (const [i, word] of words.entries()) {
    const text = word.text;
    if (text === "--spider") placed = true;
    const long = /^--(post-file|body-file|input-file|output-document|output-file|append-output|directory-prefix|save-cookies|warc-file|hsts-file|execute|config|load-cookies)(=|$)/.exec(text);
    // The value is glued to the flag (-i.env) or follows it.
    const short = /^-[A-Za-z]*?([ieOPoa])(.*)$/.exec(text);
    let key = long?.[1] ?? wgetShort[short?.[1] ?? ""];
    if (!key) continue;
    const glued = long ? text.includes("=") : !!short?.[2];
    const value = glued ? word : words[i + 1];
    if (!value) continue;
    let path = !glued ? value.text : long ? text.slice(text.indexOf("=") + 1) : short![2]!;
    if (key === "execute") {
      const command = /^\s*([A-Za-z_-]+)\s*=\s*(.*)$/.exec(path);
      const name = wgetrc[command?.[1]!.toLowerCase().replace(/[_-]/g, "") ?? ""];
      if (!command || !name) continue;
      key = name;
      path = command[2]!;
    }
    claimed.add(value);
    placed ||= key === "output-document" || key === "directory-prefix";
    // `-O -` writes the document to standard output, so it names no file.
    if (key === "output-document" && path === "-") continue;
    targets.push(make(path, value, wgetWrites.includes(key) ? "write" : "read", { via: "option", walk: "none", sends: ["post-file", "body-file", "config"].includes(key) }));
  }
  if (!placed) targets.push(make(".", undefined, "write", { via: "option", walk: "none" }));
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
for (const name of ["cd", "pushd", "popd"]) specs.set(name, { operands: "enter", walk: "none" });
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
for (const name of ["node", "bun", "deno"]) specs.set(name, { options: { "--env-file": "use" } });
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
