import type { BinaryCmd, CallExpr, Lit, Stmt } from "mvdan-sh";
import sh from "mvdan-sh";

// Discard the prior cwd only when every move in an && chain has a literal destination.
export function movedOnSuccess(stmt: Stmt, slice: (start: number, end: number) => string): boolean {
  const type = sh.syntax.NodeType;
  const state = (item: Stmt): "moved" | "unchanged" | "uncertain" => {
    const node = item.Cmd;
    if (!node) return "unchanged";
    if (type(node) === "CallExpr") {
      const call = node as CallExpr;
      const first = call.Args[0]?.Parts[0];
      const name = first && type(first) === "Lit" ? (first as Lit).Value : "";
      if (name === "popd") return "uncertain";
      if (name !== "cd" && name !== "pushd") return "unchanged";
      const target = call.Args[1];
      const part = target?.Parts[0];
      return target?.Parts.length === 1 && part && type(part) === "Lit" && (part as Lit).Value !== "-" ? "moved" : "uncertain";
    }
    if (type(node) !== "BinaryCmd") return "uncertain";
    const chain = node as BinaryCmd;
    if (slice(chain.OpPos.Offset(), chain.OpPos.Offset() + 2) !== "&&") return "uncertain";
    const left = state(chain.X!);
    const right = state(chain.Y!);
    return left === "uncertain" || right === "uncertain" ? "uncertain" : left === "moved" || right === "moved" ? "moved" : "unchanged";
  };
  return state(stmt) === "moved";
}
