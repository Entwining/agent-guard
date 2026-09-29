import { afterAll, describe, expect, test } from "bun:test";
import { appendFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { reasons } from "../src/reasons";
import { exists as processExists, instrumentEntry, instrumentRunner, run, stop } from "./hook-process";

const packageSource = join(import.meta.dir, "..");
const temporary: string[] = [];
afterAll(() => temporary.forEach((dir) => rmSync(dir, { recursive: true, force: true })));

function install() {
  const h = realpathSync(mkdtempSync(join(tmpdir(), "agent-guard-home-")));
  temporary.push(h);
  const pkg = join(h, "package");
  for (const dir of ["bin", "project"]) mkdirSync(join(h, dir), { recursive: true });
  mkdirSync(join(pkg, "node_modules"), { recursive: true });
  cpSync(join(packageSource, "bin"), join(pkg, "bin"), { recursive: true });
  instrumentEntry(join(pkg, "bin/agent-guard"));
  cpSync(join(packageSource, "src"), join(pkg, "src"), { recursive: true });
  instrumentRunner(join(pkg, "src/runner.ts"));
  cpSync(join(packageSource, "tsconfig.json"), join(pkg, "tsconfig.json"));
  cpSync(join(packageSource, "node_modules/mvdan-sh"), join(pkg, "node_modules/mvdan-sh"), { recursive: true });
  symlinkSync(process.execPath, join(h, "bin/bun"));
  return { h, pkg, guard: join(pkg, "bin/agent-guard") };
}

const bash = (h: string, command: string) => ({ tool_name: "Bash", cwd: join(h, "project"), tool_input: { command } });
const blocked = /^The agent guard could not complete its check \(.+\), so this call is blocked\. (?:Inspect|Restore) .+\n$/;

describe("entry points", () => {
  test("Claude hook", async () => {
    const { h, guard: entry } = install();
    const guard = [entry, "--runtime", "claude"];
    // Exit 0 carries no permission decision, so Claude's own rules still apply.
    expect(await run(h, guard, bash(h, "ls"))).toMatchObject({ exit: 0, stdout: "", stderr: "" });
    expect(await run(h, guard, bash(h, "rg -rn foo src"))).toMatchObject({
      exit: 0,
      stdout: `${JSON.stringify({ hookSpecificOutput: { hookEventName: "PreToolUse", additionalContext: reasons.replace } })}\n`,
      stderr: "",
    });
    expect(await run(h, guard, bash(h, "env"))).toMatchObject({ exit: 2, stderr: expect.stringMatching(/^DENIED: This dumps .* Do NOT bypass/) });
    expect(await run(h, guard, bash(h, "for x (a b); do ls; done"))).toMatchObject({ exit: 2, stderr: expect.stringContaining(reasons.syntax) });
    expect(await run(h, guard, { tool_name: "Bash", cwd: h, tool_input: { command: "rg foo" } })).toMatchObject({
      exit: 2,
      stderr: expect.stringMatching(/^DENIED: A scan rooted at the home directory/),
    });
    expect(await run(h, guard, bash(h, "rg -rn foo ~/Library"))).toMatchObject({
      exit: 2,
      stdout: "",
      stderr: expect.stringMatching(/^DENIED: A scan rooted at the home directory or ~\/Library/),
    });
    const read = { tool_name: "Read", cwd: join(h, "project"), tool_input: { file_path: ".env" } };
    expect(await run(h, guard, read)).toMatchObject({ exit: 2 });
    expect(await run(h, guard, { tool_name: "Bash" })).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
  });

  test("Codex hook", async () => {
    const { h, guard } = install();
    const entry = [guard, "--runtime", "codex"];
    const event = (command: string) => ({ tool_input: { command, cwd: join(h, "project") } });
    expect(await run(h, entry, event("ls"))).toMatchObject({ exit: 0, stdout: "", stderr: "" });
    expect(await run(h, entry, event("env"))).toMatchObject({ exit: 2, stderr: `${reasons.dump}\n` });
    // Without a cwd in the event, paths resolve against the hook's directory.
    const bare = { tool_input: { command: "ls Library/Containers" } };
    expect(await run(h, entry, bare, h)).toMatchObject({ exit: 2 });
    expect(await run(h, entry, bare, join(h, "project"))).toMatchObject({ exit: 0 });
  });

  // Rejects a wrapper that runs bun in the agent's project, where a bunfig
  // preload could exit 0 before the guard runs.
  test("bun config in the agent's project does not reach the guard", async () => {
    const { h, guard } = install();
    const project = join(h, "project");
    writeFileSync(join(project, "bunfig.toml"), 'preload = ["./preload.ts"]\n');
    writeFileSync(join(project, "preload.ts"), "process.exit(0);\n");
    expect(await run(h, [guard, "--runtime", "claude"], bash(h, "env"), project)).toMatchObject({ exit: 2, stderr: expect.stringMatching(/^DENIED:/) });
  });

  test("an ancestor tsconfig cannot replace the shell parser", async () => {
    const { h, pkg, guard } = install();
    const marker = join(h, "fake-parser-loaded");
    writeFileSync(join(h, "tsconfig.json"), JSON.stringify({ compilerOptions: { baseUrl: ".", paths: { "mvdan-sh": ["./fake.ts"] } } }));
    writeFileSync(join(h, "fake.ts"), `import { writeFileSync } from "node:fs";\nwriteFileSync(${JSON.stringify(marker)}, "");\nexport default {};\n`);
    expect(await run(h, [guard, "--runtime", "claude"], bash(h, "env"))).toMatchObject({ exit: 2, stderr: expect.stringMatching(/^DENIED:/) });
    rmSync(join(pkg, "tsconfig.json"));
    expect(await run(h, [guard, "--runtime", "claude"], bash(h, "env"))).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
    expect(existsSync(marker)).toBe(false);
  });

  test("a hoisted dependency still resolves as the package parser", async () => {
    const { h, pkg, guard } = install();
    mkdirSync(join(h, "node_modules"));
    renameSync(join(pkg, "node_modules/mvdan-sh"), join(h, "node_modules/mvdan-sh"));
    expect(await run(h, [guard, "--runtime", "claude"], bash(h, "env"))).toMatchObject({ exit: 2, stderr: expect.stringMatching(/^DENIED:/) });
  });

  test("a remapped parser fails the module identity check", async () => {
    const { h, pkg, guard } = install();
    writeFileSync(join(pkg, "tsconfig.json"), JSON.stringify({ compilerOptions: { baseUrl: ".", paths: { "mvdan-sh": ["./fake.ts"] } } }));
    writeFileSync(join(pkg, "fake.ts"), "export default { syntax: { NewParser: () => ({ Parse: () => ({ Stmts: [] }) }), KeepComments: () => 0, Variant: () => 0, LangBash: 0, Walk: () => {} } };\n");
    expect(await run(h, [guard, "--runtime", "claude"], bash(h, "env"))).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
    const direct = await run(h, [process.execPath, join(pkg, "src/guard.ts"), "--runtime", "claude", "--cwd", join(h, "project")], bash(h, "env"), pkg);
    expect(direct.stderr).toContain("mvdan-sh resolved outside its dependency package");
  });
});

describe("faults deny instead of passing", () => {
  test("the harness reaps a process when setup fails after spawn", async () => {
    const { h } = install();
    let pid = 0;
    try {
      await expect(
        run(h, ["/bin/sleep", "20"], {}, h, (spawned) => {
          pid = spawned;
          throw new Error("injected setup failure");
        }),
      ).rejects.toThrow("injected setup failure");
      expect(processExists(pid, true)).toBe(false);
    } finally {
      if (pid) stop(pid, true);
    }
  });

  const cases: [string, (h: string, pkg: string) => void][] = [
    ["bun missing", (h) => rmSync(join(h, "bin/bun"))],
    ["guard.ts missing", (_, pkg) => rmSync(join(pkg, "src/guard.ts"))],
    ["parser missing", (_, pkg) => rmSync(join(pkg, "node_modules/mvdan-sh"), { recursive: true })],
    ["exit 2 without a reason", (_, pkg) => appendFileSync(join(pkg, "src/guard.ts"), "\nprocess.exitCode = 2;\n")],
  ];
  test.each(cases)(
    "%s",
    async (_, fault) => {
      const { h, pkg, guard } = install();
      fault(h, pkg);
      const out = await run(h, [guard, "--runtime", "claude"], bash(h, "ls"));
      expect(out).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
      expect(out.seconds).toBeLessThan(5);
    },
    15_000,
  );

  test("a guard that never finishes loses its child and grandchild at the deadline", async () => {
    const { h, pkg, guard } = install();
    const pidFile = join(h, "guard.pid");
    const childPidFile = join(h, "child.pid");
    const grandchildPidFile = join(h, "grandchild.pid");
    appendFileSync(
      join(pkg, "src/guard.ts"),
      `\nconst child = Bun.spawn(["sh", "-c", ${JSON.stringify(`sleep 20 & echo $! > '${grandchildPidFile}'; wait`)}], { stdout: "inherit", stderr: "inherit" });\nawait Bun.write(${JSON.stringify(childPidFile)}, String(child.pid));\nawait Bun.write(${JSON.stringify(pidFile)}, String(process.pid));\nwhile (true) {}\n`,
    );
    const out = await run(h, [guard, "--runtime", "claude"], bash(h, "ls"));
    expect(out).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
    expect(out.seconds).toBeLessThan(4.5);
    const pid = Number(readFileSync(pidFile, "utf8"));
    expect(() => process.kill(pid, 0)).toThrow();
    const childPid = Number(readFileSync(childPidFile, "utf8"));
    expect(() => process.kill(childPid, 0)).toThrow();
    const grandchildPid = Number(readFileSync(grandchildPidFile, "utf8"));
    await Bun.sleep(500);
    expect(() => process.kill(grandchildPid, 0)).toThrow();
  }, 15_000);

  const malformed: [string, unknown][] = [
    ["a string tool_input", { tool_name: "Bash", tool_input: "cat .env" }],
    ["a command array", { tool_name: "Bash", tool_input: { command: ["cat", ".env"] } }],
    ["a Bash call without a command", { tool_name: "Bash", tool_input: {} }],
    ["a Read call that names no file_path", { tool_name: "Read", tool_input: { path: ".env" } }],
    ["a non-string Grep path", { tool_name: "Grep", tool_input: { path: 5 } }],
  ];
  test.each(malformed)("%s is denied", async (_, event) => {
    const { h, guard } = install();
    expect(await run(h, [guard, "--runtime", "claude"], event)).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
  });

  test("calls the guard has no field for still pass", async () => {
    const { h, guard } = install();
    const entry = [guard, "--runtime", "claude"];
    expect(await run(h, entry, { tool_name: "WebFetch", tool_input: {} })).toMatchObject({ exit: 0 });
    expect(await run(h, entry, { tool_name: "Grep", cwd: join(h, "project"), tool_input: { pattern: "x" } })).toMatchObject({ exit: 0 });
  });

  test("a relative HOME is denied before the guard starts", async () => {
    const { h, guard } = install();
    expect(await run(h, ["/usr/bin/env", "HOME=home", guard, "--runtime", "claude"], bash(h, "ls"))).toMatchObject({ exit: 2, stderr: expect.stringMatching(/\(HOME is not an absolute path\)/) });
  });

  test("a HOME with a trailing slash still protects App Data", async () => {
    const { h, guard } = install();
    const out = await run(h, ["/usr/bin/env", `HOME=${h}/`, guard, "--runtime", "claude"], bash(h, "ls ~/Library/Containers"));
    expect(out).toMatchObject({ exit: 2, stderr: expect.stringMatching(/^DENIED: This reads a protected macOS app-data directory/) });
  });

  test("a HOME spelled through a link still protects App Data behind a link", async () => {
    const { h, guard } = install();
    mkdirSync(join(h, "Library/Containers/com.x"), { recursive: true });
    symlinkSync(join(h, "Library/Containers"), join(h, "project/data-link"));
    const alias = `${h}-alias`;
    symlinkSync(h, alias);
    temporary.push(alias);
    const event = { tool_name: "Bash", cwd: join(alias, "project"), tool_input: { command: "cat data-link/com.x/a.txt" } };
    const out = await run(h, ["/usr/bin/env", `HOME=${alias}`, guard, "--runtime", "claude"], event);
    expect(out).toMatchObject({ exit: 2, stderr: expect.stringMatching(/^DENIED: This reads a protected macOS app-data directory/) });
  });

  test("the outer deadline kills the guard group when the supervisor stalls", async () => {
    const { h, pkg, guard } = install();
    const pidFile = join(h, "stalled-guard.pid");
    const runner = join(pkg, "src/runner.ts");
    const marker = "process.stdout.write(`AGENT_GUARD_PGID=${child.pid}\\n`);";
    writeFileSync(runner, readFileSync(runner, "utf8").replace(marker, `${marker}\nwhile (true) {}`));
    appendFileSync(join(pkg, "src/guard.ts"), `\nawait Bun.write(${JSON.stringify(pidFile)}, String(process.pid));\nwhile (true) {}\n`);
    const out = await run(h, [guard, "--runtime", "claude"], bash(h, "ls"), h, undefined, async () => {
      const pid = Number(readFileSync(pidFile, "utf8"));
      for (let i = 0; i < 20 && processExists(pid, false); i++) await Bun.sleep(25);
      expect(processExists(pid, false)).toBe(false);
    });
    expect(out).toMatchObject({ exit: 2, stderr: expect.stringMatching(blocked) });
    // Waiting up to 1 s for the process group ID plus the watchdog must stay under Pi's 4.5 s hook timeout.
    expect(out.seconds).toBeLessThan(3.5);
  }, 15_000);

  test("a child left running is stopped before the hook returns", async () => {
    const { h, pkg, guard } = install();
    const childPidFile = join(h, "child.pid");
    appendFileSync(
      join(pkg, "src/guard.ts"),
      `\nconst child = Bun.spawn(["sleep", "20"], { stdout: "ignore", stderr: "ignore" });\nawait Bun.write(${JSON.stringify(childPidFile)}, String(child.pid));\nchild.unref();\n`,
    );
    const out = await run(h, [guard, "--runtime", "claude"], bash(h, "ls"), h, undefined, () => {
      const pid = Number(readFileSync(childPidFile, "utf8"));
      expect(processExists(pid, false)).toBe(false);
    });
    expect(out.exit).toBe(0);
    expect(out.seconds).toBeLessThan(5);
  }, 15_000);
});
