import { expect, test } from "bun:test";
import { chmod, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { observeGuard } from "./runtime-observation";

test("process observations distinguish complete roles from an empty trace", async () => {
  const outside = await mkdtemp(join(tmpdir(), "guard-observation-"));
  const keys = ["AGENT_GUARD_TEST_ENTRY", "AGENT_GUARD_TEST_PID_TRACE", "AGENT_GUARD_TEST_ABLATE"];
  const previous = keys.map((key) => process.env[key]);
  try {
    const entry = join(outside, "entry");
    const trace = join(outside, "pids.jsonl");
    process.env[keys[0]!] = entry;
    process.env[keys[1]!] = trace;
    delete process.env[keys[2]!];
    const quote = (value: string) => `'${value.replaceAll("'", "'\\''")}'`;
    const fixture = join(import.meta.dir, "../fixtures/runtime-processes.ts");
    await writeFile(entry, `#!/bin/sh\nexec ${[process.execPath, fixture].map(quote).join(" ")} "$@"\n`);
    await chmod(entry, 0o700);
    await writeFile(trace, "");
    const complete = await observeGuard("claude", "{}");
    expect(complete).toMatchObject({ status: 0, stdout: "SYNTHETIC_CHECKER_OUTPUT\n", survivors: [], observationError: undefined });
    expect(complete.observed.map((row) => row.role)).toEqual(["entry", "runner", "guard"]);

    await writeFile(entry, `#!/bin/sh\nexec ${[process.execPath, fixture, "--duplicate"].map(quote).join(" ")} "$@"\n`);
    await writeFile(trace, "");
    const duplicated = await observeGuard("claude", "{}");
    expect(duplicated).toMatchObject({ status: 0, stdout: "SYNTHETIC_CHECKER_OUTPUT\n", survivors: [] });
    expect(duplicated.observationError).toContain("PID trace is incomplete");

    await writeFile(entry, "#!/bin/sh\ncat >/dev/null\nprintf 'SYNTHETIC_ENTRY_OUTPUT\\n'\n");
    await writeFile(trace, "");
    const incomplete = await observeGuard("claude", "{}");
    expect(incomplete).toMatchObject({ status: 0, stdout: "SYNTHETIC_ENTRY_OUTPUT\n", survivors: [] });
    expect(incomplete.observationError).toContain("PID trace is incomplete");
  } finally {
    keys.forEach((key, index) => {
      const value = previous[index];
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    });
    await rm(outside, { recursive: true, force: true });
  }
});
