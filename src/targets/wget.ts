import type { Target } from "../record";
import type { Context } from "./programs";

const wgetShort: Record<string, string> = { i: "input-file", O: "output-document", e: "execute", P: "directory-prefix", o: "output-file", a: "append-output" };
// The options whose value is written: the download, the log, or the directory the downloads go to.
const wgetWrites = ["output-document", "output-file", "append-output", "directory-prefix", "save-cookies", "warc-file", "hsts-file"];
// wgetrc command names ignore case, underscores and hyphens.
const wgetrc: Record<string, string> = {
  postfile: "post-file",
  bodyfile: "body-file",
  input: "input-file",
  outputdocument: "output-document",
  logfile: "output-file",
  dirprefix: "directory-prefix",
  loadcookies: "load-cookies",
  savecookies: "save-cookies",
  warcfile: "warc-file",
  hstsfile: "hsts-file",
};

// The file that holds the request body leaves the machine, as can a wgetrc file that names one, and the output document is written; `-e` runs a wgetrc command that can name either.
export function wgetTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  // A download lands in the working directory unless -O or a directory prefix says otherwise.
  let placed = false;
  for (const [i, word] of words.entries()) {
    const text = word.text;
    if (text === "--spider") placed = true;
    const long = /^--(post-file|body-file|input-file|output-document|output-file|append-output|directory-prefix|save-cookies|warc-file|hsts-file|execute|config|load-cookies)(=|$)/.exec(text);
    // The value is glued to the flag (-i.env) or follows it.
    const short = /^-[A-Za-z]*?([ieOPoa])(.*)$/.exec(text);
    let key = long?.[1] ?? wgetShort[short?.[1] ?? ""];
    if (!key) continue;
    const glued = long ? text.includes("=") : !!short?.[2];
    const value = glued ? word : words[i + 1];
    if (!value) continue;
    let path = !glued ? value.text : long ? text.slice(text.indexOf("=") + 1) : short![2]!;
    if (key === "execute") {
      const command = /^\s*([A-Za-z_-]+)\s*=\s*(.*)$/.exec(path);
      const name = wgetrc[command?.[1]!.toLowerCase().replace(/[_-]/g, "") ?? ""];
      if (!command || !name) continue;
      key = name;
      path = command[2]!;
    }
    claimed.add(value);
    placed ||= key === "output-document" || key === "directory-prefix";
    // `-O -` writes the document to standard output, so it names no file.
    if (key === "output-document" && path === "-") continue;
    targets.push(make(path, value, wgetWrites.includes(key) ? "write" : "read", { via: "option", walk: "none", sends: ["post-file", "body-file", "config"].includes(key) }));
  }
  if (!placed) targets.push(make(".", undefined, "write", { via: "option", walk: "none" }));
  return targets;
}
