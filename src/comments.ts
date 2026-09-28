// mvdan/sh 3.5 starts a comment at a # right after a quote or an expansion,
// where bash and zsh keep the # in the word and run the rest of the line.
import sh from "mvdan-sh";
import type { Comment, File } from "mvdan-sh";

export function misreadComment(file: File, bytes: Buffer): boolean {
  let misread = false;
  sh.syntax.Walk(file, (node) => {
    if (node && sh.syntax.NodeType(node) === "Comment") {
      const at = (node as Comment).Hash.Offset();
      misread ||= at > 0 && !" \t\n;&|()<>".includes(String.fromCharCode(bytes[at - 1]));
    }
    return !misread;
  });
  return misread;
}
