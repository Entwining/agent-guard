import type { Command, ValueRole, Word } from "./record";

const rgShortValues = "efgtTEABCmMjrd";
const rgLongValues = new Set(
  (
    "regexp file glob iglob type type-not encoding replace color colors sort sortr max-depth max-filesize pre pre-glob " +
    "engine threads max-columns type-add type-clear path-separator context-separator field-context-separator " +
    "field-match-separator after-context before-context context max-count ignore-file dfa-size-limit " +
    "regex-size-limit hyperlink-format"
  ).split(" "),
);
const grepShortValues = "efABCmdD";
const grepLongValues = new Set(("regexp file include exclude exclude-dir exclude-from label context after-context before-context max-count " + "binary-files devices directories").split(" "));

export function searchRoles(cmd: Command, args: Word[], prog: string) {
  const rg = prog === "rg";
  const shortValues = rg ? rgShortValues : grepShortValues;
  const longValues = rg ? rgLongValues : grepLongValues;
  const flags = cmd.flags;
  const operands: Word[] = [];
  let options = true;
  let unrestricted = 0;
  let noHidden = false;

  const value = (at: number, key: string, text: string) => {
    const target = args[at];
    if (!target) return;
    if (!rg && (key === "d" || key === "directories") && text === "recurse") flags.add("recursive");
    let role: ValueRole = "optarg";
    if (key === "e" || key === "regexp") {
      role = "pattern";
      flags.add("explicit");
    } else if (key === "f" || key === "file") {
      role = "patfile";
      flags.add("explicit");
    } else if (["g", "glob", "iglob", "include"].includes(key)) role = text.startsWith("!") ? "nglob" : "glob";
    target.role = target.role === "option" ? `option:${role}` : role;
    target.value = text;
  };

  for (let i = 0; i < args.length; i++) {
    const word = args[i]!.text;
    if (options && word === "--") {
      options = false;
      args[i]!.role = "option";
    } else if (options && /^--./.test(word)) {
      args[i]!.role = "option";
      const key = word.slice(2).split("=")[0]!;
      if (key === "files") flags.add("files");
      else if (rg && key === "hidden") {
        noHidden = false;
        flags.add("hidden");
      } else if (rg && key === "no-hidden") {
        noHidden = true;
        flags.delete("hidden");
      } else if (rg && key === "unrestricted" && ++unrestricted >= 2 && !noHidden) flags.add("hidden");
      else if (prog === "ag" && (key === "hidden" || key === "unrestricted")) flags.add("hidden");
      else if (key === "help" || key === "version") flags.add("help");
      else if (key === "fixed-strings") flags.add("fixed");
      else if (key === "recursive") flags.add("recursive");
      else if (key === "include") flags.add("include");
      if (word.includes("=")) value(i, key, word.slice(word.indexOf("=") + 1));
      else if (longValues.has(key)) {
        i++;
        value(i, key, args[i]?.text ?? "");
      }
    } else if (options && /^-./.test(word)) {
      args[i]!.role = "option";
      for (let k = 1; k < word.length; k++) {
        const c = word[k]!;
        if ((rg || prog === "grep") && c === "F") flags.add("fixed");
        if (prog === "grep" && (c === "r" || c === "R")) flags.add("recursive");
        if ((rg && c === "h") || c === "V") flags.add("help");
        if (rg && c === "r") flags.add("replace");
        if (rg && c === ".") flags.add("hidden");
        if (rg && c === "u" && ++unrestricted >= 2 && !noHidden) flags.add("hidden");
        if (prog === "ag" && c === "u") flags.add("hidden");
        if (!shortValues.includes(c)) continue;
        let text = word.slice(k + 1);
        if (rg) text = text.replace(/^=/, "");
        if (!text) {
          i++;
          text = args[i]?.text ?? "";
        }
        value(i, c, text);
        break;
      }
    } else {
      operands.push(args[i]!);
    }
  }
  let explicit = flags.has("explicit") || flags.has("files");
  for (const operand of operands) {
    operand.role = explicit ? "path" : "pattern";
    explicit = true;
  }
}
