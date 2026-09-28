// Parse shell structure into command records; rejected syntax remains
// available to the signature rules.
import sh from "mvdan-sh";
import type { Assign, BinaryCmd, CallExpr, CmdSubst, DeclClause, IfClause, Lit, Node, ParamExp, Redirect as RedirectNode, Stmt, Subshell, Word as WordNode } from "mvdan-sh";
import { resolve } from "node:path";
import { resolveCommand, stdinKind } from "./argv.ts";
import { misreadComment } from "./comments.ts";
import { boundedDirectories } from "./cwd.ts";
import { movedOnSuccess } from "./moves.ts";
import { expandHome } from "./paths.ts";
import { xargsReplacements } from "./pipeline.ts";
import type { Command, Redirect, Script, Word } from "./record.ts";
import { readWord } from "./words.ts";

const { syntax } = sh;
const parser = syntax.NewParser(syntax.KeepComments(true), syntax.Variant(syntax.LangBash));

interface Scope {
  readonly dir: { cwd: string; previous?: string; alternatives?: string[] }; // shared by the commands a cd in this scope moves
  readonly vars: Map<string, string>;
  readonly slice: (start: number, end: number) => string;
}

const type = (node: Node) => syntax.NodeType(node);

export function parseScript(source: string, cwd: string, home: string): Script {
  const script: Script = { commands: [], uninspectable: [] };

  function parse(src: string, cwd: string) {
    const bytes = Buffer.from(src);
    let file;
    try {
      file = parser.Parse(src, "");
    } catch {
      file = undefined;
    }
    if (!file || misreadComment(file, bytes)) {
      script.uninspectable.push(src);
      return;
    }
    const slice = (start: number, end: number) => bytes.subarray(start, end).toString();
    const scope: Scope = { dir: { cwd }, vars: new Map(), slice };
    for (const stmt of file.Stmts) statement(stmt, scope);
  }

  const text = (node: Node, scope: Scope) => scope.slice(node.Pos().Offset(), node.End().Offset());
  // A cd inside runs in a separate shell and leaves this scope's cwd alone.
  const isolated = (scope: Scope): Scope => ({ ...scope, dir: { ...scope.dir }, vars: new Map(scope.vars) });

  function statement(stmt: Stmt, outer: Scope) {
    const scope = stmt.Background ? isolated(outer) : outer;
    const cmd = stmt.Cmd;
    const kind = cmd && type(cmd);
    if (kind === "CallExpr" || kind === "DeclClause") return simple(cmd, stmt.Redirs, scope);
    // Redirections on a compound command, or with no command at all, still
    // open their files.
    if (stmt.Redirs.length) simple(null, stmt.Redirs, scope);
    if (!cmd) return;
    if (kind === "BinaryCmd") return binary(cmd as BinaryCmd, scope);
    if (kind === "Subshell") {
      const inner = isolated(scope);
      for (const s of (cmd as Subshell).Stmts) statement(s, inner);
      return;
    }
    children(cmd, scope);
  }

  // Only the left side of && moves the directory the right side runs in.
  function binary(cmd: BinaryCmd, scope: Scope) {
    const op = scope.slice(cmd.OpPos.Offset(), cmd.OpPos.Offset() + 2).trim();
    const failed = isolated(scope);
    const start = script.commands.length;
    statement(cmd.X, op === "&&" || op === "||" ? scope : isolated(scope));
    const middle = script.commands.length;
    const directCd = op === "&&" && movedOnSuccess(cmd.X, scope.slice);
    if (directCd) {
      const old = [failed.dir.cwd, ...(failed.dir.alternatives ?? [])];
      scope.dir.alternatives = scope.dir.alternatives?.filter((cwd) => !old.includes(cwd));
      statement(cmd.Y, scope);
      scope.dir.alternatives = boundedDirectories(scope.dir.cwd, [...(scope.dir.alternatives ?? []), ...old], home);
      return;
    }
    statement(cmd.Y, op === "||" ? failed : op === "&&" ? scope : isolated(scope));
    if (op === "|") for (const item of xargsReplacements(script.commands.slice(start, middle), script.commands.slice(middle))) parse(item.source, item.cwd);
    if (op === "||") {
      scope.dir.alternatives = boundedDirectories(scope.dir.cwd, [...(scope.dir.alternatives ?? []), failed.dir.cwd, ...(failed.dir.alternatives ?? [])], home);
    }
  }

  // Compound words may hold substitutions; branch bodies may not run.
  function children(node: Node, scope: Scope) {
    if (type(node) === "FuncDecl") scope = isolated(scope);
    if (type(node) === "IfClause") return conditional(node as IfClause, scope);
    const outer = scope;
    const loop = type(node) === "WhileClause" || type(node) === "ForClause";
    if (loop) scope = isolated(scope);
    syntax.Walk(node, (child) => {
      if (!child || child === node) return true;
      const kind = type(child);
      if (kind === "Stmt") statement(child as Stmt, scope);
      else if (kind === "Word") {
        const value = word(child as WordNode, scope);
        for (const cwd of [scope.dir.cwd, ...(scope.dir.alternatives ?? [])]) {
          script.commands.push({ argv: [value], redirects: [], cwd, program: -1, wrappers: [], shell: true, flags: new Set() });
        }
      }
      else return true;
      return false;
    });
    if (loop) outer.dir.alternatives = boundedDirectories(outer.dir.cwd, [...(outer.dir.alternatives ?? []), scope.dir.cwd, ...(scope.dir.alternatives ?? [])], home);
  }

  function conditional(node: IfClause, scope: Scope) {
    const condition = isolated(scope);
    for (const stmt of node.Cond) statement(stmt, condition);
    const then = isolated(condition);
    for (const stmt of node.Then) statement(stmt, then);
    const branches = [scope, condition, then];
    if (node.Else) {
      const otherwise = isolated(condition);
      conditional(node.Else, otherwise);
      branches.push(otherwise);
    }
    scope.dir.alternatives = boundedDirectories(scope.dir.cwd, branches.flatMap((branch) => [branch.dir.cwd, ...(branch.dir.alternatives ?? [])]), home);
  }

  function expansions(node: Node, scope: Scope, vars: string[]) {
    syntax.Walk(node, (child) => {
      if (!child) return true;
      const kind = type(child);
      if (kind === "ParamExp") vars.push((child as ParamExp).Param.Value);
      if (kind !== "CmdSubst" && kind !== "ProcSubst") return true;
      const inner = isolated(scope);
      for (const s of (child as CmdSubst).Stmts) statement(s, inner);
      return false;
    });
  }

  function word(node: WordNode, scope: Scope): Word {
    return readWord(node, scope.slice, scope.vars, home, (part, names) => expansions(part, scope, names));
  }

  function assign(node: Assign, scope: Scope, role: "assign" | "arg"): Word {
    const name = node.Name?.Value ?? "";
    const value = node.Value ? word(node.Value, scope) : undefined;
    // Array and index expressions can hold substitutions outside the value.
    syntax.Walk(node, (child) => {
      if (!child || child === node) return true;
      if (child === node.Value) return false;
      if (type(child) === "Word") word(child as WordNode, scope);
      return type(child) !== "Word";
    });
    return {
      ...(value ?? { expands: false, globs: false, vars: [] }),
      text: `${name}=${value?.text ?? ""}`,
      raw: text(node, scope),
      role,
      value: `${name}=${value?.text ?? ""}`,
    };
  }

  function literal(value: string): Word {
    return { text: value, raw: value, expands: false, globs: false, vars: [], role: "arg", value };
  }

  function simple(cmd: Node | null, redirs: RedirectNode[], scope: Scope) {
    const argv: Word[] = [];
    if (cmd && type(cmd) === "CallExpr") {
      for (const a of (cmd as CallExpr).Assigns) argv.push(assign(a, scope, "assign"));
      for (const w of (cmd as CallExpr).Args) argv.push(word(w, scope));
    } else if (cmd) {
      const decl = cmd as DeclClause;
      argv.push(literal(decl.Variant.Value));
      for (const a of decl.Args) {
        if (!a.Naked) argv.push(assign(a, scope, "arg"));
        else argv.push(a.Value ? word(a.Value, scope) : literal(a.Name?.Value ?? ""));
      }
    }
    const redirects = redirs.map((r) => redirect(r, scope)).filter((r) => r !== undefined);
    const command: Command = {
      argv,
      redirects,
      cwd: scope.dir.cwd,
      program: -1,
      wrappers: [],
      shell: true,
      flags: new Set(),
    };
    script.commands.push(command);
    const { children: sources, code } = resolveCommand(command, home);
    for (const cwd of scope.dir.alternatives ?? []) script.commands.push({ ...command, cwd });
    for (const src of sources) for (const cwd of [command.cwd, ...(scope.dir.alternatives ?? [])]) parse(src, cwd);
    script.uninspectable.push(...code);
    for (const r of redirects) {
      if (r.direction !== "heredoc" && r.direction !== "herestring") continue;
      const stdin = stdinKind(command);
      if (stdin === "shell") parse(r.target, command.cwd);
      if (stdin === "code") script.uninspectable.push(r.target);
    }
    track(command, scope);
  }

  function redirect(node: RedirectNode, scope: Scope): Redirect | undefined {
    const op = scope.slice(node.OpPos.Offset(), node.Word.Pos().Offset()).trim();
    if (op === "<<" || op === "<<-") {
      const quoted = node.Word.Parts.some((p) => type(p) !== "Lit" || (p as Lit).Value.includes("\\"));
      const vars: string[] = [];
      if (node.Hdoc && !quoted) expansions(node.Hdoc, scope, vars);
      return { direction: "heredoc", target: node.Hdoc ? text(node.Hdoc, scope) : "", vars };
    }
    const target = word(node.Word, scope);
    if (op === "<<<") return { direction: "herestring", target: target.text, vars: target.vars };
    if ((op === "<&" || op === ">&") && /^(\d+|-)$/.test(target.text)) return undefined;
    const direction = op === "<" || op === "<>" ? "in" : "out";
    return { direction, target: target.text, vars: [] };
  }

  function track(command: Command, scope: Scope) {
    const program = command.argv[command.program];
    const declaration = command.shell && ["export", "local", "declare", "typeset"].includes(program?.text ?? "");
    for (const word of command.argv.filter((w) => ((command.program < 0 && w.role === "precommand") || (declaration && w.role === "arg")) && /^[A-Za-z_][A-Za-z0-9_]*=/.test(w.text))) {
      const at = word.text.indexOf("=");
      if (!word.expands) scope.vars.set(word.text.slice(0, at), expandHome(word.text.slice(at + 1), home));
    }
    if (!program || !command.shell || !["cd", "pushd"].includes(program.text)) return;
    let target = command.argv.slice(command.program + 1).find((w) => !w.text.startsWith("-"))?.text;
    const previous = command.argv.slice(command.program + 1).some((w) => w.text === "-");
    if (previous) target = scope.dir.previous;
    if (previous && target === undefined) return;
    if (target === undefined && program.text === "cd") target = home;
    if (target !== undefined) {
      scope.dir.alternatives = boundedDirectories(resolve(scope.dir.cwd, target), [scope.dir.cwd, ...(scope.dir.alternatives ?? []), ...(scope.dir.alternatives ?? []).map((cwd) => resolve(cwd, target))], home);
      scope.dir.previous = scope.dir.cwd;
      scope.dir.cwd = resolve(scope.dir.cwd, target);
    }
  }

  parse(source, cwd);
  return script;
}
