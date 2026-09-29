import { readlinkSync } from "node:fs";
import { dirname, resolve } from "node:path";

import { isAppdata, isSensitive } from "./paths";
import type { Target } from "./record";

// Follow the symlinks in each target's path with readlink alone, so a link into
// a protected tree is judged by where it leads. Returns the same targets with
// followed paths replaced, or undefined when no path changed.
export function linkedTargets(targets: Target[], home: string): Target[] | undefined {
  let changed = false;
  const protectedPath = (path: string) => isAppdata(path, home) || isSensitive(path);
  const linked = (absolute: string): string => {
    let path = "/";
    let parts = absolute.split("/").filter(Boolean);
    let followed = false;
    let depth = 0;
    while (parts.length) {
      const part = parts.shift()!;
      path = part === ".." ? dirname(path) : resolve(path, part);
      // Do not ask the filesystem about a path inside a protected tree.
      if (protectedPath(path)) {
        changed = true;
        return path;
      }
      let target: string;
      try {
        target = readlinkSync(path);
      } catch {
        continue;
      }
      if (++depth > 8) throw new Error("Symlink chain exceeds the agent guard limit");
      const targetPath = target.startsWith("/") ? target : `${dirname(path)}/${target}`;
      followed = true;
      changed = true;
      parts = [...targetPath.split("/").filter(Boolean), ...parts];
      path = "/";
    }
    return followed ? path : absolute;
  };
  const result = targets.map((target) => {
    if (target.glob || target.expands || target.effect === "name" || (target.via === "tool" && target.glob)) return target;
    const path = linked(target.unresolved);
    return path === target.unresolved ? target : { ...target, path, unresolved: path };
  });
  return changed ? result : undefined;
}
