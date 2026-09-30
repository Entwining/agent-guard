import type { Effect, Target } from "../record";
import type { Context } from "./programs";

// `-o KEY=VALUE`, `-o "KEY VALUE"` and `-oKEY=VALUE` set a client option; these keys name a file the client reads or writes.
const sshFileOptions: Record<string, Effect> = { identityfile: "use", certificatefile: "use", globalknownhostsfile: "use", userknownhostsfile: "write", revokedhostkeys: "use", pkcs11provider: "use" };

// The option letters that take a value, from each client's synopsis in the OpenSSH 10.3p1 manual page; the first one in a cluster takes the rest of the word or the next word.
const valueLetters: Record<string, string> = { ssh: "BDEFIJLOPQRSWbceilmopw", scp: "DFJPSXcilo", sftp: "BDFJPRSXbcilos" };

// The client consumes identity files, configuration files, and -S transport operands itself; -E writes its log.
const letterEffects: Record<string, Effect> = { i: "use", F: "use", S: "use", E: "write" };

// ssh reads options before and right after the destination, scp and sftp before their first operand; later words are the remote command or operands.
export const sshTargets =
  (client: "ssh" | "scp" | "sftp") =>
  ({ words, make, claimed }: Context): Target[] => {
    const targets: Target[] = [];
    let operands = 0;
    for (let i = 0; i < words.length; i++) {
      const word = words[i]!;
      if (word.text === "--") break;
      if (!/^-./.test(word.text)) {
        if (++operands === (client === "ssh" ? 2 : 1)) break;
        continue;
      }
      const at = [...word.text.slice(1)].findIndex((letter) => valueLetters[client]!.includes(letter));
      if (at < 0) continue;
      const letter = word.text[at + 1]!;
      const glued = word.text.slice(at + 2);
      const holder = glued ? word : words[++i];
      if (!holder) break;
      const value = glued || holder.text;
      let effect: Effect | undefined = client === "sftp" && letter === "b" && value !== "-" ? "read" : letterEffects[letter];
      let paths = [value];
      if (letter === "o") {
        const setting = /^(\w+)(?:\s*=\s*|\s+)(.+)$/s.exec(value);
        effect = sshFileOptions[setting?.[1]?.toLowerCase() ?? ""];
        // ssh strips double quotes, takes several known-hosts files separated by spaces, and expands %d and ${HOME} to the home directory.
        paths = (setting?.[2] ?? "").match(/"[^"]*"|\S+/g)?.map((path) => path.replace(/"/g, "").replace(/^(%d|\$\{HOME\})(?=\/|$)/, "~")) ?? [];
      }
      if (!effect) continue;
      claimed.add(holder);
      for (const path of paths) targets.push(make(path, holder, effect, { via: "option", ...(letter === "o" && { quoted: false }) }));
    }
    return targets;
  };
