import { appendFileSync, existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const pids = join(process.env["HOME"]!, "checker-pids");
const ready = join(process.env["HOME"]!, "checker-ready");
const record = (pid: number) => {
  appendFileSync(pids, `${pid}\n`);
  appendFileSync(process.env["AGENT_GUARD_TEST_PIDS"]!, `P:${pid}\n`);
};

record(process.pid);

if (process.argv.includes("--descendant")) {
  const child = Bun.spawn(["/bin/sleep", "20"], { stdout: "ignore", stderr: "ignore" });
  record(child.pid);
  writeFileSync(ready, "");
  await child.exited;
} else {
  Bun.spawn([process.execPath, import.meta.path, "--descendant"], { stdout: "ignore", stderr: "ignore" });
  while (!existsSync(ready)) await Bun.sleep(5);
  await Bun.sleep(20_000);
}
