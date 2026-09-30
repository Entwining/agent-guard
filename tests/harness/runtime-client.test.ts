import { expect, test } from "bun:test";
import { chmod, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;

test.each([
  ["runtime-suite.ts", ["pi", "claude"], "missing hook"],
  ["runtime-suite.ts", ["pi", "claude"], "incomplete observation"],
  ["runtime-codex.ts", ["codex"], "missing hook"],
])(
  "%s resolves %j from PATH and preserves %s evidence",
  async (driver, runtimes, observation) => {
    const outside = await mkdtemp(join(tmpdir(), "guard-runtime-client-"));
    try {
      const bin = join(outside, "bin");
      const trace = join(outside, "clients.jsonl");
      const output = join(outside, "report.json");
      const preload = join(outside, "client-boundary.ts");
      await mkdir(bin);
      await symlink(process.execPath, join(bin, "bun"));
      for (const runtime of runtimes) {
        const client = join(bin, runtime);
        const fixture = join(import.meta.dir, "../fixtures/runtime-client.ts");
        await writeFile(client, `#!/bin/sh\nexec ${[process.execPath, fixture, runtime, trace, observation].map(quote).join(" ")} "$@"\n`);
        await chmod(client, 0o700);
      }
      await writeFile(
        preload,
        `const spawn = Bun.spawn; Bun.spawn = (cmd, ...options) => { if (!${JSON.stringify(runtimes)}.includes(cmd[0])) throw new Error("Client must resolve from synthetic PATH"); return spawn(cmd, ...options); };`,
      );
      const child = Bun.spawn([process.execPath, "--preload", preload, join(import.meta.dir, driver), output], {
        env: { HOME: outside, TMPDIR: outside, PATH: `${bin}:/usr/bin:/bin` },
        stdout: "pipe",
        stderr: "pipe",
      });
      const [status, , stderr] = await Promise.all([child.exited, new Response(child.stdout).text(), new Response(child.stderr).text()]);
      expect(stderr).not.toContain("Client must resolve from synthetic PATH");
      expect(status).toBe(1);
      const launches = (await readFile(trace, "utf8"))
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line) as { runtime: string; args: string[]; cwd: string });
      expect([...new Set(launches.map((launch) => launch.runtime))].sort()).toEqual(runtimes.toSorted());
      for (const launch of launches) {
        expect(launch.args).toContain("Run the requested tool.");
        expect(launch.cwd).toEndWith("/workspace");
      }
      const report = (await Bun.file(output).json()) as { summary: unknown; records: { status: number; stdout: string; verdict: string }[] };
      expect(report.records.length).toBe(launches.length);
      for (const record of report.records) expect(record).toMatchObject({ status: 7, stdout: "SYNTHETIC_RUNTIME_CLIENT\n", verdict: "unverified" });
      if (driver === "runtime-suite.ts") {
        expect(report.summary).toEqual(expect.arrayContaining(runtimes.map((runtime) => expect.objectContaining({ runtime, verified: 0, trackedSurvivors: null, deterministic: null }))));
      } else {
        expect(report.summary).toMatchObject({ evaluationStatus: "unverified", verified: 0, deterministic: null, falseDeny: null, falseAllow: null });
      }
    } finally {
      await rm(outside, { recursive: true, force: true });
    }
  },
  15_000,
);
