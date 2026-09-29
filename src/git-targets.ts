import type { Context } from "./programs";
import type { Effect, Target, Word } from "./record";

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
]);
// Options whose value is a file git reads.
const fileOptions: Record<string, string[]> = { config: ["-f", "--file", "--blob"], commit: ["-F", "--file"] };
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
  while (i < words.length && words[i]!.text.startsWith("-")) {
    const takesValue = valueOptions.includes(words[i]!.text);
    const value = words[i + 1];
    if (takesValue && value && ["-C", "--work-tree"].includes(words[i]!.text)) {
      claimed.add(value);
      targets.push(make(value.text, value, "enter", { via: "option" }));
    }
    i += takesValue ? 2 : 1;
  }
  const sub = words[i]?.text ?? "";
  const effect: Effect | undefined = printing.has(sub) ? "read" : metadata.has(sub) ? "meta" : names.has(sub) ? "name" : undefined;
  // Any other subcommand reads its operands like an unmodelled program.
  if (!effect) return targets;
  let operands = words.slice(i + 1);
  if (sub === "grep") {
    const patterns: Word[] = [];
    operands = grepOperands(operands, patterns);
    for (const pattern of patterns) claimed.add(pattern);
  }
  const keys = fileOptions[sub] ?? [];
  // Pathspecs are globs git expands itself; git grep also reads a directory whole.
  const glob = effect !== "name" && sub !== "grep";
  const add = (path: string, word: Word, as: Effect, via: "operand" | "option") => {
    claimed.add(word);
    targets.push(make(path, word, as, { via, glob }));
    // A rev:path operand names a file in a commit or the index.
    if (as === "read" && path.includes(":")) targets.push(make(path.slice(path.indexOf(":") + 1), word, as, { via, glob }));
  };
  for (let n = 0; n < operands.length; n++) {
    const word = operands[n]!;
    const glued = keys.find((key) => key.startsWith("--") && word.text.startsWith(`${key}=`));
    if (glued) add(word.text.slice(glued.length + 1), word, "read", "option");
    else if (keys.includes(word.text) && operands[n + 1]) {
      const file = operands[++n]!;
      add(file.text, file, "read", "option");
    } else if (!word.text.startsWith("-")) add(word.text, word, effect, "operand");
  }
  return targets;
}
