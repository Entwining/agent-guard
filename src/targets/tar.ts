import type { Target, Word } from "../record";
import type { Context } from "./programs";

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
export function tarTargets({ words, make, claimed }: Context): Target[] {
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
