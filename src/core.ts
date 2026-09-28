import { parseScript } from "./frontend.ts";
import { linkedRequest } from "./links.ts";
import { absPath } from "./paths.ts";
import { reasons } from "./reasons.ts";
import type { Request, Runtime, Tool } from "./record.ts";
import { appdataRules } from "./rules/appdata.ts";
import { credentialFilesystemRules, credentialRules } from "./rules/credentials.ts";
import { claudeWorkflowRules, codexWorkflowRules } from "./rules/workflow.ts";

export function buildRequest(runtime: Runtime, tool: Tool, cwd: string, input: string, glob: string, home: string): Request {
  const req: Request = { runtime, tool, home, cwd: absPath(cwd || "/", "/", home), operation: "", target: "", searchRoot: "", glob, commands: [], uninspectable: [] };
  if (tool === "bash") {
    if (input) Object.assign(req, parseScript(input, req.cwd, home));
  } else if (tool === "grep") {
    req.operation = "search";
    req.searchRoot = absPath(input || req.cwd, req.cwd, home);
  } else if (input) {
    req.operation = tool === "read" ? "read" : "write";
    req.target = absPath(input, req.cwd, home);
  }
  return req;
}

export function evaluate(req: Request): string | undefined {
  const denials = [
    ...appdataRules(req),
    ...credentialRules(req),
  ];
  // Deny lexically protected paths before asking the filesystem about links.
  if (!denials.length) {
    try {
      const linked = linkedRequest(req);
      if (linked) denials.push(...appdataRules(linked), ...credentialRules(linked));
    } catch {
      denials.push(reasons.symlink);
    }
  }
  if (!denials.length) denials.push(...credentialFilesystemRules(req));
  return denials[0];
}

export function suggestions(req: Request): string[] {
  return req.runtime === "claude" ? claudeWorkflowRules(req) : req.runtime === "codex" ? codexWorkflowRules(req) : [];
}
