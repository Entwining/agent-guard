import { normalize, resolve } from "node:path";

// Keep speculative directories bounded without losing broad home roots.
export function boundedDirectories(current: string, candidates: string[], home: string): string[] {
  const distinct = [...new Set(candidates)].filter((path) => path !== current);
  return distinct.length > 16 ? [home, `${home}/Library`] : distinct;
}

// A physical cd leaves a spelling the link walk resolves: `raw/.` or `raw/./tail`, where `raw` keeps its links and `..` segments unresolved
// and `tail` holds logical steps taken after it. Only the last `.` segment separates them; lexical resolution never produces one.
export function changeDirectory(cwd: string, target: string, physical: boolean): string {
  if (target.startsWith("/")) return physical ? `${target}/.` : resolve(target);
  const marker = cwd.endsWith("/.") ? cwd.length - 2 : cwd.lastIndexOf("/./");
  if (marker < 0) return physical ? `${cwd}/${target}/.` : resolve(cwd, target);
  const raw = cwd.slice(0, marker);
  const tail = cwd.slice(marker + 3);
  if (physical) return `${raw}/${tail ? `${tail}/` : ""}${target}/.`;
  // A logical `..` beyond the tail leaves a physical directory, whose lexical parent is its physical parent.
  const steps = normalize(tail ? `${tail}/${target}` : target).split("/");
  const up = steps.findIndex((step) => step !== "..");
  const parents = up < 0 ? steps.length : up;
  const rest = steps
    .slice(parents)
    .filter((step) => step !== ".")
    .join("/");
  return `${raw}${"/..".repeat(parents)}/.${rest ? `/${rest}` : ""}`;
}
