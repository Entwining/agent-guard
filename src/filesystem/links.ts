import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";

import type { Target } from "../record";
import { isAppdata, isSensitive, unfirmlink } from "./paths";
import { probes } from "./probes";

// Errors that mean the path is not a link: it names a regular file or directory, or nothing (a component past NAME_MAX cannot exist). Any other
// failure means the check did not run.
export const notALink = ["EINVAL", "ENOENT", "ENOTDIR", "ENAMETOOLONG"];

// The directories of the Data volume that are also reached from the root, in the kernel's own table. The kernel resolves `..` from one of them
// in the root tree, and from any other directory of the Data volume in place. The table is a fixed system file, not a path taken from a command;
// if it is missing the import fails and the guard denies.
const firmlinks = new Set(
  readFileSync("/usr/share/firmlinks", "utf8")
    .split("\n")
    .filter(Boolean)
    .map((line) => `/system/volumes/data/${line.split("\t")[1]!.toLowerCase()}`),
);

// Follow the symlinks in an absolute path with readlink alone, so a link into a protected tree is judged by where it leads. A path for which
// `stopAt` holds is returned at the point it is reached, because the filesystem is not asked about anything beyond it.
export function followLinks(absolute: string, home: string, stopAt = (path: string) => isAppdata(path, home) || isSensitive(path)): string {
  let path = "/";
  let parts = absolute.split("/").filter(Boolean);
  let followed = false;
  let depth = 0;
  while (parts.length) {
    const part = parts.shift()!;
    // The walk keeps the physical spelling, because `..` after the firmlink prefix leaves it, and only the check and the result use the plain one.
    path = part === ".." ? dirname(firmlinks.has(path.toLowerCase()) ? unfirmlink(path) : path) : resolve(path, part);
    if (stopAt(unfirmlink(path))) return unfirmlink(path);
    let target: string;
    try {
      target = probes.readlink(path);
    } catch (error) {
      if (notALink.includes((error as NodeJS.ErrnoException).code ?? "")) continue;
      throw error;
    }
    if (++depth > 8) throw new Error("Symlink chain exceeds the agent guard limit");
    const targetPath = target.startsWith("/") ? target : `${dirname(path)}/${target}`;
    followed = true;
    parts = [...targetPath.split("/").filter(Boolean), ...parts];
    path = "/";
  }
  return followed ? unfirmlink(path) : absolute;
}

// Only the prefix before a wildcard or unresolved expansion is fixed before the shell expands the rest, so probing the suffix is unsafe.
function followPrefix(absolute: string, home: string): string {
  const segments = absolute.split("/");
  const wildcard = segments.findIndex((segment) => /[*?[{$`]/.test(segment));
  if (wildcard < 0) return followLinks(absolute, home);
  const prefix = segments.slice(0, wildcard).join("/") || "/";
  const followed = followLinks(prefix, home);
  return followed === prefix ? absolute : [followed === "/" ? "" : followed, ...segments.slice(wildcard)].join("/");
}

// Returns the same targets with followed paths replaced, or undefined when no path changed.
export function linkedTargets(targets: Target[], home: string): Target[] | undefined {
  let changed = false;
  const result = targets.map((target) => {
    if ((target.effect === "name" && !target.glob) || (target.via === "tool" && target.glob)) return target;
    const path = target.glob || target.expands ? followPrefix(target.unresolved, home) : followLinks(target.unresolved, home);
    if (path === target.unresolved) return target;
    changed = true;
    return { ...target, path, unresolved: path };
  });
  return changed ? result : undefined;
}
