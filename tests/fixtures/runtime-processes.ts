import { appendFileSync } from "node:fs";

const role = process.argv.includes("--guard") ? "guard" : "runner";
const row = JSON.stringify({ pid: process.pid, ppid: process.ppid, started: String(Date.now()), role }) + "\n";
appendFileSync(process.env["AGENT_GUARD_TEST_PID_TRACE"]!, process.argv.includes("--duplicate") ? row.repeat(2) : row);
if (role === "guard") {
  await Bun.stdin.text();
  console.log("SYNTHETIC_CHECKER_OUTPUT");
} else {
  const child = Bun.spawn([process.execPath, import.meta.path, "--guard"], { stdin: "inherit", stdout: "inherit", stderr: "inherit" });
  process.exitCode = await child.exited;
}
