import type { Target, Word } from "../record";
import type { Context } from "./programs";

const globalValueOptions = ["-H", "--host", "-c", "--context", "-l", "--log-level", "--config", "--tlscacert", "--tlscert", "--tlskey"];

// The options of `docker run`, `create` and `exec` that take no value; any other option takes the next word unless its value is glued.
const flagOptions = [
  "--detach",
  "--help",
  "--init",
  "--interactive",
  "--no-healthcheck",
  "--oom-kill-disable",
  "--privileged",
  "--publish-all",
  "--quiet",
  "--read-only",
  "--rm",
  "--sig-proxy",
  "--tty",
  "--use-api-socket",
];
const valueOptions = ["--entrypoint"];
const flagLetters = "diqPt";
const composeValueOptions = ["-f", "--file", "-p", "--project-name", "--project-directory", "--profile", "--env-file", "--ansi", "--parallel", "--progress"];

// Where the image (`run`, `create`) or the container (`exec`) is: every word after it belongs to the command run inside the container, not to docker.
function commandStart(words: Word[], start: number): number {
  for (let i = start + 1; i < words.length; i++) {
    const text = words[i]!.text;
    if (!text.startsWith("-")) return i;
    if (text === "--") return i + 1;
    if (text.startsWith("--")) {
      if (!text.includes("=") && (valueOptions.includes(text) || (!flagOptions.includes(text) && !/^-\D/.test(words[i + 1]?.text ?? "")))) i++;
      continue;
    }
    const value = [...text.slice(1)].findIndex((letter) => !flagLetters.includes(letter));
    if (value >= 0 && value === text.length - 2) i++;
  }
  return words.length;
}

// A bind mount, a copy out of the host, a build secret or an image archive hands a host file to docker; the image, container names and the command run inside it are names.
export function dockerTargets({ words, make, claimed }: Context): Target[] {
  const targets: Target[] = [];
  const mount = (path: string, word: Word, effect: "read" | "write" | "use" = "read", via: "option" | "operand" = "option") => {
    claimed.add(word);
    targets.push(make(path, word, effect, { via }));
  };
  // The subcommand follows the global options and their values, and a management word such as `container` in `docker container cp`.
  let start = 0;
  while (words[start]?.text.startsWith("-")) start += globalValueOptions.includes(words[start]!.text) ? 2 : 1;
  if (["container", "image", "buildx"].includes(words[start]?.text ?? "")) start++;
  const sub = words[start]?.text;
  const end = ["run", "create", "exec"].includes(sub ?? "") ? commandStart(words, start) : words.length;
  // After a compose subcommand, `-f` is that subcommand's option (`logs -f` follows), not the compose file.
  let composeSub = start + 1;
  while (words[composeSub]?.text.startsWith("-")) composeSub += composeValueOptions.includes(words[composeSub]!.text) ? 2 : 1;
  for (const [i, word] of words.slice(0, end).entries()) {
    if (words[i - 1]?.text === "--entrypoint") continue;
    // cp names a container as `name:path`; any other operand is a host path, and the last one is where the copy lands.
    if (sub === "cp" && i > start && !word.text.startsWith("-") && !/^[\w.-]+:/.test(word.text)) mount(word.text, word, i === words.length - 1 ? "write" : "read", "operand");
    // A short option's value is glued (`-i.env`, `-vSPEC`, `-v=SPEC`) or the next word; pflag reads the rest of a cluster after the option that takes it.
    const previous = words[i - 1]?.text;
    if (sub === "load") {
      const input = /^(?:--input=|-i)(.+)$/s.exec(word.text)?.[1] ?? (previous === "-i" || previous === "--input" ? word.text : undefined);
      if (input) mount(input, word);
    }
    const secret = word.text.startsWith("--secret=") ? word.text.slice("--secret=".length) : previous === "--secret" ? word.text : undefined;
    for (const field of secret?.split(",") ?? []) if (/^(src|source)=/.test(field)) mount(field.slice(field.indexOf("=") + 1), word);
    // The client reads or writes these files itself; `-f` names one only for build and compose.
    const hostFile =
      /^--(file|label-file|cidfile|iidfile|tlscacert|tlscert|tlskey|config)(?:=(.*))?$/s.exec(word.text) ??
      (sub === "build" || (sub === "compose" && i < composeSub) ? /^-[A-Za-z]*?(f)=?(.*)$/s.exec(word.text) : null);
    if (hostFile) {
      const holder = hostFile[2] ? word : words[i + 1];
      if (holder) mount(hostFile[2] || holder.text, holder, ["cidfile", "iidfile"].includes(hostFile[1]!) ? "write" : "use");
    }
    const long = /^--(volume|mount)(?:=(.*))?$/s.exec(word.text);
    const short = /^-[A-Za-z]*?v(=?)(.*)$/s.exec(word.text);
    const key = long?.[1] ?? (short ? "volume" : undefined);
    const inline = long ? long[2] : short?.[2];
    const value = inline ? word : words[i + 1];
    if (!key || !value) continue;
    const spec = inline || value.text;
    if (key === "volume") mount(spec.split(":")[0]!, value);
    else for (const field of spec.split(",")) if (/^(src|source)=/.test(field)) mount(field.slice(field.indexOf("=") + 1), value);
  }
  return targets;
}
