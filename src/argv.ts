import { basename, resolve } from "node:path";

import { absPath, expandHome } from "./paths";
import { findRoots } from "./programs";
import type { Command, Items, Word } from "./record";
import { searchRoles, showsHidden } from "./search-roles";

const shellPrograms = new Set(["sh", "bash", "zsh", "dash", "ksh", "csh", "tcsh"]);
const codePrograms = new Set(["python", "python3", "node", "bun", "deno", "ruby", "perl", "php", "osascript", "lua"]);
const zshBuiltins = new Set(["echo", "printf", "print", "export", "typeset", "declare", "set", "command", "eval", "source", "."]);
const shellCodeFlag = /^-[a-z]*c[a-z]*$/;
const sudoValueOption = /^(-[A-Za-z]*[ughpCDRTrtU]|--(user|group|host|prompt|chdir|chroot|role|type|other-user|close-from|command-timeout))$/;

// egrep and fgrep are grep with a fixed matcher.
export function programName(text: string): string {
  const name = basename(text);
  return name === "egrep" || name === "fgrep" ? "grep" : name;
}

export interface Child {
  source: string;
  items?: Items;
}

export function resolveCommand(cmd: Command, home: string): { children: Child[]; code: string[] } {
  const w = cmd.argv;
  const children: Child[] = [];
  const code: string[] = [];
  let i = 0;
  let shell = true;
  let lastPrecommand = "";
  // Bash reads an assignment after nocorrect as an argument; zsh does not.
  const assignment = (word: Word) => word.role === "assign" || /^[A-Za-z_][A-Za-z0-9_]*=/.test(word.raw);
  while (i < w.length && (assignment(w[i]!) || w[i]!.raw === "nocorrect")) w[i++]!.role = "precommand";
  while (i < w.length && ["builtin", "-", "noglob"].includes(w[i]!.text)) {
    lastPrecommand = w[i]!.text;
    w[i++]!.role = "precommand";
  }
  // builtin runs nothing when the next word is not a builtin.
  if (lastPrecommand === "builtin" && !zshBuiltins.has(w[i]?.text ?? "")) i = w.length;
  wrappers: while (i < w.length) {
    const name = basename(w[i]!.text);
    switch (name) {
      case "command":
        if (w[i]!.text !== "command") break wrappers;
        w[i++]!.role = "precommand";
        while (["-p", "--"].includes(w[i]?.text ?? "")) i++;
        // command -v only looks the name up.
        if (["-v", "-V"].includes(w[i]?.text ?? "")) i = w.length;
        break;
      case "exec":
        if (!shell || w[i]!.text !== "exec") break wrappers;
        w[i++]!.role = "precommand";
        while (["-c", "-l"].includes(w[i]?.text ?? "")) i++;
        if (w[i]?.text === "-a") i += 2;
        break;
      case "nohup":
        w[i++]!.role = "precommand";
        if (w[i]?.text === "--") i++;
        break;
      case "timeout":
        w[i++]!.role = "precommand";
        while (w[i]?.text?.startsWith("-")) {
          if (["-s", "--signal", "-k", "--kill-after"].includes(w[i]!.text)) i++;
          i++;
        }
        i++; // duration
        break;
      case "nice":
        w[i++]!.role = "precommand";
        if (w[i]?.text === "-n" || w[i]?.text === "--adjustment") i += 2;
        else if (/^(-n|--adjustment=|-\d)/.test(w[i]?.text ?? "")) i++;
        break;
      case "sudo":
      case "doas":
        w[i++]!.role = "precommand";
        while (i < w.length && w[i]!.text !== "--" && (w[i]!.text.startsWith("-") || /^[A-Za-z_][A-Za-z0-9_]*=/.test(w[i]!.text))) {
          if (sudoValueOption.test(w[i]!.text)) w[++i]!.role = "precommand";
          w[i++]!.role = "precommand";
        }
        if (w[i]?.text === "--") w[i++]!.role = "precommand";
        break;
      case "script":
        // script [-adkpqr] [-F pipe] [-t time] [file [command ...]]
        w[i++]!.role = "precommand";
        while (w[i]?.text?.startsWith("-")) {
          if (["-F", "-t"].includes(w[i]!.text)) i++;
          i++;
        }
        if (w[i]) w[i++]!.role = "precommand";
        break;
      case "arch":
        w[i++]!.role = "precommand";
        while (w[i]?.text?.startsWith("-")) {
          if (["-e", "-d", "-arch"].includes(w[i]!.text)) i++;
          i++;
        }
        break;
      case "stdbuf":
        w[i++]!.role = "precommand";
        while (/^-[ioe]/.test(w[i]?.text ?? "")) {
          if (/^-[ioe]$/.test(w[i]!.text)) i++;
          i++;
        }
        break;
      case "caffeinate":
        w[i++]!.role = "precommand";
        while (/^-[disumtw]$/.test(w[i]?.text ?? "")) {
          if (w[i]!.text === "-t" || w[i]!.text === "-w") i++;
          i++;
        }
        break;
      case "time":
        w[i++]!.role = "precommand";
        while (w[i]?.text?.startsWith("-")) {
          if (["-f", "-o", "--format", "--output"].includes(w[i]!.text)) i++;
          i++;
        }
        break;
      case "xargs":
        w[i++]!.role = "precommand";
        while (w[i]?.text?.startsWith("-")) {
          if (["-a", "-d", "-E", "-I", "-L", "-n", "-P", "-s", "--arg-file", "--delimiter", "--replace", "--max-args"].includes(w[i]!.text)) i++;
          i++;
        }
        break;
      case "env":
        w[i++]!.role = "precommand";
        while (i < w.length) {
          const arg = w[i]!.text;
          if (arg === "-C") {
            if (w[i + 1]) {
              cmd.cwd = resolve(cmd.cwd, w[i + 1]!.text);
              w[i + 1]!.role = "precommand";
            }
            i += 2;
          } else if (["-u", "-P"].includes(arg)) i += 2;
          else if (arg === "-S") {
            // The split string is a command line of its own.
            children.push({ source: w[i + 1]?.text ?? "" });
            cmd.wrappers.push("env-S");
            i = w.length;
          } else if (arg.startsWith("-") || arg.includes("=")) i++;
          else break;
        }
        break;
      case "repeat":
        // zsh's repeat COUNT runs the rest; bash parses it as a plain command.
        if (!shell || w[i]!.text !== "repeat") break wrappers;
        w[i++]!.role = "precommand";
        i++;
        continue;
      case "envchain":
        w[i++]!.role = "precommand";
        // --set, --list and --unset manage the store and run nothing.
        if (w[i]?.text?.startsWith("-")) i = w.length;
        if (w[i]) w[i]!.role = "namespace";
        i++;
        break;
      default:
        break wrappers;
    }
    cmd.wrappers.push(name);
    shell = false;
  }
  cmd.shell = shell;
  if (i >= w.length) return { children, code };
  cmd.program = i;
  w[i]!.role = "program";
  if (w[i]!.text.startsWith("=") && w[i]!.text.length > 1) w[i]!.text = w[i]!.text.slice(1);
  const name = programName(w[i]!.text);
  const rest = w.slice(i + 1);
  if (["rg", "grep", "ag", "ack"].includes(name)) {
    searchRoles(cmd, rest, name);
  } else if (name === "find") {
    for (const [n, word] of rest.entries()) {
      if (!["-exec", "-execdir", "-ok", "-okdir"].includes(word.text)) continue;
      const end = rest.findIndex((a, k) => k > n && (a.text === ";" || a.text === "+"));
      const clause = rest.slice(n + 1, end < 0 ? undefined : end);
      // find hands the command every name it walks, dotfiles included.
      children.push({ source: clause.map((a) => a.raw).join(" "), items: { root: absPath(findRoots(rest)[0]?.text ?? ".", cmd.cwd, home), hidden: true } });
    }
  } else if (name === "fd") {
    let pattern = true;
    const roots: string[] = [];
    for (let n = 0; n < rest.length; n++) {
      const arg = rest[n]!.text;
      if (["-x", "-X", "--exec", "--exec-batch"].includes(arg)) {
        rest[n]!.role = "option";
        children.push({
          source: rest
            .slice(n + 1)
            .map((word) => word.raw)
            .join(" "),
          items: { root: absPath(roots[0] ?? ".", cmd.cwd, home), hidden: showsHidden(rest) },
        });
        break;
      } else if ((/^(--search-path|--base-directory)(=|$)/.test(arg) || arg === "-C") && (arg.includes("=") || rest[n + 1])) {
        const separate = !arg.includes("=");
        const base = arg === "-C" || arg.startsWith("--base-directory");
        if (separate) rest[n++]!.role = "option";
        const path = rest[n]!;
        path.role = "path";
        path.value = separate ? path.text : arg.slice(arg.indexOf("=") + 1);
        if (base) {
          path.value = expandHome(path.value, home);
          cmd.cwd = resolve(cmd.cwd, path.value);
        }
      } else if (["-E", "-e", "-t", "-d", "--exclude", "--extension", "--type", "--max-depth"].includes(arg)) {
        rest[n]!.role = "option";
        if (rest[n + 1]) rest[++n]!.role = "optarg";
      } else if (arg.startsWith("-")) rest[n]!.role = "option";
      else if (pattern) {
        rest[n]!.role = "pattern";
        pattern = false;
      } else {
        rest[n]!.role = "path";
        roots.push(arg);
      }
    }
  } else if (name === "du") {
    for (let n = 0; n < rest.length; n++) {
      const arg = rest[n]!.text;
      if (["-d", "-I", "-B", "-t", "--max-depth", "--exclude", "--block-size", "--threshold"].includes(arg) || /^-[^-]+[dIBt]$/.test(arg)) {
        rest[n]!.role = "option";
        if (rest[n + 1]) rest[++n]!.role = "optarg";
      } else if (arg.startsWith("-")) rest[n]!.role = "option";
    }
  } else if (shellPrograms.has(name)) {
    const flag = rest.findIndex((a) => shellCodeFlag.test(a.text));
    if (flag >= 0) {
      if (rest[flag + 1]) rest[flag + 1]!.role = "code";
      children.push({ source: rest[flag + 1]?.text ?? "" });
    }
  } else if (name === "eval" && (shell || cmd.wrappers.includes("command"))) {
    children.push({ source: rest.map((a) => a.text).join(" ") });
  } else if (codePrograms.has(name)) {
    const flag = rest.findIndex((a) => /^(-[ceE]|--eval)$/.test(a.text));
    if (flag >= 0) {
      if (rest[flag + 1]) rest[flag + 1]!.role = "code";
      code.push(rest[flag + 1]?.text ?? "");
    }
  }
  return { children, code };
}

export function stdinKind(cmd: Command): "shell" | "code" | undefined {
  const program = cmd.argv[cmd.program];
  if (!program) return undefined;
  const name = basename(program.text);
  const args = cmd.argv.slice(cmd.program + 1);
  if (shellPrograms.has(name) && !args.some((a) => shellCodeFlag.test(a.text))) return "shell";
  if (codePrograms.has(name)) return "code";
  return undefined;
}
