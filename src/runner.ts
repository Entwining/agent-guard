const child = Bun.spawn({
  cmd: [process.execPath, `${import.meta.dir}/guard.ts`, ...process.argv.slice(2)],
  stdin: "inherit",
  stdout: "pipe",
  stderr: "pipe",
});
const forward = async (stream: ReadableStream<Uint8Array>) => {
  for await (const chunk of stream) process.stdout.write(chunk);
};
const output = Promise.all([forward(child.stdout), forward(child.stderr)]);

const exit = await child.exited;
await output;
process.exitCode = exit;
