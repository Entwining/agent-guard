import { afterAll, describe, expect, test } from "bun:test";
import { appendFileSync, cpSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir, userInfo } from "node:os";
import { join } from "node:path";
import { buildRequest, evaluate, suggestions } from "../src/core.ts";
import { reasons } from "../src/reasons.ts";
import type { Tool } from "../src/record.ts";
import appdataCases from "./fixtures/appdata.test.ts";
import credentialCases from "./fixtures/credentials.test.ts";
import searchCases from "./fixtures/search.test.ts";
import cwdCases from "./fixtures/cwd.test.ts";
import shellCases from "./fixtures/shell.test.ts";
import type { BehaviorRow } from "./fixtures/types.test.ts";

const here = import.meta.dir;
const packageSource = join(here, "..");

const behaviorCases: BehaviorRow[] = [...appdataCases, ...credentialCases, ...searchCases, ...cwdCases, ...shellCases];

// Resolve the temp root so symlinked prefixes cannot hide paths under test.
const root = realpathSync(mkdtempSync(join(tmpdir(), "agent-guard-")));
const temporary = [root];
afterAll(() => temporary.forEach((dir) => rmSync(dir, { recursive: true, force: true })));
const home = join(root, "home");
for (const dir of [".ssh/config.d", ".ssh/directory.pub", ".ssh/keys", ".ssh/known_hosts.backup", "project", "Library/Containers"]) {
  mkdirSync(join(home, dir), { recursive: true });
}
for (const file of [
  ".ssh/allowed_signers", ".ssh/config", ".ssh/config.d/nested.pub", ".ssh/config.d/private", ".ssh/config.work",
  ".ssh/directory.pub/private", ".ssh/id.pub", ".ssh/keys/nested.pub", ".ssh/known_hosts.backup/private",
  ".ssh/known_hosts.old", ".ssh/private", "project/file.txt",
]) {
  writeFileSync(join(home, file), "");
}
for (const [link, target] of [
  [".ssh/deceptive.pub", ".ssh/private"], ["project-link", "project"], ["project/key-link", ".ssh/private"],
  ["project/public-link", ".ssh/id.pub"], ["project/ssh-link", ".ssh"],
  ["project/data-link", "Library/Containers"], ["project/env-link", ".npmrc"],
  ["project/protected-loop", "Library/Containers/loopa"],
]) {
  symlinkSync(join(home, target), join(home, link));
}
symlinkSync("data-link", join(home, "project/data-chain"));
symlinkSync("../.npmrc", join(home, "project/env-relative"));
symlinkSync("loop-b", join(home, "project/loop-a"));
symlinkSync("loop-a", join(home, "project/loop-b"));
symlinkSync("loopb", join(home, "Library/Containers/loopa"));
symlinkSync("loopa", join(home, "Library/Containers/loopb"));
const expand = (value: string) => value.replaceAll("$H", home).replaceAll("$R", root);

test("workflow suggestions do not override security denials", () => {
  const safe = buildRequest("claude", "bash", join(home, "project"), "find . -name x", "", home);
  expect(evaluate(safe)).toBeUndefined();
  expect(suggestions(safe)).toContain(reasons.find);
  const unsafe = buildRequest("claude", "bash", home, "find / -name x", "", home);
  expect(evaluate(unsafe)).toBe(reasons.broad);
  const launcher = buildRequest("codex", "bash", join(home, "project"), "command claude --version", "", home);
  expect(evaluate(launcher)).toBeUndefined();
  expect(suggestions(launcher)).toContain(reasons.launcher);
});

test("22 relative cd commands stay within the hook deadline", () => {
  const input = "cd d1; cd d2; cd d3; cd d4; cd d5; cd d6; cd d7; cd d8; cd d9; cd d10; cd d11; cd d12; cd d13; cd d14; cd d15; cd d16; cd d17; cd d18; cd d19; cd d20; cd d21; cd d22; du -sh";
  const start = performance.now();
  expect(evaluate(buildRequest("claude", "bash", home, input, "", home))).toBeTruthy();
  expect(performance.now() - start).toBeLessThan(2000);
});

describe("behavior table", () => {
  test.each(behaviorCases.map((behaviorCase) => [`${behaviorCase.tool} ${JSON.stringify(behaviorCase.input)} in ${behaviorCase.cwd}`, behaviorCase] as const))("%s", (_, behaviorCase) => {
    const tool = behaviorCase.tool.toLowerCase() as Tool;
    const input = tool === "bash" ? behaviorCase.input.replaceAll("$U", userInfo().username) : expand(behaviorCase.input);
    const runtimes: ("claude" | "codex")[] = tool === "bash" ? ["claude", "codex"] : ["claude"];
    for (const runtime of runtimes) {
      const reason = evaluate(buildRequest(runtime, tool, expand(behaviorCase.cwd), input, behaviorCase.glob ?? "", home));
      expect({ runtime, exit: reason ? 2 : 0 }).toEqual({ runtime, exit: behaviorCase[runtime] ?? -1 });
      if (runtime === "codex" && behaviorCase.codex_reason && reason) expect(reason).toBe(behaviorCase.codex_reason);
    }
  });
});

function install() {
  const h = realpathSync(mkdtempSync(join(tmpdir(), "agent-guard-home-")));
  temporary.push(h);
  const pkg = join(h, "package");
  for (const dir of ["bin", "project"]) mkdirSync(join(h, dir), { recursive: true });
  mkdirSync(join(pkg, "node_modules"), { recursive: true });
  cpSync(join(packageSource, "bin"), join(pkg, "bin"), { recursive: true });
  cpSync(join(packageSource, "src"), join(pkg, "src"), { recursive: true });
  cpSync(join(packageSource, "node_modules/mvdan-sh"), join(pkg, "node_modules/mvdan-sh"), { recursive: true });
  symlinkSync(process.execPath, join(h, "bin/bun"));
  return { h, pkg, guard: join(pkg, "bin/agent-guard") };
}

