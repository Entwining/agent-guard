// Workflow rules differ by runtime; the safety rules do not.
// Codex: run claude through the provider-aware launcher function, which picks
// the persisted provider mode before gateway credentials are expanded.
import { basename } from "node:path";
import { reasons } from "../reasons.ts";
import type { Request } from "../record.ts";

const {
  find: findReason, replace: replaceReason, include: includeReason,
  bre: breReason, launcher: launcherReason,
} = reasons;

export function claudeWorkflowRules(req: Request): string[] {
  const denials: string[] = [];
  for (const cmd of req.commands) {
    if (cmd.program < 0) continue;
    const name = basename(cmd.argv[cmd.program].text);
    if (name === "find") denials.push(findReason);
    if (name !== "rg") continue;
    if (cmd.flags.has("replace")) denials.push(replaceReason);
    if (cmd.flags.has("include")) denials.push(includeReason);
    if (cmd.flags.has("fixed")) continue;
    for (const w of cmd.argv.slice(cmd.program + 1)) {
      if ((w.role === "pattern" || w.role === "option:pattern") && /(^|[^\\])\\\|/.test(w.value)) denials.push(breReason);
    }
  }
  return denials;
}

export function codexWorkflowRules(req: Request): string[] {
  const denials: string[] = [];
  for (const cmd of req.commands) {
    if (cmd.program < 0) continue;
    const program = cmd.argv[cmd.program].text;
    if (basename(program) === "claude" && (program.includes("/") || cmd.wrappers.length)) denials.push(launcherReason);
  }
  return denials;
}
