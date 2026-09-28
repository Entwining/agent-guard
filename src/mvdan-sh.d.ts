// The mvdan-sh package ships no types. This declares only the AST subset the
// front end reads, so the package needs no type dependency. Positions are
// UTF-8 byte offsets into the parsed source.
declare module "mvdan-sh" {
  export interface Pos {
    Offset(): number;
  }
  export interface Node {
    Pos(): Pos;
    End(): Pos;
  }
  export interface Lit extends Node {
    Value: string;
  }
  export interface Comment extends Node {
    Hash: Pos;
  }
  export interface Word extends Node {
    Parts: Node[];
  }
  export interface Assign extends Node {
    Naked: boolean;
    Name: Lit | null;
    Value: Word | null;
  }
  export interface Redirect extends Node {
    OpPos: Pos;
    Word: Word;
    Hdoc: Word | null;
  }
  export interface Stmt extends Node {
    Cmd: Node | null;
    Background: boolean;
    Redirs: Redirect[];
  }
  export interface File extends Node {
    Stmts: Stmt[];
  }
  export interface CallExpr extends Node {
    Assigns: Assign[];
    Args: Word[];
  }
  export interface DeclClause extends Node {
    Variant: Lit;
    Args: Assign[];
  }
  export interface BinaryCmd extends Node {
    OpPos: Pos;
    X: Stmt;
    Y: Stmt;
  }
  export interface Subshell extends Node {
    Stmts: Stmt[];
  }
  export interface IfClause extends Node {
    Cond: Stmt[];
    Then: Stmt[];
    Else: IfClause | null;
  }
  export interface SglQuoted extends Node {
    Dollar: boolean;
    Value: string;
  }
  export interface DblQuoted extends Node {
    Parts: Node[];
  }
  export interface ParamExp extends Node {
    Excl: boolean;
    Length: boolean;
    Width: boolean;
    Param: Lit;
    Index: Node | null;
    Slice: object | null;
    Repl: object | null;
    Exp: object | null;
  }
  export interface CmdSubst extends Node {
    Stmts: Stmt[];
  }

  const sh: {
    syntax: {
      NewParser(...options: unknown[]): { Parse(source: string, name: string): File };
      NodeType(node: Node): string;
      Walk(node: Node, visit: (node: Node | null) => boolean): void;
      KeepComments(keep: boolean): unknown;
      Variant(language: unknown): unknown;
      LangBash: unknown;
    };
  };
  export default sh;
}
