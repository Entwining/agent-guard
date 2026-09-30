// Credential rules: keep credential files and private keys out of what the
// model reads. Clients the program table models as consuming a credential file
// themselves (dotenvx, node --env-file, ssh -i) are not readers and stay
// allowed; the guard does not control what they do with the contents.
import { isSensitive, isSensitiveRoot, sshPrivate } from "../filesystem/paths";
import { sshScopeDenied } from "../filesystem/ssh";
import { reasons } from "../reasons";
import type { Request, Target } from "../record";
import { secretReasons, secretSignatures } from "./secrets";

const { file: fileReason, codeFile: codeFileReason, upload: uploadReason, ssh: sshReason, grepSsh: grepSshReason, hiddenSearch: hiddenSearchReason } = reasons;

function targetReason(target: Target, home: string): string | undefined {
  const reason = target.via === "code" ? codeFileReason : target.sends ? uploadReason : fileReason;
  if (target.effect === "write") return sshPrivate(target.path) ? sshReason : undefined;
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
  for (const fragment of req.uninspectable) denials.push(...secretSignatures(fragment.text));
  return denials;
}

// The core runs this last: it compares inodes, so it touches the filesystem.
export function credentialFilesystemRules(req: Request, targets: Target[]): string[] {
  for (const target of targets) {
    if (!["read", "write", "list"].includes(target.effect) || target.via === "items" || (target.via === "tool" && target.glob)) continue;
    // A working directory is checked only for a search that reads what is under it.
    if ((target.via === "cwd" || target.via === "scan") && !target.search) continue;
    if (sshScopeDenied(target.path, req.home, target.search, !target.expands)) return [target.search ? grepSshReason : sshReason];
  }
  return [];
}
