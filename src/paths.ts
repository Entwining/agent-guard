// Filesystem checks run after lexical denials to avoid touching protected trees.
import { realpathSync, statSync } from "node:fs";
import { userInfo } from "node:os";
import { basename, dirname, resolve } from "node:path";

export const appdataTrees = ["Containers", "Group Containers", "Mobile Documents", "CloudStorage"];

// The one list of credential-bearing paths, matched against absolute paths.
export const sensitivePaths = [
  "**/.env",
  "**/.env.*",
  "**/.npmrc",
  "**/.zprofile*",
  "**/.zsh_history*",
  "**/*.pem",
  "**/*.key",
  "**/auth.json*",
  "**/.credentials.json*",
  "**/.aws/credentials*",
  "**/private-keys-v1.d",
  "**/private-keys-v1.d/**",
];

const sensitiveGlobs = sensitivePaths.map((path) => new Bun.Glob(path));

// Directories that hold listed files, so a search rooted at one reads them.
// ~/.ssh has its own inode-based check and reason.
const credentialRoots = [".aws", ".gnupg"];
const credentialDirectories = [".ssh", ...credentialRoots];

export function expandHome(path: string, home: string): string {
  for (const prefix of ["~", `~${userInfo().username}`]) {
    if (path === prefix || path.startsWith(`${prefix}/`)) return home + path.slice(prefix.length);
  }
  return path;
}

