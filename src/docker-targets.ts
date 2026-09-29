import type { Context } from "./programs";
import type { Target, Word } from "./record";

// A bind mount hands a host directory to the container; the image, container names and the command run inside it are names.
export function dockerTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  const mount = (path: string, word: Word) => {
    claimed.add(word);
    targets.push(make(path, word, "read", { via: "option" }));
  };
  for (const [i, word] of words.entries()) {
    const glued = /^--(volume|mount)=/.exec(word.text);
    const key = glued?.[1] ?? (["-v", "--volume"].includes(word.text) ? "volume" : word.text === "--mount" ? "mount" : undefined);
    const value = glued ? word : words[i + 1];
    if (!key || !value) continue;
    const spec = glued ? value.text.slice(glued[0].length) : value.text;
    if (key === "volume") mount(spec.split(":")[0]!, value);
    else for (const field of spec.split(",")) if (/^(src|source)=/.test(field)) mount(field.slice(field.indexOf("=") + 1), value);
  }
  return targets;
}
