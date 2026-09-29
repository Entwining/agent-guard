import { basename, dirname } from "node:path";

import { followLinks, notALink } from "./links";
import { isAppdata, sshPublic } from "./paths";
import { probes } from "./probes";

// A path that does not exist has no inode to compare. Any other failure means the comparison did not run, so it propagates.
function stat(path: string) {
  try {
    return probes.stat(path);
  } catch (error) {
    if (notALink.includes((error as NodeJS.ErrnoException).code ?? "")) return undefined;
    throw error;
  }
}

function sameFile(a: string, b: string): boolean {
  if (a === b) return true;
  const x = stat(a);
  const y = stat(b);
  return x !== undefined && y !== undefined && x.dev === y.dev && x.ino === y.ino;
}

function kind(path: string): "dir" | "file" | "other" {
  const st = stat(path);
  return st?.isDirectory() ? "dir" : st?.isFile() ? "file" : "other";
}

// Whether the spelling puts a path at or under `root`, or for a search above it. Case is ignored because the default APFS volume ignores it.
function near(path: string, root: string, search: boolean): boolean {
  const [spelled, base] = [path.toLowerCase().replace(/\/$/, "") || "/", root.toLowerCase()];
  return spelled === base || spelled.startsWith(`${base}/`) || (search && (spelled === "/" || base.startsWith(`${spelled}/`)));
}

// Filesystem check for a file tool target, or a search root when search is set. It compares inodes so a case alias of ~/.ssh resolves to the
// directory it names. Only a path whose spelling, or whose readlink-followed spelling, is already at or under ~/.ssh is compared: the inode
// probes follow links, so a target that is not established to be in ~/.ssh scope is never handed to them. When ~/.ssh scope leads into App Data
// the comparison is skipped and the target is denied, because stat would search that tree.
export function sshScopeDenied(target: string, home: string, search: boolean): boolean {
  const ssh = `${home}/.ssh`;
  const appdataOnly = (path: string) => isAppdata(path, home);
  const roots = [...new Set([ssh, followLinks(ssh, home, appdataOnly)])];
  const candidates = [...new Set([target, followLinks(target, home, appdataOnly)])];
  if (!candidates.some((candidate) => roots.some((root) => near(candidate, root, search)))) return false;
  if ([...roots, ...candidates].some((path) => isAppdata(path, home))) return true;
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
