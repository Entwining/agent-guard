import { parseScript } from "./frontend";
import { linkedTargets } from "./links";
import { absPath } from "./paths";
import { reasons } from "./reasons";
import type { Request, Runtime, Tool } from "./record";
import { appdataRules } from "./rules/appdata";
import { credentialFilesystemRules, credentialRules } from "./rules/credentials";
import { claudeWorkflowRules } from "./rules/workflow";
import { extractTargets } from "./targets";

export function buildRequest(runtime: Runtime, tool: Tool, cwd: string, input: string, glob: string, home: string): Request {
  const inputCwd = cwd || "/";
  const req: Request = {
    runtime,
    tool,
    home,
    cwd: absPath(inputCwd, "/", home),
    inputCwd,
    pathInput: input,
    operation: "",
    target: "",
    searchRoot: "",
    glob,
    commands: [],
    uninspectable: [],
    parseFailed: false,
  };
  if (tool === "bash") {
    if (input) Object.assign(req, parseScript(input, inputCwd, home));
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
  if (req.parseFailed) return reasons.syntax;
  const targets = extractTargets(req);
  const denials = [...appdataRules(req, targets), ...credentialRules(req, targets)];
  // Deny lexically protected paths before asking the filesystem about links.
  if (!denials.length) {
    try {
      const linked = linkedTargets(targets, req.home);
      if (linked) denials.push(...appdataRules(req, linked), ...credentialRules(req, linked));
    } catch {
      denials.push(reasons.symlink);
    }
  }
  if (!denials.length) denials.push(...credentialFilesystemRules(req, targets));
  return denials[0];
}

export function suggestions(req: Request): string[] {
  return req.runtime === "claude" ? claudeWorkflowRules(req) : [];
}
