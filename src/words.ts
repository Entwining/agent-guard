import type { DblQuoted, Lit, Node, ParamExp, SglQuoted, Word as WordNode } from "mvdan-sh";
import sh from "mvdan-sh";

import { expandHome } from "./paths";
import type { Word } from "./record";

const type = sh.syntax.NodeType;

export function readWord(node: WordNode, slice: (start: number, end: number) => string, vars: Map<string, string>, home: string, visit: (part: Node, names: string[]) => void): Word {
  const text = (part: Node) => slice(part.Pos().Offset(), part.End().Offset());
  const out: Word = { text: "", raw: text(node), expands: false, globs: false, vars: [], role: "arg", value: "" };
  const expansion = (part: Node) => {
    const param = part as ParamExp;
    const plain = type(part) === "ParamExp" && !(param.Excl || param.Length || param.Width || param.Index || param.Slice || param.Repl || param.Exp);
    const known = plain ? (param.Param!.Value === "HOME" ? home : vars.get(param.Param!.Value)) : undefined;
    out.text += known ?? text(part);
    out.expands ||= known === undefined;
    visit(part, out.vars);
  };
  node.Parts.forEach((part, index) => {
    const kind = type(part);
    if (kind === "Lit") {
      let value = (part as Lit).Value;
      if (index === 0) value = expandHome(value, home);
      if (Bun.$.braces(value).length > 1) out.globs = true;
      for (let i = 0; i < value.length; i++) {
        if (value[i] === "\\") {
          out.text += value[++i] ?? "";
          continue;
        }
        if ("*?[".includes(value[i]!)) out.globs = true;
        out.text += value[i];
      }
    } else if (kind === "SglQuoted") {
      const quoted = part as SglQuoted;
      out.text += quoted.Dollar
        ? quoted.Value.replace(/\\(?:x([0-9a-fA-F]{1,2})|u([0-9a-fA-F]{4})|([0-7]{1,3})|(.))/gs, (_match, hex, unicode, octal, char) => {
            if (hex || unicode || octal) return String.fromCodePoint(parseInt(hex ?? unicode ?? octal, octal ? 8 : 16));
            return ({ a: "\x07", b: "\b", e: "\x1b", f: "\f", n: "\n", r: "\r", t: "\t", v: "\v" } as Record<string, string>)[char] ?? char;
          })
        : quoted.Value;
    } else if (kind === "DblQuoted") {
      for (const inner of (part as DblQuoted).Parts) {
        if (type(inner) === "Lit") out.text += (inner as Lit).Value.replace(/\\\n/g, "").replace(/\\([$`"\\])/g, "$1");
        else expansion(inner);
      }
    } else if (kind === "ExtGlob") {
      out.globs = true;
      out.text += text(part);
    } else {
      expansion(part);
    }
  });
  out.value = out.text;
  return out;
}
