// Credential rules: keep credential files and private keys out of what the
// model reads. Clients that consume a credential file themselves (dotenvx, node
// --env-file, ssh -i) are not readers and stay allowed.
import { isSensitive, isSensitiveRoot, sshPrivate, sshScopeDenied } from "../paths";
import { readers } from "../programs";
import { reasons } from "../reasons";
import type { Request, Target } from "../record";
import { secretReasons, secretSignatures } from "./secrets";

const { file: fileReason, upload: uploadReason, ssh: sshReason, grepSsh: grepSshReason, hiddenSearch: hiddenSearchReason } = reasons;

function targetReason(target: Target, home: string): string | undefined {
  const reason = target.sends ? uploadReason : fileReason;
  if (target.effect === "write") {
    // A destination that is itself a listed file.
    if (target.via === "operand" && isSensitive(target.path, target.glob)) return reason;
    return sshPrivate(target.path) ? sshReason : undefined;
  }
  if (target.effect !== "read") return undefined;
  if (target.walk === "hidden") return hiddenSearchReason;
  // A directory that holds a listed file is read whole.
  return isSensitive(target.path, target.glob) || (!target.glob && isSensitiveRoot(target.path, home)) ? reason : undefined;
}

export function credentialRules(req: Request, targets: Target[]): string[] {
  const denials: string[] = [];
  const groups = Map.groupBy(targets, (target) => target.command);
  const judge = (command: number) => {
    for (const target of groups.get(command) ?? []) {
      const reason = targetReason(target, req.home);
      if (reason) denials.push(reason);
    }
  };
  judge(-1);
  for (const [i, cmd] of req.commands.entries()) {
    judge(i);
    denials.push(...secretReasons(cmd));
  }
  for (const fragment of req.uninspectable) denials.push(...secretSignatures(fragment), ...readerSignatures(fragment));
  return denials;
}

// The core runs this last: it follows symlinks, so it touches the filesystem.
export function credentialFilesystemRules(req: Request, targets: Target[]): string[] {
  for (const target of targets) {
    if (!["read", "write", "list"].includes(target.effect) || target.via === "items" || (target.via === "tool" && target.glob)) continue;
    // A working directory is checked only for a search that reads what is under it.
    if ((target.via === "cwd" || target.via === "scan") && !target.search) continue;
    if (sshScopeDenied(target.path, req.home, target.search)) return [target.search ? grepSshReason : sshReason];
  }
  return [];
}

const names = "\\.env(?:\\.[A-Za-z0-9_.-]+)?|\\.npmrc|\\.zsh_history|\\.zprofile|private-keys-v1\\.d|\\S*\\.(pem|key)|auth\\.json|\\.credentials\\.json|\\.aws/credentials|\\.ssh/\\S*";
const readerSignature = new RegExp(`(^|[^A-Za-z0-9_])(${readers.filter((p) => p !== ".").join("|")})([^A-Za-z0-9_-].*)?[\\s/"'=](${names})($|[\\s/"'>|&)\`])`, "s");

// Interpreter code, case bodies, or source the parser rejects: a reader
// command that stands next to a credential path.
function readerSignatures(fragment: string): string[] {
  return fragment
    .replace(/&&|\|\||\n/g, ";")
    .split(";")
    .filter((segment) => readerSignature.test(segment))
    .map(() => fileReason);
}
