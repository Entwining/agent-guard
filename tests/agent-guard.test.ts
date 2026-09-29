import { afterAll, describe, expect, test } from "bun:test";
import { chmodSync, existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, statSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir, userInfo } from "node:os";
import { basename, dirname, join } from "node:path";

import { buildRequest, evaluate, suggestions } from "../src/core";
import { probes } from "../src/probes";
import { reasons } from "../src/reasons";
import type { Tool } from "../src/record";
import appdataCases from "./fixtures/appdata.test";
import credentialCases from "./fixtures/credentials.test";
import cwdCases from "./fixtures/cwd.test";
import optionCases from "./fixtures/options.test";
import programCases from "./fixtures/programs.test";
import readerCases from "./fixtures/readers.test";
import searchCases from "./fixtures/search.test";
import shellCases from "./fixtures/shell.test";
import type { BehaviorRow } from "./fixtures/types.test";

const behaviorCases: BehaviorRow[] = [...appdataCases, ...credentialCases, ...optionCases, ...programCases, ...readerCases, ...searchCases, ...cwdCases, ...shellCases];

// Resolve the temp root so symlinked prefixes cannot hide paths under test.
const root = realpathSync(mkdtempSync(join(tmpdir(), "agent-guard-")));
const home = join(root, "home");
const locked = join(home, "project/locked");
afterAll(() => {
  chmodSync(locked, 0o755);
  rmSync(root, { recursive: true, force: true });
});
for (const dir of [".ssh/config.d", ".ssh/directory.pub", ".ssh/keys", ".ssh/known_hosts.backup", "project/nested", "project/locked", "Library/Containers/com.x"]) {
  mkdirSync(join(home, dir), { recursive: true });
}
for (const file of [
  ".ssh/allowed_signers",
  ".ssh/config",
  ".ssh/config.d/nested.pub",
  ".ssh/config.d/private",
  ".ssh/config.work",
  ".ssh/directory.pub/private",
  ".ssh/id.pub",
  ".ssh/id_rsa",
  ".ssh/keys/nested.pub",
  ".ssh/known_hosts.backup/private",
  ".ssh/known_hosts.old",
  ".ssh/private",
  "project/file.txt",
]) {
  writeFileSync(join(home, file), "");
}
for (const [link, target] of [
  [".ssh/deceptive.pub", ".ssh/private"],
  ["project-link", "project"],
  ["project/key-link", ".ssh/private"],
  ["project/public-link", ".ssh/id.pub"],
  ["project/ssh-link", ".ssh"],
  ["project/data-link", "Library/Containers"],
  ["project/env-link", ".npmrc"],
  ["project/protected-loop", "Library/Containers/loopa"],
]) {
  symlinkSync(join(home, target!), join(home, link!));
}
symlinkSync("data-link", join(home, "project/data-chain"));
symlinkSync("nested", join(home, "project/alias1"));
symlinkSync(join(home, "Library/Containers"), join(home, "project/nested/alias2"));
// A link whose text is spelled through the firmlink names the same directory as the plain spelling.
symlinkSync(`/System/Volumes/Data${home}/Library/Containers/com.x`, join(home, "project/firmlink-data"));
symlinkSync(`/System/Volumes/Data${home}/.ssh`, join(home, "project/firmlink-ssh"));
symlinkSync(`/System/Volumes/Data/../Data${home}/project/data-link`, join(home, "project/firmlink-up"));
// Read through the firmlink spelling, `..` from a directory that is also reached from the root goes to the root's parent, so climbing one level more than the depth lands on `/`.
symlinkSync(`${"../".repeat(`${home}/project`.split("/").length)}${home.slice(1)}/project/data-link`, join(home, "project/firmlink-climb"));
// `mnt` exists only on the Data volume, so `..` from it stays there.
symlinkSync(`/System/Volumes/Data/mnt/../../Data${home}/project/data-link`, join(home, "project/firmlink-data-only"));
symlinkSync(join(home, "Library/Containers/com.x"), join(home, ".ssh/inside-link"));
symlinkSync("../.npmrc", join(home, "project/env-relative"));
symlinkSync("loop-b", join(home, "project/loop-a"));
symlinkSync("loop-a", join(home, "project/loop-b"));
symlinkSync("loopb", join(home, "Library/Containers/loopa"));
symlinkSync("loopa", join(home, "Library/Containers/loopb"));
chmodSync(locked, 0);
const expand = (value: string) => value.replaceAll("$H", home).replaceAll("$R", root);

