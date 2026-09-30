// Turn a request into inferred targets under modelled command semantics, each with what the command is modelled to do to it.
import { basename } from "node:path";

import { absPath, expandHome } from "../filesystem/paths";
import type { Command, Effect, Request, Target, Word } from "../record";
import { programName } from "../shell/argv";
import { type Context, DEFAULT_EFFECT, dataPrograms, globalOptions, specFor, specs, walkOf } from "./programs";

const pathRoles = new Set(["arg", "path", "patfile", "option:patfile", "optarg"]);

function maker(home: string, cmd: Command, command: number, walk: Target["walk"], sends: boolean): Context["make"] {
  return (path, word, effect, options = {}) => {
    const quoted = options.quoted ?? (word ? /^['"]/.test(word.raw) : false);
    const input = (quoted ? path : expandHome(path, home)).replace(/^file:\/\//i, "");
    const base = options.base ?? cmd.cwd;
    return {
      path: absPath(path, base, home, quoted),
      unresolved: input.startsWith("/") ? input : `${base}/${input}`,
      glob: options.glob ?? word?.globs ?? false,
      effect,
      walk: options.walk ?? walk,
      sends: options.sends ?? sends,
      expands: options.expands ?? word?.expands ?? false,
      via: options.via ?? "operand",
      search: options.search ?? false,
      command,
    };
  };
}

// A glued `--name=value` is a path in the value, and `@path` or an httpie `field=@path` reads the file; a bare option is not a path.
function operandValue(word: Word): string | undefined {
  const value = word.value.startsWith("-") ? (word.value.includes("=") ? word.value.slice(word.value.indexOf("=") + 1) : undefined) : word.value;
  if (value === undefined) return undefined;
  if (value.startsWith("@")) return value.slice(1) || undefined;
  return /^[^=@\s]+?(?:==?|:)@(.+)$/s.exec(value)?.[1] ?? value;
}

function commandTargets(cmd: Command, command: number, home: string): Target[] {
  const program = cmd.argv[cmd.program];
  const name = program ? programName(program.text) : "";
  const spec = specFor(name);
  // A command with no program, such as a for loop's word list, still names paths.
  const words = cmd.argv.slice(cmd.program + 1);
  const walk = walkOf(spec, name, words);
  const options = { ...globalOptions, ...spec.options };
  // An option's value takes the option's effect, glued with = or in the next word; the last letter of a cluster such as -lf takes the value.
  const optionEffect = (i: number): Effect | undefined => {
    const word = words[i]!;
    const previous = words[i - 1]?.text ?? "";
    const option = word.value.startsWith("-") ? (word.value.includes("=") ? word.value.slice(0, word.value.indexOf("=")) : "") : /^-[^-]/.test(previous) ? `-${previous.at(-1)}` : previous;
    return options[option];
  };
  // A short option takes the rest of its word as the value: `-idata`, and `-vidata` after flags in a cluster.
  const glued = (word: Word): { value: string; effect: Effect } | undefined => {
    if (!/^-[^-]/.test(word.text)) return undefined;
    const letters = [...word.text.slice(1)];
    const at = letters.findIndex((letter) => options[`-${letter}`] !== undefined);
    return at >= 0 && at < letters.length - 1 ? { value: word.text.slice(at + 2), effect: options[`-${letters[at]}`]! } : undefined;
  };
  const operandWords = words.filter((word, i) => !word.text.startsWith("-") && optionEffect(i) === undefined);
  // With `-t DIR`, alone or in a cluster, every operand is a source and the directory is the destination. A glob expands to several words, so it may hide sources.
  const last = operandWords.at(-1);
  const intoDirectory =
    spec.options?.["-t"] === "write" && words.some((word) => /^-[^-]*t/.test(word.text) || (/^--t[a-z-]*(=|$)/.test(word.text) && "--target-directory".startsWith(word.text.split("=")[0]!)));
  const destination = spec.last && !last?.globs && !intoDirectory ? last : undefined;
  const remote = (word: Word) => spec.remote?.test(word.value) ?? false;
  // A copy sends what it reads only when it names another machine; a local copy keeps its reads on this one.
  const sends = (spec.sends ?? false) && (!spec.remote || operandWords.some(remote));
  const make = maker(home, cmd, command, walk, sends);
  const ctx: Context = { cmd, words, walk, claimed: new Set(), make };
  // The word list of a for loop or a [[ test is not read by the shell.
  const operands: Effect = spec.operands ?? (program ? DEFAULT_EFFECT : "use");
  const targets: Target[] = [];
  for (const redirect of cmd.redirects) {
    if ((redirect.direction === "in" || redirect.direction === "out") && redirect.target)
      targets.push(make(redirect.target, undefined, redirect.direction === "in" ? "read" : "write", { via: "redirect", glob: redirect.globs, expands: redirect.expands }));
  }
  if (cmd.items && program) targets.push(make(cmd.items.root, undefined, operands, { via: "items", glob: false, walk: cmd.items.hidden ? "hidden" : "visible" }));
  // xargs reads its arguments from the -a file.
  if (cmd.wrappers.includes("xargs") && program) {
    const options = cmd.argv.slice(0, cmd.program);
    for (const [i, word] of options.entries()) {
      const file = word.text === "-a" || word.text === "--arg-file" ? options[i + 1] : undefined;
      if (file) targets.push(make(file.text, file, "read", { via: "option" }));
      else if (word.text.startsWith("--arg-file=")) targets.push(make(word.text.slice("--arg-file=".length), word, "read", { via: "option" }));
    }
  }
  if (program?.value.includes("/")) targets.push(make(program.value, program, "use", { via: "option" }));
  const start = targets.length;
  targets.push(...(spec.targets?.(ctx) ?? []));
  for (const [i, word] of words.entries()) {
    const short = ctx.claimed.has(word) ? undefined : glued(word);
    if (short) {
      targets.push(make(short.value, word, short.effect, { via: "option" }));
      continue;
    }
    const value = pathRoles.has(word.role) && !ctx.claimed.has(word) ? operandValue(word) : undefined;
    if (!value) continue;
    const effect = remote(word) ? "name" : word === destination ? spec.last! : (optionEffect(i) ?? (word.role === "optarg" ? "use" : operands));
    targets.push(make(value, word, effect, { via: word.role === "optarg" ? "option" : "operand" }));
  }
  const lists = spec.cwd && !targets.slice(start).some((target) => target.via === "operand");
  if (lists) targets.push(make(cmd.cwd, undefined, "list", { via: spec.cwd! }));
  // A program runs in its working directory, which App Data records even when the command names nothing,
  // or when it is a program the table does not model and may read what it does not name.
  const named = targets.some((target) => ["operand", "cwd", "scan"].includes(target.via) && !["enter", "name"].includes(target.effect));
  if (program && !dataPrograms.includes(name) && !["cd", "pushd", "popd"].includes(name) && (!named || !specs.has(name))) targets.push(make(cmd.cwd, undefined, "enter", { via: "cwd", walk: "none" }));
  return targets;
}

// A word that starts like a path, or touches a quote: `e.key` is a property, `'.key'` and `"cert.pem"` are files. Quotes are not paired
// across lines, so an apostrophe in a comment does not hide the string literals after it; a string literal is a shell command that can name a file
// anywhere in it, so its words count wherever they sit.
function codeTokens(code: string): string[] {
  const words = (text: string) => [...text.matchAll(/[\w.~/-]+/g)];
  const strings = [...code.matchAll(/(['"`])((?:(?!\1)[^\n])*)\1/g)].flatMap((match) => words(match[2]!));
  return [...words(code).filter((match) => /^[.~]|\//.test(match[0]) || /['"`]/.test(code[match.index - 1] ?? "") || /['"`]/.test(code[match.index + match[0].length] ?? "")), ...strings].map(
    (match) => match[0],
  );
}

export function extractTargets(req: Request): Target[] {
  const targets: Target[] = [];
  const add = (path: string, cwd: string, inputCwd: string, effect: Effect, options: Partial<Target>) => {
    const input = expandHome(path, req.home);
    targets.push({
      path: absPath(path, cwd, req.home),
      unresolved: input.startsWith("/") ? input : `${inputCwd}/${input}`,
      glob: false,
      effect,
      walk: "none",
      sends: false,
      expands: false,
      via: "tool",
      search: false,
      command: -1,
      ...options,
    });
  };
  const tool = (path: string, effect: Effect, options: Partial<Target> = {}) => add(path, req.cwd, req.inputCwd, effect, options);
  if (req.operation === "read" || req.operation === "write") tool(req.pathInput, req.operation === "read" ? "read" : "write");
  if (req.operation === "search") {
    tool(req.pathInput || req.inputCwd, "read", { walk: "visible", search: true });
    if (req.glob && !req.glob.startsWith("!")) tool(`${req.searchRoot}/${basename(req.glob)}`, "read", { glob: true });
  }
  req.commands.forEach((cmd, command) => targets.push(...commandTargets(cmd, command, req.home)));
  // Inline code opens files the guard cannot trace, so each token that names a path is inferred to be a read target.
  for (const { text, cwd } of req.uninspectable) for (const token of codeTokens(text)) add(token, cwd, cwd, "read", { via: "code" });
  return targets;
}
