import type { Word } from "./record";

// Per interpreter: the short flags after which the code follows, in the rest of the cluster when `glued` and in the next word otherwise,
// and the flags whose value is the rest of the cluster, so a value such as `-rtime` does not read its last letter as a code flag.
const flags: Record<string, { code: string; value: string; glued: boolean }> = {
  python: { code: "c", value: "WX", glued: true },
  python3: { code: "c", value: "WX", glued: true },
  node: { code: "ep", value: "", glued: false },
  bun: { code: "ep", value: "", glued: true },
  ruby: { code: "e", value: "rICEix", glued: true },
  perl: { code: "eE", value: "MmIidDCFx", glued: true },
  php: { code: "rR", value: "dcfz", glued: true },
  osascript: { code: "e", value: "", glued: true },
  lua: { code: "e", value: "l", glued: true },
  deno: { code: "", value: "", glued: false },
};

// The table entry a program name selects, ignoring a version suffix such as `python3.14`.
export function interpreterName(name: string): string | undefined {
  return [name, name.replace(/[\d.]+$/, "")].find((candidate) => candidate in flags);
}

// The code an interpreter runs from its command line: the word after a code flag, the value glued to `--eval=`, or deno's eval operand.
export function interpreterCode(name: string, args: Word[]): string[] {
  const { code, value, glued } = flags[name]!;
  const found: string[] = [];
  const take = (word: Word | undefined) => {
    if (!word) return;
    word.role = "code";
    found.push(word.text);
  };
  if (name === "deno") {
    // Flags such as `-p` and `--ext=ts` sit between `eval` and the code.
    // `eval` follows the global flags and the value of a flag such as `--log-level debug`.
    const operands = args.filter((word) => !word.text.startsWith("-"));
    const at = operands.slice(0, 2).findIndex((word) => word.text === "eval");
    if (at >= 0) for (const word of operands.slice(at + 1)) take(word);
    return found;
  }
  for (let i = 0; i < args.length; i++) {
    const text = args[i]!.text;
    // node's --run names a package script; only php's takes code.
    const long = (name === "php" ? /^--(eval|print|run)(=(.*))?$/s : /^--(eval|print)(=(.*))?$/s).exec(text);
    if (long) {
      if (long[2] === undefined) take(args[++i]);
      else found.push(long[3]!);
      continue;
    }
    if (!/^-[^-]/.test(text)) continue;
    for (let k = 1; k < text.length; k++) {
      if (value.includes(text[k]!)) break;
      if (code.includes(text[k]!)) {
        if (k < text.length - 1 && !glued) continue;
        if (k < text.length - 1) found.push(text.slice(k + 1));
        else take(args[++i]);
        break;
      }
    }
  }
  return found;
}