// curl, open and git read a file:// URL as the path it names.
export function absPath(path: string, cwd: string, home: string, quoted = false): string {
  if (!quoted) path = expandHome(path, home);
  return resolve(cwd, path.replace(/^file:\/\//i, ""));
}

// Whether the shell could expand the glob's leading segments to `directory`, so
// `~/Lib*/Cont*/x` reads inside `~/Library/Containers`. A pattern that stops at
// the directory only names it and is left to the callers' other checks.
function globReaches(path: string, directory: string): boolean {
  const pattern = path.split("/");
  const target = directory.split("/");
  if (pattern.length <= target.length) return false;
  return target.every((segment, i) => i === 0 || pattern[i] === "**" || new Bun.Glob(pattern[i]!.toLowerCase()).match(segment.toLowerCase()));
}

export function isAppdata(path: string, home: string, glob = false): boolean {
  if (glob) {
    const expanded = Bun.$.braces(path);
    if (expanded.length > 1) return expanded.some((each) => isAppdata(each, home, true));
    if (appdataTrees.some((tree) => globReaches(path, `${home}/Library/${tree}`))) return true;
  }
  const library = `${home}/Library/`.toLowerCase();
  if (!path.toLowerCase().startsWith(library)) return false;
  const rest = path.slice(library.length).toLowerCase();
  if (appdataTrees.some((tree) => rest === tree.toLowerCase() || rest.startsWith(`${tree.toLowerCase()}/`))) return true;
  if (!glob) return false;
  const fixed = rest.split(/[*?[]/)[0]!.replace(/\/$/, "");
  return fixed !== "" && appdataTrees.some((tree) => tree.toLowerCase().startsWith(fixed));
}

export function isBroad(path: string, home: string, glob = false): boolean {
  if (glob) {
    const expanded = Bun.$.braces(path);
    if (expanded.length > 1) return expanded.some((each) => isBroad(each, home, true));
  }
  path = path.toLowerCase();
  home = home.toLowerCase();
  if (path === "/") return true;
  const trimmed = path.replace(/\/$/, "");
  if (trimmed === home || trimmed === `${home}/library` || home.startsWith(`${trimmed}/`)) return true;
  if (!glob) return false;
  const pattern = new Bun.Glob(path);
  if ([home, `${home}/library`, ...appdataTrees.flatMap((tree) => [`${home}/library/${tree.toLowerCase()}`, `${home}/library/${tree.toLowerCase()}/x`])].some((candidate) => pattern.match(candidate)))
    return true;
  const prefix = path.split(/[*?[]/, 1)[0]!.replace(/\/$/, "");
  return path.includes("**") && (prefix === home || prefix === `${home}/library` || home.startsWith(`${prefix}/`));
}

export function isLibrary(path: string, home: string): boolean {
  return path.toLowerCase() === `${home}/library`.toLowerCase() || isAppdata(path, home);
}

const sshPublic = (name: string) => /^(config|config\..*|.*\.pub|allowed_signers|known_hosts.*)$/s.test(name);

// Under ~/.ssh only a top-level client config, public key, allowed_signers or
// known_hosts file is public.
export function sshPrivate(path: string): boolean {
  const at = path.lastIndexOf("/.ssh/");
  if (at < 0 || at + 6 >= path.length) return false;
  const rest = path.slice(at + 6);
  return rest.includes("/") || !sshPublic(rest);
}

// A credential-bearing absolute path, after brace expansion. For an unquoted
// glob, a pattern counts when it could match a listed name; a bare wildcard
// does not, unless it sits in a credential directory. Names match regardless of
// case because the default APFS volume ignores it.
export function isSensitive(path: string, glob = false): boolean {
  const expanded = Bun.$.braces(path);
  if (expanded.length > 1) return expanded.some((each) => isSensitive(each, glob));
  const lower = path.toLowerCase();
  if ([".env.example", ".env.age"].includes(basename(lower))) return false;
  if (sshPrivate(path) || sensitiveGlobs.some((listed) => listed.match(lower))) return true;
  if (!glob) return false;
  if (globDirectory(path)) return true;
  const base = basename(lower);
  if (/^[*?]*$/.test(base)) return credentialDirectories.includes(basename(dirname(lower)));
  const pattern = new Bun.Glob(base);
  return sensitivePaths.some((listed) => {
    const name = basename(listed).replaceAll("*", "x");
    return !/^x*$/.test(name) && pattern.match(name);
  });
}

// A wildcard directory segment such as `.s*` may expand to a credential
// directory. The shell's wildcards skip a leading dot, so only a segment that
// starts with one counts.
function globDirectory(path: string): boolean {
  const segments = path.split("/");
  return segments.slice(0, -1).some((segment, i) => {
    if (!segment.startsWith(".") || !/[*?[]/.test(segment)) return false;
    const pattern = new Bun.Glob(segment.toLowerCase());
    return credentialDirectories.some((dir) => pattern.match(dir) && isSensitive([...segments.slice(0, i), dir, ...segments.slice(i + 1)].join("/"), true));
  });
}

// A search reads everything under its root, so a credential directory counts as
// its listed files do.
export function isSensitiveRoot(path: string): boolean {
  return isSensitive(path) || credentialRoots.includes(basename(path.toLowerCase()));
}

// A path that does not exist or cannot be searched has no inode to compare.
function stat(path: string) {
  try {
    return statSync(path);
  } catch {
    return undefined;
  }
}

function sameFile(a: string, b: string): boolean {
  if (a === b) return true;
  const x = stat(a);
  const y = stat(b);
  return x !== undefined && y !== undefined && x.dev === y.dev && x.ino === y.ino;
}

function real(path: string): string {
  try {
    return realpathSync(path);
  } catch {
    return path;
  }
}

function kind(path: string): "dir" | "file" | "other" {
  const st = stat(path);
  return st?.isDirectory() ? "dir" : st?.isFile() ? "file" : "other";
}

// Filesystem check for a file tool target, or a search root when search is
// set. It follows symlinks and compares inodes so a link or a case alias of
// ~/.ssh resolves to the directory it names.
export function sshScopeDenied(target: string, home: string, search: boolean): boolean {
  const ssh = `${home}/.ssh`;
  const candidates = [...new Set([target, real(target)])];
  const roots = [...new Set([ssh, real(ssh)])];
  for (const candidate of candidates) {
    for (const root of roots) {
      if (sameFile(candidate, root)) return true;
      if (search) {
        for (let parent = root; parent !== "/";) {
          parent = dirname(parent);
          if (sameFile(candidate, parent)) return true;
        }
      }
      for (let parent = candidate; parent !== "/";) {
        parent = dirname(parent);
        if (!sameFile(parent, root)) continue;
        if (parent !== dirname(candidate) || !sshPublic(basename(candidate))) return true;
        if (kind(target) === "dir") return true;
        if (search && kind(target) !== "file") return true;
      }
    }
  }
  return false;
}
