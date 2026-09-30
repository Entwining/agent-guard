import { afterAll, expect, test } from "bun:test";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { exists, instrumentEntry, instrumentRunner, run } from "./hook-process";

const source = join(import.meta.dir, "..");
const temporary: string[] = [];
afterAll(() => temporary.forEach((dir) => rmSync(dir, { recursive: true, force: true })));

function install(beforeSpawn: string, afterSpawn = "", descendants = false) {
  const h = realpathSync(mkdtempSync(join(tmpdir(), "agent-guard-group-")));
  temporary.push(h);
  const pkg = join(h, "package");
  for (const dir of ["bin", "package/bin", "package/src"]) mkdirSync(join(h, dir), { recursive: true });
  const entry = join(pkg, "bin/agent-guard");
  cpSync(join(source, "bin/agent-guard"), entry);
  instrumentEntry(entry);
  cpSync(join(source, "tsconfig.json"), join(pkg, "tsconfig.json"));
  symlinkSync(process.execPath, join(h, "bin/bun"));
  const runner = readFileSync(join(source, "src/runner.ts"), "utf8");
  const spawn = /(const child = Bun\.spawn\(\{[\s\S]*?\}\);)/;
  if (!spawn.test(runner)) throw new Error("Runner spawn injection point missing");
  const injected = runner.replace(spawn, `$1\n${afterSpawn}`).replace("const child = Bun.spawn", `${beforeSpawn}\nconst child = Bun.spawn`);
  writeFileSync(join(pkg, "src/runner.ts"), 'import { existsSync } from "node:fs";\n' + injected);
  instrumentRunner(join(pkg, "src/runner.ts"));
  if (descendants) cpSync(join(import.meta.dir, "fixtures/entry-descendants.ts"), join(pkg, "src/guard.ts"));
  else writeFileSync(join(pkg, "src/guard.ts"), "JSON.parse(await Bun.stdin.text());\nprocess.exit(0);\n");
  return { h, pkg, entry };
}

test("the entry passes the event to the checker through a pipe", async () => {
  const { h, pkg, entry } = install("");
  writeFileSync(join(pkg, "src/guard.ts"), 'import { fstatSync } from "node:fs";\nconsole.log(JSON.stringify({ pipe: fstatSync(0).isFIFO(), event: JSON.parse(await Bun.stdin.text()) }));\n');
  const event = { marker: "synthetic input" };
  const result = await run(h, [entry, "--runtime", "claude"], event);
  expect(result).toMatchObject({ exit: 0, stdout: `${JSON.stringify({ pipe: true, event })}\n`, stderr: "" });
});

test("a runner starting after one second still passes within the total deadline", async () => {
  const { h, entry } = install("await Bun.sleep(1500);");
  const result = await run(h, [entry, "--runtime", "claude"], {});
  expect(result).toMatchObject({ exit: 0, stdout: "", stderr: "" });
  expect(result.seconds).toBeGreaterThan(1);
  expect(result.seconds).toBeLessThan(3.5);
}, 15_000);

const ready = "while (!existsSync(`${process.env.HOME}/checker-ready`)) await Bun.sleep(5);\n";
test("an early runner failure denies and stops its checker and descendants", async () => {
  const { h, entry } = install("", `${ready}process.exit(7);`, true);
  const result = await run(h, [entry, "--runtime", "claude"], {}, h, undefined, async () => {
    const pids = readFileSync(join(h, "checker-pids"), "utf8").trim().split("\n").map(Number);
    expect(pids).toHaveLength(3);
    for (let attempt = 0; attempt < 20 && pids.some((pid) => exists(pid, false)); attempt++) await Bun.sleep(25);
    expect(pids.filter((pid) => exists(pid, false))).toEqual([]);
  });
  expect(result).toMatchObject({ exit: 2, stdout: "", stderr: expect.stringContaining("so this call is blocked") });
  expect(result.seconds).toBeLessThan(3.5);
}, 15_000);

test("a runner stalled during startup is stopped at the total deadline", async () => {
  const { h, entry } = install("await Bun.sleep(20_000);");
  const result = await run(h, [entry, "--runtime", "claude"], {});
  expect(result).toMatchObject({ exit: 2, stdout: "", stderr: expect.stringContaining("so this call is blocked") });
  expect(result.seconds).toBeGreaterThan(2.5);
  expect(result.seconds).toBeLessThan(3.5);
}, 15_000);
