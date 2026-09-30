// The only filesystem calls the guard makes about a path taken from a command. Tests replace these to record each probed path and check that
// none of them resolves into App Data, so every caller must reach the filesystem through here.
import { readlinkSync, statSync } from "node:fs";

export const probes = { readlink: readlinkSync, stat: statSync };
