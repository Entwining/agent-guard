// App Data rules. macOS records a Files & Folders App Data entry whenever a
// process reads or enumerates another app's ~/Library data tree, so these deny
// those reads and the broad walks that reach them.
import { programName } from "../argv";
import { absPath, appdataTrees, isAppdata, isBroad, isLibrary } from "../paths";
import { reasons } from "../reasons";
import type { Command, Request } from "../record";

const { appdata: appdataReason, broad: broadReason } = reasons;

const trees = appdataTrees.join("|");
// These print or assign their arguments; only a glob the shell expands before
// they run reads a directory.
export const dataPrograms = new Set(["echo", "printf", "print", ":", "true", "false", "export", "set", "unset", "typeset", "declare", "local"]);
const noWalkPrograms = new Set(["mv", "stat", "test", "[", "mkdir", "dd"]);

export function appdataRules(req: Request): string[] {
  const denials: string[] = [];
  if ((req.operation === "read" || req.operation === "write") && isAppdata(req.target, req.home)) denials.push(appdataReason);
  if (req.operation === "search") {
    if (isAppdata(req.searchRoot, req.home)) denials.push(appdataReason);
    else if (isLibrary(req.searchRoot, req.home)) denials.push(broadReason);
  }
  for (const cmd of req.commands) {
    const reason = appdataCommand(cmd, req.home);
    if (reason) denials.push(reason);
  }
  const home = req.home.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const signature = new RegExp(`(~|\\$HOME|\\$\\{HOME\\}|${home})/Library/(${trees})`, "i");
  for (const fragment of req.uninspectable) if (signature.test(fragment)) denials.push(appdataReason);
  return denials;
}

function appdataCommand(cmd: Command, home: string): string | undefined {
  const cwd = cmd.cwd;
  const prog = cmd.program >= 0 ? programName(cmd.argv[cmd.program]!.text) : "";
  const data = dataPrograms.has(prog);
  // cd reads its target but walks nothing.
  const recursive = cmd.argv.slice(cmd.program + 1).some((word) => (prog === "ls" ? /^(--recursive$|-[^-]*R)/ : /^(--recursive$|-[^-]*[rR])/).test(word.text));
  const gitConfig = prog === "git" && cmd.argv.slice(cmd.program + 1).some((word) => word.text === "config");
  const walk = !data && !gitConfig && !["cd", "pushd", "popd"].includes(prog) && !noWalkPrograms.has(prog) && !(["ls", "cp"].includes(prog) && !recursive);
  const paths: { path: string; glob: boolean }[] = [];
  for (const [i, word] of cmd.argv.entries()) {
    if (!word.value || ["pattern", "code", "option:pattern"].includes(word.role)) continue;
    if (word.role === "program" && !word.value.includes("/")) continue;
    if (data && !word.globs && i > cmd.program) continue;
    // A value glued to its option, as in --env-file=PATH, is a path too.
    const text = prog === "dd" && i > cmd.program ? word.value.replace(/^(if|of)=/, "") : word.value;
    const values = text.startsWith("-") && text.includes("=") ? [text, text.slice(text.indexOf("=") + 1)] : [text];
    for (const value of values) {
      // An expansion the front end cannot resolve may well be $HOME.
      if (word.expands && new RegExp(`/Library/(${trees})(/.*)?$`, "is").test(value)) return appdataReason;
      paths.push({ path: absPath(value, cwd, home, /^['"]/.test(word.raw)), glob: word.globs });
    }
  }
  for (const r of cmd.redirects) {
    if ((r.direction === "in" || r.direction === "out") && r.target) paths.push({ path: absPath(r.target, cwd, home), glob: r.globs });
  }
  if (paths.some(({ path, glob }) => isAppdata(path, home, glob))) return appdataReason;
  // Shell glob expansion touches directories even when the command does not walk them.
  if (paths.some(({ path, glob }) => isBroad(path, home, glob) && (walk || glob))) return broadReason;
  if (cmd.program < 0) return undefined;

  // Path operands scope a walk away from the current directory; a search
  // tool's pattern is not one of them.
  let scoped = false;
  for (const word of cmd.argv.slice(cmd.program + 1)) {
    const text = word.text;
    if ((word.role === "arg" || word.role === "path") && !text.startsWith("-")) scoped = true;
  }
  if (prog === "ls" && !scoped && isAppdata(cwd, home)) return appdataReason;
  // rg and fd also read ignore files from cwd when given a path operand.
  let denied = false;
  if (["find", "du", "tree"].includes(prog)) denied = !scoped && (isBroad(cwd, home) || isAppdata(cwd, home));
  else if (prog === "ls" || prog === "grep") denied = !scoped && recursive && isBroad(cwd, home);
  else if (["rg", "fd", "ag", "ack"].includes(prog)) denied = !scoped && (isBroad(cwd, home) || isLibrary(cwd, home));
  return denied ? broadReason : undefined;
}
