import { basename } from "node:path";

import type { Effect, Target } from "../record";
import type { Context } from "./programs";

// rg, grep, ag and ack: the roles search-roles.ts gave the words say which are roots, pattern files and globs.
export function searchTargets(name: string, { cmd, words, make, claimed }: Context): Target[] {
  const help = cmd.flags.has("help");
  const files = cmd.flags.has("files");
  const recursive = name === "grep" && cmd.flags.has("recursive");
  const walk = recursive || (["rg", "ag"].includes(name) && cmd.flags.has("hidden")) ? "hidden" : "visible";
  // Listing names, or printing help, reads no content; a hidden listing feeds what reads it.
  const effect: Effect = help || (files && walk !== "hidden") ? "list" : "read";
  const targets: Target[] = [];
  for (const [i, word] of words.entries()) {
    // The file these options name holds ignore rules that the search reads.
    const owner = word.role === "optarg" ? words[i - 1] : word.role === "option:optarg" ? word : undefined;
    if (word.role === "path") targets.push(make(word.value, word, effect, { via: "operand", walk }));
    else if (word.role === "patfile" || word.role === "option:patfile") targets.push(make(word.value, word, effect, { via: "option", walk: "none" }));
    else if (owner && /^--(ignore-file|exclude-from)(=|$)/.test(owner.text)) targets.push(make(word.value, word, "read", { via: "option", walk: "none" }));
    else continue;
    claimed.add(word);
  }
  const scoped = targets.some((target) => target.via === "operand");
  if (!scoped && (name !== "grep" || recursive)) targets.push(make(cmd.cwd, undefined, help ? "list" : effect, { via: name === "grep" ? "cwd" : "scan", walk, search: !help }));
  for (const word of words) {
    if (word.role !== "glob" && word.role !== "option:glob") continue;
    claimed.add(word);
    // A positive glob selects files even past ignore rules.
    targets.push(make(basename(word.value), word, effect, { via: "option", glob: true, walk: "none" }));
  }
  return targets;
}
