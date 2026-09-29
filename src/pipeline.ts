import { basename } from "node:path";

import { stdinKind } from "./argv";
import type { Command, Word } from "./record";
import { showsHidden } from "./search-roles";

// A dotfile name, or a glob the shell expands to dotfiles; `.` and `..` are directories.
const hiddenName = (text: string) => /^\.(?!\.?$)/.test(basename(text));

// xargs passes the names a walk printed to its command, so mark that command.
export function markWalkedInput(left: Command[], right: Command[]) {
  const walker = left.find((cmd) => {
    const name = cmd.program >= 0 ? basename(cmd.argv[cmd.program]!.text) : "";
    const args = cmd.argv.slice(cmd.program + 1);
    const operands = args.filter((word) => !word.text.startsWith("-"));
    if (name === "ls") return args.some((word) => /^(--all|--almost-all|-[A-Za-z0-9]*[aA][A-Za-z0-9]*)$/.test(word.text)) || operands.some((word) => hiddenName(word.text));
    if (name === "echo" || name === "printf") return operands.some((word) => word.globs && hiddenName(word.text));
    return name === "find" || (name === "fd" && showsHidden(args));
  });
  if (walker) for (const cmd of right) if (cmd.wrappers.includes("xargs")) cmd.items = { root: walker.cwd, hidden: true };
}

// printf and zsh's echo decode these escapes and stop at `\c`; a NUL separates xargs -0 items, so it reads as a line break.
// A printf format takes up to three octal digits after the backslash; %b and echo take a leading 0 as well.
const escapes: Record<string, string> = { n: "\n", t: "\t", "\\": "\\" };
const unescape = (text: string, operand = false) =>
  text
    .split("\\c", 1)[0]!
    .replace(
      new RegExp(`\\\\(?:x([0-9a-fA-F]{1,2})|(${operand ? "0[0-7]{0,3}|[1-7][0-7]{0,2}" : "[0-7]{1,3}"})|([nt\\\\])|u([0-9a-fA-F]{1,4})|U([0-9a-fA-F]{1,8}))`, "g"),
      (_, hex?: string, octal?: string, named?: string, unicode?: string, wide?: string) => {
        const hexadecimal = hex ?? unicode ?? wide;
        const code = hexadecimal !== undefined ? parseInt(hexadecimal, 16) : octal !== undefined ? parseInt(octal, 8) : undefined;
        return code === undefined ? escapes[named!]! : code === 0 ? "\n" : String.fromCharCode(code);
      },
    );

// What printf writes for a format built from literal text, %s, %b and %%; another conversion is unknown.
function printfOutput(args: Word[]): string | undefined {
  const [format, ...operands] = (args[0]?.text === "--" ? args.slice(1) : args).map((word) => word.text);
  if (format === undefined) return "";
  const pieces = format.split(/(%[sb%])/);
  if (pieces.some((piece) => piece.includes("%") && !/^%[sb%]$/.test(piece))) return undefined;
  const converts = pieces.some((piece) => /^%[sb]$/.test(piece));
  let out = "";
  do {
    for (const piece of pieces) {
      if (piece === "%%") out += "%";
      else if (piece === "%s") out += operands.shift() ?? "";
      else if (piece === "%b") out += unescape(operands.shift() ?? "", true);
      else out += unescape(piece);
    }
  } while (converts && operands.length);
  return out;
}

// A literal echo or printf value piped into a shell is the command line it runs.
export function shellInput(left: Command[], right: Command[]): { source: string; cwd: string }[] {
  const producer = left.find((cmd) => cmd.program >= 0 && ["printf", "echo"].includes(basename(cmd.argv[cmd.program]!.text)));
  const shell = right.find((cmd) => cmd.program >= 0 && stdinKind(cmd) === "shell");
  if (!producer || !shell) return [];
  const args = producer.argv.slice(producer.program + 1);
  const source =
    basename(producer.argv[producer.program]!.text) === "printf"
      ? printfOutput(args)
      : unescape(
          args
            .filter((word) => !word.text.startsWith("-"))
            .map((word) => word.text)
            .join(" "),
          true,
        );
  return source === undefined ? [] : [{ source, cwd: shell.cwd }];
}

// xargs runs its command once with every item, or once per item with -I.
function xargsCommands(xargs: Command, items: string[]): { source: string; cwd: string }[] {
  const options = xargs.argv.slice(0, xargs.program);
  let marker = "";
  for (let i = 0; i < options.length; i++) {
    const option = options[i]!.text;
    if (option === "-I" || option === "--replace") marker = options[i + 1]?.text ?? "";
    else if (option.startsWith("-I")) marker = option.slice(2);
    else if (option.startsWith("--replace=")) marker = option.slice("--replace=".length);
  }
  // xargs strips quotes and backslashes from what it reads, with or without -I.
  const stripped = (value: string) => value.replace(/["'\\]/g, "");
  const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;
  const command = xargs.argv.slice(xargs.program);
  // Without -I, xargs splits its input on blanks, so a printed line can become several arguments.
  if (!marker)
    return [
      {
        source: [
          ...command.map((word) => word.raw),
          ...items
            .flatMap((item) => item.split(/\s+/))
            .filter(Boolean)
            .flatMap((item) => [item, stripped(item)])
            .map(quote),
        ].join(" "),
        cwd: xargs.cwd,
      },
    ];
  return items
    .flatMap((line) => [line, stripped(line)])
    .map((item) => ({ source: command.map((word) => (word.text.includes(marker) ? quote(word.text.replaceAll(marker, item)) : word.raw)).join(" "), cwd: xargs.cwd }));
}

// Literal printf and echo values are known before xargs passes them to a command.
export function xargsReplacements(left: Command[], right: Command[]): { source: string; cwd: string }[] {
  const producer = left.find((cmd) => cmd.program >= 0 && ["printf", "echo"].includes(basename(cmd.argv[cmd.program]!.text)));
  const xargs = right.find((cmd) => cmd.program >= 0 && cmd.wrappers.includes("xargs"));
  if (!producer || !xargs) return [];
  const args = producer.argv.slice(producer.program + 1);
  if (basename(producer.argv[producer.program]!.text) !== "printf")
    return xargsCommands(
      xargs,
      args.filter((word) => !word.text.startsWith("-")).map((word) => unescape(word.text, true)),
    );
  const output = printfOutput(args);
  return output === undefined ? [] : xargsCommands(xargs, output.split("\n").filter(Boolean));
}

// A here-string or here-document body is the item list xargs reads.
export function xargsHereInput(xargs: Command, body: string): { source: string; cwd: string }[] {
  return xargsCommands(
    xargs,
    body
      .split("\n")
      .flatMap((line) => line.split(/\s+/))
      .filter(Boolean),
  );
}
