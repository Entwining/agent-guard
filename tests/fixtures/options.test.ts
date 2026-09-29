import type { BehaviorRow } from "./types.test";

export default [
  { tool: "Bash", input: "node --env-file=$HOME/Library/Containers/x app.js", cwd: "$H/project", claude: 2, codex: 2, reason: "appdata", note: "a value glued to its option with = is a path" },
  { tool: "Bash", input: "git --work-tree=$HOME/Library/Containers status", cwd: "$H/project", claude: 2, codex: 2, reason: "appdata" },
  { tool: "Bash", input: "rg --ignore-file ~/Library/Containers/x foo src", cwd: "$H/project", claude: 2, codex: 2, reason: "appdata", note: "rg opens the ignore file" },
  { tool: "Bash", input: "grep --exclude-from ~/Library/Containers/x TOKEN src", cwd: "$H/project", claude: 2, codex: 2, reason: "appdata" },
  { tool: "Bash", input: "node --env-file=.env.example app.js", cwd: "$H/project", claude: 0, codex: 0 },
  { tool: "Bash", input: "rg -m 5 foo src", cwd: "$H/project", claude: 0, codex: 0 },
  { tool: "Bash", input: "rg --ignore-file .ignore foo src", cwd: "$H/project", claude: 0, codex: 0 },
  { tool: "Bash", input: "cat ~/Library/{C..C}ontainers/x", cwd: "$H", claude: 2, codex: 2, reason: "appdata", note: "the shell expands a brace sequence" },
  { tool: "Bash", input: "cat ~/Library/{1..1}CloudStorage/x", cwd: "$H", claude: 2, codex: 2, reason: "appdata" },
  { tool: "Bash", input: "cat ~/.e{n..n}v", cwd: "$H/project", claude: 2, codex: 2, reason: "file" },
  { tool: "Bash", input: "cat ~/.np{m..m}rc", cwd: "$H/project", claude: 2, codex: 2, reason: "file" },
  { tool: "Bash", input: "cat file{1..3}.txt", cwd: "$H/project", claude: 0, codex: 0 },
  { tool: "Bash", input: "echo {1..3}", cwd: "$H/project", claude: 0, codex: 0 },
] satisfies BehaviorRow[];
