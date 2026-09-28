import { readlinkSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";
import { expandHome, isAppdata, isSensitive } from "./paths.ts";
import type { Request, Word } from "./record.ts";
import { dataPrograms } from "./rules/appdata.ts";

export function linkedRequest(req: Request): Request | undefined {
  let changed = false;
  const protectedPath = (path: string) => isAppdata(path, req.home) || isSensitive(path);
  const linked = (value: string, cwd: string, quoted = false): string => {
    const input = quoted ? value : expandHome(value, req.home);
    const absolute = input.startsWith("/") ? input : `${cwd}/${input}`;
    let path = "/";
    let parts = absolute.split("/").filter(Boolean);
    let followed = false;
    let depth = 0;
    while (parts.length) {
      path = resolve(path, parts.shift()!);
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
      const targetPath = resolve(dirname(path), target);
      followed = true;
      changed = true;
      if (protectedPath(targetPath)) return targetPath;
      const next = resolve(targetPath, ...parts);
      if (protectedPath(next)) return next;
      parts = next.split("/").filter(Boolean);
      path = "/";
    }
    return followed ? path : value;
  };
  const word = (w: Word, cwd: string): Word => {
    if (w.expands || w.globs || !w.value || w.value.startsWith("-")) return w;
    if (w.role === "program" && !w.value.includes("/")) return w;
    if (!["arg", "path", "patfile", "option:patfile", "program"].includes(w.role)) return w;
    const value = linked(w.value, cwd, /^[\'"]/.test(w.raw));
    return value === w.value ? w : { ...w, value, text: w.role.startsWith("option:") ? w.text : value };
  };
  const commands = req.commands.map((cmd) => ({
    ...cmd,
    argv: cmd.argv.map((w, i) =>
      dataPrograms.has(basename(cmd.argv[cmd.program]?.text ?? "")) && i > cmd.program ? w : word(w, cmd.cwd)),
    redirects: cmd.redirects.map((r) =>
      (r.direction === "in" || r.direction === "out") && r.target ? { ...r, target: linked(r.target, cmd.cwd) } : r),
  }));
  const target = req.target ? linked(req.target, req.cwd) : req.target;
  const searchRoot = req.searchRoot ? linked(req.searchRoot, req.cwd) : req.searchRoot;
  return changed ? { ...req, target, searchRoot, commands } : undefined;
}