function run(h: string, cmd: string[], event: unknown, cwd = h) {
  const started = performance.now();
  const result = Bun.spawnSync({ cmd, cwd, stdin: Buffer.from(JSON.stringify(event)), env: { HOME: h, PATH: `${join(h, "bin")}:/usr/bin:/bin` } });
  return { exit: result.exitCode, stdout: result.stdout.toString(), stderr: result.stderr.toString(), seconds: (performance.now() - started) / 1000 };
}

const bash = (h: string, command: string) => ({ tool_name: "Bash", cwd: join(h, "project"), tool_input: { command } });
const blocked = /^The agent guard could not complete its check \(.+\), so this call is blocked\. (?:Inspect|Restore) .+\n$/;

describe("entry points", () => {
  test("Claude hook", () => {
    const { h, guard: entry } = install();
    const guard = [entry, "--runtime", "claude"];
    // Exit 0 carries no permission decision, so Claude's own rules still apply.
    expect(run(h, guard, bash(h, "ls"))).toMatchObject({ exit: 0, stdout: "", stderr: "" });
    expect(run(h, guard, bash(h, "find . -name x"))).toMatchObject({
      exit: 0,
      stdout: `${JSON.stringify({ hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: reasons.find } })}\n`,
      stderr: "",
    });
    expect(run(h, guard, bash(h, "env"))).toMatchObject({ exit: 2, stderr: expect.stringMatching(/^DENIED: This dumps .* Do NOT bypass/) });
    const read = { tool_name: "Read", cwd: join(h, "project"), tool_input: { file_path: ".env" } };
    expect(run(h, guard, read)).toMatchObject({ exit: 2 });
    expect(run(h, guard, { tool_name: "Bash" })).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
  });

  test("Codex hook", () => {
    const { h, guard } = install();
    const entry = [guard, "--runtime", "codex"];
    const event = (command: string) => ({ tool_input: { command, cwd: join(h, "project") } });
    expect(run(h, entry, event("ls"))).toMatchObject({ exit: 0, stdout: "", stderr: "" });
    expect(run(h, entry, event("command claude --version"))).toMatchObject({
      exit: 0,
      stdout: `${JSON.stringify({ hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: reasons.launcher } })}\n`,
      stderr: "",
    });
    expect(run(h, entry, event("env"))).toMatchObject({ exit: 2, stderr: `${reasons.dump}\n` });
    // Without a cwd in the event, paths resolve against the hook's directory.
    const bare = { tool_input: { command: "ls Library/Containers" } };
    expect(run(h, entry, bare, h)).toMatchObject({ exit: 2 });
    expect(run(h, entry, bare, join(h, "project"))).toMatchObject({ exit: 0 });
  });

  // Rejects a wrapper that runs bun in the agent's project, where a bunfig
  // preload could exit 0 before the guard runs.
  test("bun config in the agent's project does not reach the guard", () => {
    const { h, guard } = install();
    const project = join(h, "project");
    writeFileSync(join(project, "bunfig.toml"), 'preload = ["./preload.ts"]\n');
    writeFileSync(join(project, "preload.ts"), "process.exit(0);\n");
    expect(run(h, [guard, "--runtime", "claude"], bash(h, "env"), project)).toMatchObject({ exit: 2, stderr: expect.stringMatching(/^DENIED:/) });
  });
});

describe("faults deny instead of passing", () => {
  const cases: [string, (h: string, pkg: string) => void][] = [
    ["bun missing", (h) => rmSync(join(h, "bin/bun"))],
    ["guard.ts missing", (_, pkg) => rmSync(join(pkg, "src/guard.ts"))],
    ["parser missing", (_, pkg) => rmSync(join(pkg, "node_modules/mvdan-sh"), { recursive: true })],
    ["exit 2 without a reason", (_, pkg) => appendFileSync(join(pkg, "src/guard.ts"), "\nprocess.exitCode = 2;\n")],
  ];
  test.each(cases)("%s", (_, fault) => {
    const { h, pkg, guard } = install();
    fault(h, pkg);
    const out = run(h, [guard, "--runtime", "claude"], bash(h, "ls"));
    expect(out).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
    expect(out.seconds).toBeLessThan(5);
  }, 15_000);

  test("a guard that never finishes is killed at the deadline", () => {
    const { h, pkg, guard } = install();
    const pidFile = join(h, "guard.pid");
    appendFileSync(join(pkg, "src/guard.ts"), `\nawait Bun.write(${JSON.stringify(pidFile)}, String(process.pid));\nwhile (true) {}\n`);
    const out = run(h, [guard, "--runtime", "claude"], bash(h, "ls"));
    expect(out).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
    expect(out.seconds).toBeLessThan(5);
    const pid = Number(readFileSync(pidFile, "utf8"));
    expect(() => process.kill(pid, 0)).toThrow();
  }, 15_000);

  test("a child left running does not hold the hook past its deadline", () => {
    const { h, pkg, guard } = install();
    appendFileSync(join(pkg, "src/guard.ts"), '\nBun.spawn(["sleep", "20"], { stdout: "inherit", stderr: "inherit" }).unref();\n');
    const out = run(h, [guard, "--runtime", "claude"], bash(h, "ls"));
    expect(out.exit).toBe(0);
    expect(out.seconds).toBeLessThan(5);
  }, 15_000);
});
