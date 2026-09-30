import { afterAll, expect, test } from "bun:test";
import { chmod, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const home = await mkdtemp(join(tmpdir(), "guard-runtime-fidelity-"));
const entry = join(home, "synthetic-entry");
const trace = join(home, "hook-trace.jsonl");
const pidTrace = join(home, "pids.jsonl");

await writeFile(entry, '#!/bin/sh\ncat >/dev/null\nprintf "synthetic advice\\n"\nprintf "synthetic reason\\n" >&2\nexit "$AGENT_GUARD_TEST_EXIT"\n');
await chmod(entry, 0o700);
await writeFile(pidTrace, "");

afterAll(async () => {
  await rm(home, { recursive: true, force: true });
});

for (const exit of [0, 2, 7]) {
  test(`Runtime hook preserves entry status and both output streams: ${exit}`, async () => {
    await writeFile(trace, "");
    const child = Bun.spawn([process.execPath, join(import.meta.dir, "runtime-hook-driver.ts"), "claude"], {
      cwd: home,
      env: {
        ...process.env,
        HOME: home,
        AGENT_GUARD_TEST_ENTRY: entry,
        AGENT_GUARD_TEST_HOOK_TRACE: trace,
        AGENT_GUARD_TEST_PID_TRACE: pidTrace,
        AGENT_GUARD_TEST_EXIT: String(exit),
      },
      stdin: "pipe",
      stdout: "pipe",
      stderr: "pipe",
    });
    child.stdin.write(JSON.stringify({ tool_name: "Bash", tool_input: { command: "echo synthetic" } }));
    child.stdin.end();

    const [status, stdout, stderr] = await Promise.all([child.exited, new Response(child.stdout).text(), new Response(child.stderr).text()]);
    const records = (await readFile(trace, "utf8"))
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line));

    expect(status).toBe(exit);
    expect(stdout).toBe("synthetic advice\n");
    expect(stderr).toBe("synthetic reason\n");
    expect(records).toHaveLength(1);
    expect(records[0]).toMatchObject({ status: exit, stdout: "synthetic advice\n", stderr: "synthetic reason\n" });
  });
}