test("a file:// URL names the path it reads", () => {
  const url = (path: string) => buildRequest("claude", "bash", join(home, "project"), `curl -s file://${join(home, path)}`, "", home);
  expect(evaluate(url("Library/Containers/x"))).toBe(reasons.appdata);
  expect(evaluate(url(".ssh/id_rsa"))).toBe(reasons.file);
  expect(evaluate(url("project/file.txt"))).toBeUndefined();
});

test("workflow suggestions do not override security denials", () => {
  const safe = buildRequest("claude", "bash", join(home, "project"), "rg -rn foo src", "", home);
  expect(evaluate(safe)).toBeUndefined();
  expect(suggestions(safe)).toContain(reasons.replace);
  const unsafe = buildRequest("claude", "bash", home, "rg -rn foo ~/Library", "", home);
  expect(evaluate(unsafe)).toBe(reasons.broad);
  expect(suggestions(unsafe)).toContain(reasons.replace);
});

test("unsupported shell syntax fails closed with a usable alternative", () => {
  const request = buildRequest("claude", "bash", join(home, "project"), "for x (a b); do ls; done", "", home);
  expect(evaluate(request)).toBe(reasons.syntax);
});

test("home scans deny without a user ignore file", () => {
  expect(existsSync(join(home, ".ignore"))).toBe(false);
  for (const command of ["rg needle", "fd needle"]) {
    expect(evaluate(buildRequest("claude", "bash", home, command, "", home))).toBe(reasons.broad);
  }
});

test("22 relative cd commands stay within the hook deadline", () => {
  const input = "cd d1; cd d2; cd d3; cd d4; cd d5; cd d6; cd d7; cd d8; cd d9; cd d10; cd d11; cd d12; cd d13; cd d14; cd d15; cd d16; cd d17; cd d18; cd d19; cd d20; cd d21; cd d22; du -sh";
  const start = performance.now();
  expect(evaluate(buildRequest("claude", "bash", home, input, "", home))).toBeTruthy();
  expect(performance.now() - start).toBeLessThan(2000);
});

test("SSH private case aliases are denied on the case-insensitive test volume", () => {
  expect(existsSync(join(home, ".SSH/private"))).toBe(true);
  expect(evaluate(buildRequest("claude", "grep", home, ".SSH/private", "", home))).toBe(reasons.grepSsh);
  for (const command of ["cat", "ls"]) {
    expect(evaluate(buildRequest("claude", "bash", home, `${command} .SSH/private`, "", home))).toBe(reasons.ssh);
  }
});

test("SSH public case aliases remain exempt", () => {
  const alias = join(home, ".SSH/config");
  expect(existsSync(alias)).toBe(true);
  for (const tool of ["read", "grep"] as const) {
    expect(evaluate(buildRequest("claude", tool, home, alias, "", home))).toBeUndefined();
  }
  expect(evaluate(buildRequest("claude", "bash", home, "cat .SSH/config", "", home))).toBeUndefined();
});

test("a path in the directory a linked ~/.ssh leads to is judged as ~/.ssh", () => {
  const relocated = join(root, "relocated-home");
  const target = join(root, "relocated-ssh");
  mkdirSync(relocated, { recursive: true });
  mkdirSync(target, { recursive: true });
  writeFileSync(join(target, "private"), "");
  symlinkSync(target, join(relocated, ".ssh"));
  expect(evaluate(buildRequest("claude", "bash", relocated, `cat ${join(target, "private")}`, "", relocated))).toBe(reasons.ssh);
});

