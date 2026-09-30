import { expect, test } from "bun:test";
import { mkdtemp, rm, symlink } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";

import { resultPath } from "./runtime-output";

const checkout = resolve(import.meta.dir, "../..");
const experiment = join(checkout, "experiments/read-enforcement-comparison.py");

test("runtime reports reject checkout paths and symlink aliases", async () => {
  const outside = await mkdtemp(join(tmpdir(), "guard-output-"));
  try {
    const alias = join(outside, "checkout");
    const protectedAlias = join(outside, "protected");
    await symlink(checkout, alias);
    await symlink(join(homedir(), "Library/Containers"), protectedAlias);
    expect(resultPath(join(outside, "result.json"), "test")).toBe(join(outside, "result.json"));
    expect(() => resultPath(join(checkout, "tests/harness/result.json"), "test")).toThrow();
    expect(() => resultPath(join(checkout, "tests/harness", "..", "result.json"), "test")).toThrow();
    expect(() => resultPath(join(alias, "result.json"), "test")).toThrow();
    expect(() => resultPath(join(protectedAlias, "result.json"), "test")).toThrow();
  } finally {
    await rm(outside, { recursive: true, force: true });
  }
});

test.each([
  ["runtime-suite.ts", []],
  ["runtime-codex.ts", []],
  ["runtime-paired-runs.ts", ["missing-normal.json", "missing-baseline.json"]],
])("%s rejects checkout output before starting", async (driver, prefix) => {
  const output = join(checkout, "tests/harness/should-not-exist.json");
  const child = Bun.spawn([process.execPath, join(import.meta.dir, driver), ...prefix, output], { stdout: "pipe", stderr: "pipe" });
  const [status, stderr] = await Promise.all([child.exited, new Response(child.stderr).text()]);
  expect(status).not.toBe(0);
  expect(stderr).toContain("Result path must stay outside the checkout");
  expect(await Bun.file(output).exists()).toBe(false);
});

test("experiment rejects checkout output before creating artifacts", async () => {
  const output = join(checkout, "experiments", "should-not-exist.jsonl");
  const child = Bun.spawn(
    ["python3", "-B", experiment, "--guard-mode", "off", "--os-mode", "off", "--form", "direct_operand", "--synthetic-home", join(tmpdir(), "guard-output-unused-home"), "--output", output],
    {
      stdout: "pipe",
      stderr: "pipe",
    },
  );
  const [status, stderr] = await Promise.all([child.exited, new Response(child.stderr).text()]);
  expect(status).toBe(2);
  expect(stderr).toContain("output must stay outside the checkout");
  expect(await Bun.file(output).exists()).toBe(false);
  expect(await Bun.file(`${output}.artifacts`).exists()).toBe(false);
});

test("experiment path boundary accepts outside output and rejects a checkout alias", async () => {
  const outside = await mkdtemp(join(tmpdir(), "guard-experiment-output-"));
  try {
    const alias = join(outside, "checkout");
    const protectedAlias = join(outside, "protected");
    await symlink(checkout, alias);
    await symlink(join(homedir(), "Library/Containers"), protectedAlias);
    const script = `import pathlib, runpy, sys\nf = runpy.run_path(sys.argv[1])["result_path"]\ntry:\n print(f(pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]), pathlib.Path(sys.argv[4])))\nexcept ValueError as error:\n print(error, file=sys.stderr)\n sys.exit(2)`;
    const run = async (path: string) => {
      const child = Bun.spawn(["python3", "-B", "-c", script, experiment, path, homedir(), checkout], { stdout: "pipe", stderr: "pipe" });
      const [status, stdout, stderr] = await Promise.all([child.exited, new Response(child.stdout).text(), new Response(child.stderr).text()]);
      return { status, stdout, stderr };
    };
    const accepted = await run(join(outside, "result.jsonl"));
    expect(accepted.status).toBe(0);
    expect(accepted.stdout.trimEnd()).toEndWith("/result.jsonl");
    const rejected = await run(join(alias, "result.jsonl"));
    expect(rejected.status).toBe(2);
    expect(rejected.stderr).toContain("output must stay outside the checkout");
    const protectedResult = await run(join(protectedAlias, "result.jsonl"));
    expect(protectedResult.status).toBe(2);
  } finally {
    await rm(outside, { recursive: true, force: true });
  }
});
