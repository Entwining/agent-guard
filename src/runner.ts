// A separate process group lets the deadline stop children inheriting hook pipes.
const child = Bun.spawn({
  cmd: [process.execPath, `${import.meta.dir}/guard.ts`, ...process.argv.slice(2)],
  stdin: "inherit",
  stdout: "pipe",
  stderr: "pipe",
  detached: true,
});
process.stdout.write(`AGENT_GUARD_PGID=${child.pid}\n`);
const forward = async (stream: ReadableStream<Uint8Array>) => {
  for await (const chunk of stream) process.stdout.write(chunk);
};
const output = Promise.all([forward(child.stdout), forward(child.stderr)]);

const stopGroup = () => {
  try {
    process.kill(-child.pid, "SIGKILL");
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
  }
};

let expired = false;
const deadline = setTimeout(() => {
  expired = true;
  stopGroup();
}, 2800);
const exit = await child.exited;
clearTimeout(deadline);
stopGroup();
await output;
process.exitCode = expired ? 1 : exit;
