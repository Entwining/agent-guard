// A search option's value keeps the option's word as option:ROLE.
export type ValueRole = "pattern" | "patfile" | "glob" | "nglob" | "optarg";
export type Role = "arg" | "assign" | "precommand" | "namespace" | "program" | "code" | "option" | "path" | ValueRole | `option:${ValueRole}`;
export type Flag = "explicit" | "files" | "help" | "fixed" | "recursive" | "include" | "replace" | "hidden";

export interface Word {
  text: string; // quotes removed; ~, $HOME and ${HOME} expanded
  raw: string;
  expands: boolean; // holds an expansion the front end cannot resolve
  globs: boolean; // holds an unquoted glob character
  vars: string[]; // parameter names the shell expands
  role: Role; // set by argv.ts
  value: string; // an option's value without its flag, set by argv.ts
}

export interface Redirect {
  direction: "in" | "out" | "herestring" | "heredoc";
  target: string; // file, word, or heredoc body
  vars: string[]; // parameters the shell expands in the target or body
}

export interface Command {
  argv: Word[];
  redirects: Redirect[];
  cwd: string; // after the cd commands that run before it in the same shell
  program: number; // argv index of the word that runs, -1 when nothing runs
  wrappers: string[];
  shell: boolean; // the program runs in this shell rather than behind a wrapper
  flags: Set<Flag>;
}

export interface Script {
  commands: Command[];
  uninspectable: string[];
  parseFailed: boolean;
}

export type Runtime = "claude" | "codex" | "pi";
export type Tool = "bash" | "read" | "edit" | "write" | "grep";

export interface Request {
  runtime: Runtime;
  tool: Tool;
  home: string;
  cwd: string;
  inputCwd: string;
  pathInput: string;
  operation: "" | "read" | "write" | "search";
  target: string;
  searchRoot: string;
  glob: string;
  commands: Command[];
  uninspectable: string[];
  parseFailed: boolean;
}