describe("behavior table", () => {
  test.each(behaviorCases.map((behaviorCase) => [`${behaviorCase.tool} ${JSON.stringify(behaviorCase.input)} in ${behaviorCase.cwd}`, behaviorCase] as const))("%s", (_, behaviorCase) => {
    const tool = behaviorCase.tool.toLowerCase() as Tool;
    // In a shell command `$H` is the fixture home unless it starts a longer variable name such as `$HOME`.
    const input = tool === "bash" ? behaviorCase.input.replaceAll("$U", userInfo().username).replace(/\$H(?![A-Za-z_])/g, home) : expand(behaviorCase.input);
    const runtimes: ("claude" | "codex")[] = tool === "bash" ? ["claude", "codex"] : ["claude"];
    for (const runtime of runtimes) {
      const request = buildRequest(runtime, tool, expand(behaviorCase.cwd), input, behaviorCase.glob ?? "", home);
      const reason = evaluate(request);
      expect({ runtime, exit: reason ? 2 : 0 }).toEqual({ runtime, exit: behaviorCase[runtime] ?? -1 });
      if (behaviorCase.reason) expect(reason).toBe(reasons[behaviorCase.reason]);
      if (runtime === "codex" && behaviorCase.codex_reason && reason) expect(reason).toBe(behaviorCase.codex_reason);
      if (runtime === "claude" && behaviorCase.claude_suggestions) expect(suggestions(request)).toEqual(behaviorCase.claude_suggestions.map((key) => reasons[key]));
    }
  });
});

