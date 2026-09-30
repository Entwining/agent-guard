import type { Target, Word } from "../record";
import type { Context } from "./programs";

// curl's short options that take a value; the rest of a cluster is that value.
export const curlValueLetters = "AbcCdDeEFHKmoPQrTtuUwxXyYz";
const curlDataOptions = /^(d|data|data-ascii|data-binary|data-urlencode|json|H|header|proxy-header|url-query|variable)$/;

// The options whose value is a file curl writes; `-` sends the output to standard output.
const curlWrites = ["o", "output", "D", "dump-header", "c", "cookie-jar", "etag-save", "libcurl", "stderr", "hsts", "alt-svc", "trace", "trace-ascii", "ssl-sessions"];

export function curlTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  // The text after an @ or < inside a value is literal: neither the shell nor curl expands a ~ there.
  const read = (path: string, word: Word, quoted?: boolean, sends = true) => {
    claimed.add(word);
    targets.push(make(path, word, "read", { via: "option", sends, ...(quoted && { quoted }) }));
  };
  // -O and --remote-name save each download under its remote name, in the working directory or the --output-dir.
  let remoteName = false;
  let outputDir: Word | undefined;
  for (let i = 0; i < words.length; i++) {
    const word = words[i]!;
    const text = word.text;
    if (text === "--") break;
    remoteName ||= /^(--remote-name(-all)?|-[^-]*O)$/.test(text);
    if (/^--output-dir(=|$)/.test(text)) {
      outputDir = text.includes("=") ? word : words[++i];
      if (outputDir) claimed.add(outputDir);
      continue;
    }
    // curl reads `file:path` and `file://path` alike, decodes %XX, and expands `[a-z]` and `{a,b}` ranges.
    const url = /^(--url=)?(file:.*)$/is.exec(text);
    if (url) {
      claimed.add(word);
      const path = url[2]!.replace(/^file:(\/\/)?/i, "").replace(/%([0-9a-f]{2})/gi, (_, hex: string) => String.fromCharCode(parseInt(hex, 16)));
      targets.push(make(path, word, "read", { via: "operand", glob: /[[{]/.test(path) }));
      continue;
    }
    let key = "";
    let value = "";
    const long =
      /^--(data|data-ascii|data-binary|data-urlencode|json|form|header|proxy-header|url-query|variable|upload-file|config|output|dump-header|write-out|cookie|etag-compare|cookie-jar|etag-save|libcurl|stderr|hsts|alt-svc|trace|trace-ascii|ssl-sessions)(=|$)/s.exec(
        text,
      );
    if (long) {
      key = long[1]!;
      value = text.includes("=") ? text.slice(text.indexOf("=") + 1) : (words[++i]?.text ?? "");
    } else if (/^-[^-]/.test(text)) {
      for (let k = 1; k < text.length; k++) {
        if (!curlValueLetters.includes(text[k]!)) continue;
        key = text[k]!;
        value = text.slice(k + 1) || (words[++i]?.text ?? "");
        break;
      }
    }
    const from = words[i]!;
    if (curlDataOptions.test(key)) {
      if (/@./s.test(value)) read(value.slice(value.indexOf("@") + 1), from, true);
    } else if (key === "F" || key === "form") {
      const file = value.slice(value.indexOf("=") + 1).replace(/^[@<]/, "");
      // A quoted file name may hold a `;`.
      read(/^"([^"]*)"/.exec(file)?.[1] ?? file.split(";")[0]!, from, true);
    } else if (["T", "upload-file", "K", "config", "etag-compare"].includes(key)) read(value, from);
    // The response is written out with the text of a `@file` template; `@-` is standard input.
    else if ((key === "w" || key === "write-out") && /^@./.test(value) && value !== "@-") read(value.slice(1), from, true, false);
    // A cookie value with no `=` names a cookie file that curl parses itself.
    else if ((key === "b" || key === "cookie") && value && !value.includes("=")) {
      claimed.add(from);
      targets.push(make(value, from, "use", { via: "option" }));
    } else if (curlWrites.includes(key) && value) {
      claimed.add(from);
      // `-o -` sends the response to standard output, so it names no file.
      if (value !== "-") targets.push(make(value, from, "write", { via: "option" }));
    }
  }
  if (remoteName) targets.push(make(outputDir ? outputDir.text.replace(/^--output-dir=/, "") : ".", outputDir, "write", { via: "option", walk: "none" }));
  return targets;
}
