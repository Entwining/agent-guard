import { appendFileSync } from "node:fs";

import { observeGuard } from "./runtime-observation";

const input = await Bun.stdin.text();
const result = await observeGuard(process.argv[2] ?? "claude", input);
appendFileSync(process.env["AGENT_GUARD_TEST_HOOK_TRACE"]!, JSON.stringify({ input: JSON.parse(input), ...result }) + "\n");
await Promise.all([
  new Promise<void>((resolve, reject) => process.stdout.write(result.stdout, (error) => (error ? reject(error) : resolve()))),
  new Promise<void>((resolve, reject) => process.stderr.write(result.stderr, (error) => (error ? reject(error) : resolve()))),
]);
if (result.spawnError) throw new Error(result.spawnError);
if (result.signal) process.kill(process.pid, result.signal);
else process.exitCode = result.status!;
