import { appendFileSync } from "node:fs";

import type { observeGuard } from "../harness/runtime-observation";

const [runtime, trace, observation, ...args] = process.argv.slice(2);
if (!runtime || !trace) throw new Error("Synthetic runtime requires its name and trace path");
appendFileSync(trace, JSON.stringify({ runtime, args, cwd: process.cwd() }) + "\n");
if (observation === "incomplete observation") {
  const hook: Awaited<ReturnType<typeof observeGuard>> & { input: { tool_input: { command: string } } } = {
    input: { tool_input: { command: "SYNTHETIC_COMMAND" } },
    status: 0,
    signal: null,
    stdout: "",
    stderr: "",
    spawnError: undefined,
    timedOut: false,
    ms: 0,
    observed: [],
    survivors: [],
    observationError: "SYNTHETIC_INCOMPLETE_PROCESS_TRACE",
  };
  appendFileSync(process.env["AGENT_GUARD_TEST_HOOK_TRACE"]!, JSON.stringify(hook) + "\n");
}
console.log("SYNTHETIC_RUNTIME_CLIENT");
process.exitCode = 7;
