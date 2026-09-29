import type { Context } from "./programs";
import type { Target, Word } from "./record";

// Subcommands that print the content of a file, a commit, or the index.
const printing = new Set(["show", "diff", "log", "cat-file", "blame", "annotate", "grep", "archive", "format-patch", "whatchanged", "difftool", "diff-index", "diff-tree", "credential"]);
const valueOptions = ["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path"];

// git grep takes its pattern from -e, or from the first operand.
function grepOperands(operands: Word[]): Word[] {
  const rest: Word[] = [];
  let patterned = false;
  let options = true;
  for (let n = 0; n < operands.length; n++) {
    const text = operands[n]!.text;
    if (options && text === "--") options = false;
    else if (options && text === "-e") {
      patterned = true;
      n++;
    } else if (options && text === "-f") {
      // The file holds the patterns; git reads it.
      patterned = true;
      if (operands[n + 1]) rest.push(operands[++n]!);
    } else if (options && text.startsWith("-")) rest.push(operands[n]!);
    else if (!patterned) patterned = true;
    else rest.push(operands[n]!);
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
  if (!printing.has(sub)) return targets;
  let operands = words.slice(i + 1);
  if (sub === "grep") operands = grepOperands(operands);
  for (const word of operands) {
    if (word.text.startsWith("-")) continue;
    claimed.add(word);
    // Pathspecs are globs git expands itself; git grep also reads a directory whole.
    const glob = sub !== "grep";
    targets.push(make(word.text, word, "read", { via: "operand", glob }));
    // A rev:path operand names a file in a commit or the index.
    if (word.text.includes(":")) targets.push(make(word.text.slice(word.text.indexOf(":") + 1), word, "read", { via: "operand", glob }));
  }
  return targets;
}