// Every filesystem probe the guard makes about a command's path goes through `probes`. With App Data trees unreadable, a probe that would
// search inside one fails with EACCES, which is the effect the guard must never cause.
describe("probe safety", () => {
  const trees = (base: string) => ["Containers", "Group Containers", "Mobile Documents", "CloudStorage"].map((tree) => join(base, "Library", tree));
  const scratch = join(root, "probe-safety");

  // Run `run` with the directories unreadable and return the probes that failed with EACCES, apart from the ones aimed at the unreadable project directory.
  function searchesInside<T>(unreadable: string[], run: () => T): { result: T; violations: string[]; statted: string[] } {
    const violations: string[] = [];
    const statted: string[] = [];
    const [readlink, stat] = [probes.readlink, probes.stat];
    const watch = <F extends (path: string) => unknown>(call: F, name: string) =>
      ((path: string) => {
        if (name === "stat") statted.push(path);
        try {
          return call(path);
        } catch (error) {
          if ((error as NodeJS.ErrnoException).code === "EACCES" && !path.includes("/project/locked")) violations.push(`${name} ${path}`);
          throw error;
        }
      }) as F;
    probes.readlink = watch(readlink, "readlink") as typeof probes.readlink;
    probes.stat = watch(stat, "stat") as typeof probes.stat;
    for (const dir of unreadable) chmodSync(dir, 0);
    try {
      return { result: run(), violations, statted };
    } finally {
      for (const dir of unreadable) chmodSync(dir, 0o755);
      [probes.readlink, probes.stat] = [readlink, stat];
    }
  }

  test("a glob behind a link into App Data reaches the kernel as EACCES, not ENOENT", () => {
    // Premise: statting `cache/*.txt` makes the kernel search inside the tree `cache` leads to, which is what a guard probe must not do.
    const fixture = join(scratch, "premise");
    mkdirSync(join(fixture, "app/com.x"), { recursive: true });
    mkdirSync(join(fixture, "plain"), { recursive: true });
    symlinkSync(join(fixture, "app/com.x"), join(fixture, "cache"));
    symlinkSync(join(fixture, "plain"), join(fixture, "control"));
    chmodSync(join(fixture, "app/com.x"), 0);
    try {
      expect(() => statSync(join(fixture, "cache/*.txt"))).toThrow(expect.objectContaining({ code: "EACCES" }));
      expect(() => statSync(join(fixture, "control/*.txt"))).toThrow(expect.objectContaining({ code: "ENOENT" }));
    } finally {
      chmodSync(join(fixture, "app/com.x"), 0o755);
    }
  });

  test("no row of the behavior table, and no firmlink spelling or unresolved expansion behind a link, makes the guard search inside App Data", () => {
    for (const tree of trees(home)) mkdirSync(join(tree, "com.x"), { recursive: true });
    const { violations, statted } = searchesInside(trees(home), () => {
      for (const row of behaviorCases) {
        const tool = row.tool.toLowerCase() as Tool;
        const input = tool === "bash" ? row.input.replaceAll("$U", userInfo().username) : expand(row.input);
        evaluate(buildRequest("claude", tool, expand(row.cwd), input, row.glob ?? "", home));
      }
      // The firmlink names the same directory outside the spelling the App Data rule matches, so only the absence of a probe protects it.
      evaluate(buildRequest("claude", "bash", join(home, "project"), `cat /System/Volumes/Data${home}/Library/Containers/com.x/file.txt`, "", home));
      // An unresolved expansion behind a link names no path the link walk can follow, so it must not reach the inode comparison either.
      evaluate(buildRequest("claude", "bash", join(home, "project"), "cat data-link/$UNSET_VARIABLE", "", home));
    });
    expect(violations).toEqual([]);
    // stat follows links, so it may only be aimed at a path that leads into ~/.ssh or to a directory above it, never at one that leads elsewhere.
    const ssh = `${home}/.ssh`.toLowerCase();
    // A path that does not exist is resolved through its nearest existing ancestor, since that is as far as stat can follow it.
    const resolved = (path: string): string => (existsSync(path) ? realpathSync(path) : join(resolved(dirname(path)), basename(path)));
    const inScope = (path: string) => {
      const real = resolved(path).toLowerCase().replace(/\/$/, "");
      return `${real}/`.startsWith(`${ssh}/`) || ssh.startsWith(`${real}/`);
    };
    // The directories that hold such a path are compared too, and they lead nowhere by themselves.
    expect(statted.filter((path) => !inScope(path) && !statted.some((other) => other.startsWith(`${path}/`) && inScope(other)))).toEqual([]);
  });

  test("a link inside ~/.ssh that leads into App Data is denied without a probe", () => {
    const linked = join(scratch, "linked-home");
    mkdirSync(join(linked, ".ssh"), { recursive: true });
    mkdirSync(join(linked, "Library/Containers/com.x"), { recursive: true });
    symlinkSync(join(linked, "Library/Containers/com.x"), join(linked, ".ssh/inside-link"));
    const { result, violations } = searchesInside([join(linked, "Library/Containers")], () => evaluate(buildRequest("claude", "bash", linked, "ls .ssh/inside-link", "", linked)));
    expect(result).toBe(reasons.ssh);
    expect(violations).toEqual([]);
  });

  // The link walk reads the same paths first, so the real filesystem cannot make stat fail alone; the probe is replaced to fail on its own.
  const statFailing = (code: string) => {
    const stat = probes.stat;
    probes.stat = (() => {
      throw Object.assign(new Error(code), { code });
    }) as typeof probes.stat;
    try {
      return evaluate(buildRequest("claude", "bash", join(home, "project"), "cat ~/.ssh/id.pub", "", home));
    } finally {
      probes.stat = stat;
    }
  };

  test("an inode comparison that fails for a reason other than absence is a denial", () => {
    expect(statFailing("EIO")).toBeDefined();
  });

  test("an inode comparison against a path that does not exist finds no match", () => {
    expect(statFailing("ENOENT")).toBeUndefined();
  });
});
