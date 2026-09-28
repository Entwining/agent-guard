import { readlinkSync } from "node:fs";
import { basename, dirname, resolve } from "node:path";

import { absPath, expandHome, isAppdata, isSensitive } from "./paths";
import type { Request, Word } from "./record";
import { dataPrograms } from "./rules/appdata";

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
    return followed ? path : value;
  };
  const word = (w: Word, cwd: string): Word => {
    if (w.expands || w.globs || !w.value || w.value.startsWith("-")) return w;
    if (w.role === "program" && !w.value.includes("/")) return w;
    if (!["arg", "path", "patfile", "option:patfile", "program"].includes(w.role)) return w;
    const value = linked(w.value, cwd, /^['"]/.test(w.raw));
    return value === w.value ? w : { ...w, value, text: w.role.startsWith("option:") ? w.text : value };
  };
  const commands = req.commands.map((cmd) => {
    const checkedCwd = linked(cmd.cwd, "/");
    const cwd = checkedCwd === cmd.cwd ? absPath(cmd.cwd, "/", req.home) : checkedCwd;
    if (cwd !== cmd.cwd) changed = true;
    return {
      ...cmd,
      cwd,
      argv: cmd.argv.map((w, i) => (dataPrograms.has(basename(cmd.argv[cmd.program]?.text ?? "")) && i > cmd.program ? w : word(w, cmd.cwd))),
      redirects: cmd.redirects.map((r) => ((r.direction === "in" || r.direction === "out") && r.target ? { ...r, target: linked(r.target, cmd.cwd) } : r)),
    };
  });
  const checkedCwd = linked(req.inputCwd, "/");
  const cwd = checkedCwd === req.inputCwd ? req.cwd : checkedCwd;
  const target = req.target ? linked(req.pathInput, req.inputCwd) : req.target;
  const searchRoot = req.searchRoot ? linked(req.pathInput || req.inputCwd, req.inputCwd) : req.searchRoot;
  return changed ? { ...req, cwd, target, searchRoot, commands } : undefined;
}
