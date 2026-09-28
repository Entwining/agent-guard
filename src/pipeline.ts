import { basename } from "node:path";

import type { Command } from "./record";

// A literal printf value is known before xargs substitutes it into a command.
export function xargsReplacements(left: Command[], right: Command[]): { source: string; cwd: string }[] {
  const printf = left.find((cmd) => cmd.program >= 0 && basename(cmd.argv[cmd.program]!.text) === "printf");
  const xargs = right.find((cmd) => cmd.program >= 0 && cmd.wrappers.includes("xargs"));
  if (!printf || !xargs) return [];
  const args = printf.argv.slice(printf.program + 1);
  if (args.length !== 2 || !/^%s(?:\\n)?$/.test(args[0]!.text)) return [];

  const options = xargs.argv.slice(0, xargs.program);
  let marker = "";
  for (let i = 0; i < options.length; i++) {
    const option = options[i]!.text;
    if (option === "-I" || option === "--replace") marker = options[i + 1]?.text ?? "";
    else if (option.startsWith("-I")) marker = option.slice(2);
    else if (option.startsWith("--replace=")) marker = option.slice("--replace=".length);
  }
  if (!marker) return [];
  const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;
  const source = xargs.argv
    .slice(xargs.program)
    .map((word) => (word.text.includes(marker) ? quote(word.text.replaceAll(marker, args[1]!.text)) : word.raw))
    .join(" ");
  return [{ source, cwd: xargs.cwd }];
}
