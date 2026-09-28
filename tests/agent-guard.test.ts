import { afterAll, describe, expect, test } from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir, userInfo } from "node:os";
import { join } from "node:path";

import { buildRequest, evaluate, suggestions } from "../src/core";
import { reasons } from "../src/reasons";
import type { Tool } from "../src/record";
import appdataCases from "./fixtures/appdata.test";
import credentialCases from "./fixtures/credentials.test";
import cwdCases from "./fixtures/cwd.test";
import searchCases from "./fixtures/search.test";
import shellCases from "./fixtures/shell.test";
import type { BehaviorRow } from "./fixtures/types.test";

const behaviorCases: BehaviorRow[] = [...appdataCases, ...credentialCases, ...searchCases, ...cwdCases, ...shellCases];

// Resolve the temp root so symlinked prefixes cannot hide paths under test.
const root = realpathSync(mkdtempSync(join(tmpdir(), "agent-guard-")));
afterAll(() => rmSync(root, { recursive: true, force: true }));
const home = join(root, "home");
for (const dir of [".ssh/config.d", ".ssh/directory.pub", ".ssh/keys", ".ssh/known_hosts.backup", "project/nested", "Library/Containers"]) {
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
symlinkSync("../.npmrc", join(home, "project/env-relative"));
symlinkSync("loop-b", join(home, "project/loop-a"));
symlinkSync("loop-a", join(home, "project/loop-b"));
symlinkSync("loopb", join(home, "Library/Containers/loopa"));
symlinkSync("loopa", join(home, "Library/Containers/loopb"));
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

describe("behavior table", () => {
  test.each(behaviorCases.map((behaviorCase) => [`${behaviorCase.tool} ${JSON.stringify(behaviorCase.input)} in ${behaviorCase.cwd}`, behaviorCase] as const))("%s", (_, behaviorCase) => {
    const tool = behaviorCase.tool.toLowerCase() as Tool;
    const input = tool === "bash" ? behaviorCase.input.replaceAll("$U", userInfo().username) : expand(behaviorCase.input);
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
