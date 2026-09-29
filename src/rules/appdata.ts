// App Data rules. macOS records a Files & Folders App Data entry whenever a
// process reads or enumerates another app's ~/Library data tree, so these deny
// those reads and the broad walks that reach them.
import { appdataTrees, isAppdata, isBroad, isLibrary } from "../paths";
import { reasons } from "../reasons";
import type { Request, Target } from "../record";

const { appdata: appdataReason, broad: broadReason } = reasons;

const trees = appdataTrees.join("|");

// A name is text, not a file, unless the shell expands it as a glob.
const touches = (target: Target) => target.effect !== "name" || target.glob;

function appdataHit(target: Target, home: string): boolean {
  if (!touches(target) || target.via === "scan") return false;
  // An expansion the front end cannot resolve may well be $HOME.
  return (target.expands && new RegExp(`/Library/(${trees})(/.*)?$`, "is").test(target.path)) || isAppdata(target.path, home, target.glob);
}

function broadHit(target: Target, home: string): boolean {
  if (target.via === "scan") return isBroad(target.path, home) || isAppdata(target.path, home);
  if (!touches(target)) return false;
  if (target.via === "tool") return target.search && isLibrary(target.path, home);
  // Shell glob expansion touches directories even when the command does not walk them.
  return isBroad(target.path, home, target.glob) && (target.walk !== "none" || target.glob);
}

export function appdataRules(req: Request, targets: Target[]): string[] {
  const denials: string[] = [];
  const groups = Map.groupBy(
    // The producer of a walk that feeds another command was judged on its own targets.
    targets.filter((target) => target.via !== "items" && !(target.via === "tool" && target.glob)),
    (target) => target.command,
  );
  for (const group of groups.values()) {
    if (group.some((target) => appdataHit(target, req.home))) denials.push(appdataReason);
    else if (group.some((target) => broadHit(target, req.home))) denials.push(broadReason);
  }
  const home = req.home.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const signature = new RegExp(`(~|\\$HOME|\\$\\{HOME\\}|${home})/Library/(${trees})`, "i");
  for (const fragment of req.uninspectable) if (signature.test(fragment.text)) denials.push(appdataReason);
  return denials;
}
