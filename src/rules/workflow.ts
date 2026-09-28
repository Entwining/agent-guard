import { basename } from "node:path";

import { reasons } from "../reasons";
import type { Request } from "../record";

const { replace: replaceReason, include: includeReason, bre: breReason } = reasons;

export function claudeWorkflowRules(req: Request): string[] {
  const advice: string[] = [];
  for (const cmd of req.commands) {
    if (cmd.program < 0) continue;
    const name = basename(cmd.argv[cmd.program]!.text);
    if (name !== "rg") continue;
    if (cmd.flags.has("replace")) advice.push(replaceReason);
    if (cmd.flags.has("include")) advice.push(includeReason);
    if (cmd.flags.has("fixed")) continue;
    for (const w of cmd.argv.slice(cmd.program + 1)) {
      if ((w.role === "pattern" || w.role === "option:pattern") && /(^|[^\\])\\\|/.test(w.value)) advice.push(breReason);
    }
  }
  return advice;
}
