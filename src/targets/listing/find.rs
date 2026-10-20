use crate::record::Word;
use std::collections::BTreeMap;

/// Name patterns that cover the files an action handles, each of which
/// matches one of them; `None` is unbounded. Every filter that passed before
/// the action contributes its patterns, so fixed text that identifies a
/// protected name stays checked beside a wider filter.
type Bound = Option<Vec<String>>;

/// What an action handles: the names that bound it, and whether a `-type f`
/// test passed before it, so that no name is a directory to walk below.
#[derive(Clone, Default)]
pub(super) struct Handed {
    pub names: Bound,
    pub files: bool,
}

/// The name bounds a `find` expression places on what it hands on.
pub(super) struct Bounds {
    /// The files each `-exec`-family action receives, by the index of its
    /// action word.
    pub execs: BTreeMap<usize, Handed>,
    /// The names it prints to standard output.
    pub printed: Handed,
}

/// Bounds for the expression starting at `start`, or `None` when it is empty
/// or uses a primary or syntax this model does not know.
pub(super) fn bounds(args: &[Word], start: usize) -> Option<Bounds> {
    let mut parser = Parser {
        args,
        at: start,
        printed: Vec::new(),
        execs: BTreeMap::new(),
        actions: false,
    };
    let whole = parser.or(&Handed::default(), 0)?;
    if parser.at != args.len() {
        return None;
    }
    // Without an action, find prints every name the expression accepts.
    let printed = if parser.actions {
        Handed {
            files: parser.printed.iter().all(|handed| handed.files),
            names: parser
                .printed
                .into_iter()
                .map(|handed| handed.names)
                .collect::<Option<Vec<_>>>()
                .map(|bounds| bounds.concat()),
        }
    } else {
        whole
    };
    Some(Bounds {
        execs: parser.execs,
        printed,
    })
}

struct Parser<'a> {
    args: &'a [Word],
    at: usize,
    printed: Vec<Handed>,
    execs: BTreeMap<usize, Handed>,
    actions: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.args.get(self.at).map(|word| word.as_str())
    }

    /// A disjunction is bounded only when every branch is, and an action in
    /// one branch learns nothing from the branches that failed before it.
    fn or(&mut self, guard: &Handed, depth: usize) -> Option<Handed> {
        let mut branches = vec![self.and(guard, depth)?];
        while matches!(self.peek(), Some("-o" | "-or")) {
            self.at += 1;
            branches.push(self.and(guard, depth)?);
        }
        Some(Handed {
            files: branches.iter().all(|branch| branch.files),
            names: branches
                .into_iter()
                .map(|branch| branch.names)
                .collect::<Option<Vec<_>>>()
                .map(|bounds| bounds.concat()),
        })
    }

    /// Terms run left to right, so an action is bounded only by the terms
    /// that passed before it, and each term bounds the conjunction.
    fn and(&mut self, guard: &Handed, depth: usize) -> Option<Handed> {
        let mut guard = guard.clone();
        let mut bound = Handed::default();
        let mut terms = 0;
        loop {
            match self.peek() {
                None | Some(")" | "-o" | "-or") => break,
                Some("-a" | "-and") => {
                    self.at += 1;
                    continue;
                }
                _ => {}
            }
            let term = self.unary(&guard, depth)?;
            terms += 1;
            if let Some(names) = term.names {
                bound
                    .names
                    .get_or_insert_with(Vec::new)
                    .extend(names.iter().cloned());
                guard.names.get_or_insert_with(Vec::new).extend(names);
            }
            bound.files |= term.files;
            guard.files |= term.files;
        }
        (terms > 0).then_some(bound)
    }

    fn unary(&mut self, guard: &Handed, depth: usize) -> Option<Handed> {
        if depth > crate::limits::MAX_NESTING {
            return None;
        }
        match self.peek()? {
            "!" | "-not" => {
                self.at += 1;
                self.unary(guard, depth + 1)?;
                Some(Handed::default())
            }
            "(" => {
                self.at += 1;
                let bound = self.or(guard, depth + 1)?;
                (self.peek()? == ")").then(|| self.at += 1)?;
                Some(bound)
            }
            _ => self.primary(guard),
        }
    }

    fn value(&mut self) -> Option<&Word> {
        let value = self.args.get(self.at)?;
        self.at += 1;
        Some(value)
    }

    fn primary(&mut self, guard: &Handed) -> Option<Handed> {
        let index = self.at;
        let primary = self.value()?.as_str();
        match primary {
            "-name" | "-iname" | "-path" | "-ipath" | "-wholename" | "-iwholename" => {
                let filter = self.value()?;
                // A filter the shell fills in at run time selects unknown names.
                if filter.expands || filter.runtime_unknown {
                    return Some(Handed::default());
                }
                return Some(Handed {
                    names: crate::filesystem::name_bounds(filter).map(|mut names| {
                        if !names.contains(&filter.text) {
                            names.push(filter.text.clone());
                        }
                        names
                    }),
                    files: false,
                });
            }
            "-type" => {
                let kind = self.value()?;
                return Some(Handed {
                    names: None,
                    files: kind == "f",
                });
            }
            // A format can print any text.
            "-printf" => {
                self.value()?;
                self.actions = true;
                self.printed.push(Handed::default());
            }
            "-fprintf" => {
                self.value()?;
                self.value()?;
                self.actions = true;
                self.printed.push(Handed::default());
            }
            "-print" | "-print0" | "-ls" => {
                self.actions = true;
                self.printed.push(guard.clone());
            }
            "-exec" | "-execdir" | "-ok" | "-okdir" => {
                let end = self.args[self.at..]
                    .iter()
                    .position(|word| word == ";" || word == "+")?;
                self.at += end + 1;
                self.actions = true;
                self.execs.insert(index, guard.clone());
                // The command may print anything, such as a file's contents.
                self.printed.push(Handed::default());
            }
            "-delete" => self.actions = true,
            // The named file can be standard output.
            "-fprint" | "-fprint0" | "-fls" => {
                self.value()?;
                self.actions = true;
                self.printed.push(guard.clone());
            }
            "-prune" | "-depth" | "-d" | "-empty" | "-false" | "-true" | "-follow" | "-xdev"
            | "-x" | "-mount" | "-nouser" | "-nogroup" | "-readable" | "-writable"
            | "-executable" | "-quit" => {}
            "-xtype" | "-maxdepth" | "-mindepth" | "-mtime" | "-mmin" | "-atime" | "-amin"
            | "-ctime" | "-cmin" | "-Btime" | "-Bmin" | "-newer" | "-anewer" | "-cnewer"
            | "-mnewer" | "-size" | "-user" | "-group" | "-uid" | "-gid" | "-perm" | "-links"
            | "-inum" | "-samefile" | "-regex" | "-iregex" | "-regextype" | "-fstype"
            | "-flags" | "-lname" | "-ilname" | "-used" => {
                self.value()?;
            }
            newer if newer.len() == 8 && newer.starts_with("-newer") => {
                self.value()?;
            }
            _ => return None,
        }
        Some(Handed::default())
    }
}
