import type { Effect, Target, Word } from "../record";
import type { Context } from "./programs";

// Subcommands that print the content of a file, a commit, or the index.
const printing = new Set(["show", "diff", "log", "cat-file", "blame", "annotate", "grep", "archive", "format-patch", "whatchanged", "difftool", "diff-index", "diff-tree", "credential"]);
// Subcommands whose operands are paths that git stages, moves, or inspects without printing them.
const metadata = new Set(["add", "rm", "mv", "restore", "checkout", "reset", "stash", "check-ignore", "check-attr", "update-index", "ls-files", "status", "clean", "commit"]);
// Subcommands whose operands are refs, remotes, names, or URLs.
const names = new Set([
  "branch",
  "tag",
  "remote",
  "switch",
  "push",
  "fetch",
  "pull",
  "merge",
  "rebase",
  "cherry-pick",
  "revert",
  "reflog",
  "rev-parse",
  "describe",
  "bisect",
  "init",
  "clone",
  "submodule",
  "worktree",
  "config",
  "lfs",
  "sparse-checkout",
]);
// Options whose value is a file git reads.
const pathspecFile = ["--pathspec-from-file"];
const fileOptions: Record<string, string[]> = {
  config: ["-f", "--file", "--blob"],
  commit: ["-F", "--file", ...pathspecFile],
  tag: ["-F", "--file"],
  merge: ["-F", "--file"],
  add: pathspecFile,
  rm: pathspecFile,
  restore: pathspecFile,
  reset: pathspecFile,
  checkout: pathspecFile,
  stash: pathspecFile,
};
const valueOptions = ["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path"];

// git grep takes its pattern from -e, or from the first operand; the pattern is text, not a path.
function grepOperands(operands: Word[], patterns: Word[]): Word[] {
  const rest: Word[] = [];
  let patterned = false;
  let options = true;
  for (let n = 0; n < operands.length; n++) {
    const text = operands[n]!.text;
    if (options && text === "--") options = false;
    else if (options && text === "-e") {
      patterned = true;
      if (operands[n + 1]) patterns.push(operands[++n]!);
    } else if (options && text === "-f") {
      // The file holds the patterns; git reads it.
      patterned = true;
      if (operands[n + 1]) rest.push(operands[++n]!);
    } else if (options && text.startsWith("-")) rest.push(operands[n]!);
    else if (!patterned) {
      patterned = true;
      patterns.push(operands[n]!);
    } else rest.push(operands[n]!);
  }
  return rest;
}

export function gitTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  let i = 0;
  // Operands are relative to the directory the last -C or --work-tree names.
  let base: string | undefined;
  while (i < words.length && words[i]!.text.startsWith("-")) {
    const text = words[i]!.text;
    const glued = /^--work-tree=(.*)$/s.exec(text);
    if (/^--(?:namespace|exec-path)=/.test(text)) claimed.add(words[i]!);
    const takesValue = !glued && valueOptions.includes(text);
    const value = glued ? words[i] : words[i + 1];
    if ((glued || (takesValue && ["-C", "--work-tree"].includes(text))) && value) {
      claimed.add(value);
      const target = make(glued ? glued[1]! : value.text, value, "enter", { via: "option", base });
      targets.push(target);
      base = target.path;
    }
    i += takesValue ? 2 : 1;
  }
  const sub = words[i]?.text ?? "";
  if (words[i]) claimed.add(words[i]!);
  // Any subcommand git has no group for reads its operands like an unmodelled program.
  const effect: Effect = printing.has(sub) ? "read" : metadata.has(sub) ? "meta" : names.has(sub) ? "name" : "read";
  let operands = words.slice(i + 1);
  if (sub === "grep") {
    const patterns: Word[] = [];
    operands = grepOperands(operands, patterns);
    for (const pattern of patterns) claimed.add(pattern);
  }
  const keys = fileOptions[sub] ?? [];
  // A pathspec holding a glob character is one git expands itself; git grep also reads a directory whole.
  const pathspec = effect !== "name" && sub !== "grep";
  const add = (path: string, word: Word, as: Effect, via: "operand" | "option") => {
    claimed.add(word);
    const glob = pathspec && /[*?[]/.test(path);
    targets.push(make(path, word, as, { via, glob, base }));
    // A rev:path operand names a file in a commit or the index.
    if (as === "read" && path.includes(":")) targets.push(make(path.slice(path.indexOf(":") + 1), word, as, { via, glob, base }));
  };
  // `bundle create FILE` writes FILE; the other bundle subcommands read the bundle they are given.
  const bundleAction = sub === "bundle" ? operands.find((word) => !word.text.startsWith("-")) : undefined;
  const bundleFile = bundleAction?.text === "create" ? operands.filter((word) => !word.text.startsWith("-"))[1] : undefined;
  for (let n = 0; n < operands.length; n++) {
    const word = operands[n]!;
    // A value is glued to a long option with = and to a short one directly.
    const glued = keys.find((key) => (key.startsWith("--") ? word.text.startsWith(`${key}=`) : word.text.length > key.length && word.text.startsWith(key)));
    if (glued) add(word.text.slice(glued.length + (glued.startsWith("--") ? 1 : 0)), word, "read", "option");
    else if (keys.includes(word.text) && operands[n + 1]) {
      const file = operands[++n]!;
      add(file.text, file, "read", "option");
    } else if (word === bundleAction) claimed.add(word);
    else if (!word.text.startsWith("-")) add(word.text, word, word === bundleFile ? "write" : effect, "operand");
  }
  return targets;
}
