// Turn a request into the paths it touches, each with what the command does to it.
import { basename } from "node:path";

import { programName } from "./argv";
import { absPath, expandHome } from "./paths";
import { type Context, DEFAULT_EFFECT, dataPrograms, globalOptions, specFor, specs, walkOf } from "./programs";
import type { Command, Effect, Request, Target, Word } from "./record";

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
      expands: word?.expands ?? false,
      via: options.via ?? "operand",
      search: options.search ?? false,
      command,
    };
  };
}

// A glued `--name=value` is a path in the value; a bare option is not.
function operandValue(word: Word): string | undefined {
  if (!word.value.startsWith("-")) return word.value;
  return word.value.includes("=") ? word.value.slice(word.value.indexOf("=") + 1) : undefined;
}

function commandTargets(cmd: Command, command: number, home: string): Target[] {
  const program = cmd.argv[cmd.program];
  const name = program ? programName(program.text) : "";
  const spec = specFor(name);
  // A command with no program, such as a for loop's word list, still names paths.
  const words = cmd.argv.slice(cmd.program + 1);
  const walk = walkOf(spec, name, words);
  const operandWords = words.filter((word) => !word.text.startsWith("-"));
  const destination = spec.last ? operandWords.at(-1) : undefined;
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
      targets.push(make(redirect.target, undefined, redirect.direction === "in" ? "read" : "write", { via: "redirect", glob: redirect.globs }));
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
  const options = { ...globalOptions, ...spec.options };
  for (const [i, word] of words.entries()) {
    const value = pathRoles.has(word.role) && !ctx.claimed.has(word) ? operandValue(word) : undefined;
    if (!value) continue;
    // An option's value takes the option's effect, glued with = or in the next word.
    const previous = words[i - 1]?.text ?? "";
    // The last letter of a cluster such as -lf takes the value.
    const option = word.value.startsWith("-") ? word.value.slice(0, word.value.indexOf("=")) : /^-[^-]/.test(previous) ? `-${previous.at(-1)}` : previous;
    const effect = remote(word) ? "name" : word === destination ? spec.last! : (options[option] ?? (word.role === "optarg" ? "use" : operands));
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
  // Inline code opens files the guard cannot trace, so each token that names a path counts as a read.
  for (const { text, cwd } of req.uninspectable) for (const token of text.split(/[^\w.\/~-]+/).filter(Boolean)) add(token, cwd, cwd, "read", { via: "code" });
  return targets;
}
