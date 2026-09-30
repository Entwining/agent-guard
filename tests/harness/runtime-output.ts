import { randomUUID } from "node:crypto";
import { homedir } from "node:os";
import { join, resolve, sep } from "node:path";

import { followLinks } from "../../src/filesystem/links";
import { isAppdata, isSensitiveRoot } from "../../src/filesystem/paths";

const checkout = resolve(import.meta.dir, "../..");
const home = homedir();
const protectedPath = (path: string) => isAppdata(path, home) || isSensitiveRoot(path, home);
const physicalCheckout = followLinks(checkout, home, protectedPath);
const within = (directory: string, path: string) => path === directory || path.startsWith(`${directory}${sep}`);

export function resultPath(value: string | undefined, label: string): string {
  const path = resolve(value ?? join("/tmp", `agent-guard-${label}-${randomUUID()}.json`));
  if (within(checkout, path) || protectedPath(path)) throw new Error("Result path must stay outside the checkout and protected paths");

  const physical = followLinks(path, home, protectedPath);
  if (within(physicalCheckout, physical) || protectedPath(physical)) throw new Error("Result path must stay outside the checkout and protected paths");
  return path;
}
