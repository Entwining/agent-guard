use crate::record::Word;
use std::collections::BTreeMap;

/// Name patterns that cover the files an action handles, each of which
/// matches one of them; `None` is unbounded. Every filter that passed before
/// the action contributes its patterns, so fixed text that identifies a
/// protected name stays checked beside a wider filter.
type Bound = Option<Vec<String>>;

/// The name bounds a `find` expression places on what it hands on.
pub(super) struct Bounds {
    /// The files each `-exec`-family action receives, by the index of its
    /// action word.
    pub execs: BTreeMap<usize, Bound>,
    /// The names it prints to standard output.
    pub printed: Bound,
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
    let whole = parser.or(&None, 0)?;
    if parser.at != args.len() {
        return None;
    }
    // Without an action, find prints every name the expression accepts.
    let printed = if parser.actions {
        parser
            .printed
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .map(|bounds| bounds.concat())
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
    printed: Vec<Bound>,
    execs: BTreeMap<usize, Bound>,
    actions: bool,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&str> {
        self.args.get(self.at).map(|word| word.as_str())
    }

    /// A disjunction is bounded only when every branch is, and an action in
    /// one branch learns nothing from the branches that failed before it.
    fn or(&mut self, guard: &Bound, depth: usize) -> Option<Bound> {
        let mut branches = vec![self.and(guard, depth)?];
        while matches!(self.peek(), Some("-o" | "-or")) {
            self.at += 1;
            branches.push(self.and(guard, depth)?);
        }
        Some(
            branches
                .into_iter()
                .collect::<Option<Vec<_>>>()
                .map(|bounds| bounds.concat()),
        )
    }

    /// Terms run left to right, so an action is bounded only by the terms
    /// that passed before it, and each term bounds the conjunction.
    fn and(&mut self, guard: &Bound, depth: usize) -> Option<Bound> {
        let mut guard = guard.clone();
        let mut bound = None;
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
            if let Some(names) = term {
                bound
                    .get_or_insert_with(Vec::new)
                    .extend(names.iter().cloned());
                guard.get_or_insert_with(Vec::new).extend(names);
            }
        }
        (terms > 0).then_some(bound)
    }

    fn unary(&mut self, guard: &Bound, depth: usize) -> Option<Bound> {
        if depth > crate::limits::MAX_NESTING {
            return None;
        }
        match self.peek()? {
            "!" | "-not" => {
                self.at += 1;
                self.unary(guard, depth + 1)?;
                Some(None)
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

    fn primary(&mut self, guard: &Bound) -> Option<Bound> {
        let index = self.at;
        let primary = self.value()?.as_str();
        match primary {
            "-name" | "-iname" | "-path" | "-ipath" | "-wholename" | "-iwholename" => {
                let filter = self.value()?;
                // A filter the shell fills in at run time selects unknown names.
                if filter.expands || filter.runtime_unknown {
                    return Some(None);
                }
                return Some(crate::filesystem::name_bounds(filter).map(|mut names| {
                    if !names.contains(&filter.text) {
                        names.push(filter.text.clone());
                    }
                    names
                }));
            }
            // A format can print any text.
            "-printf" => {
                self.value()?;
                self.actions = true;
                self.printed.push(None);
            }
            "-fprintf" => {
                self.value()?;
                self.value()?;
                self.actions = true;
                self.printed.push(None);
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
                self.printed.push(None);
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
            "-type" | "-xtype" | "-maxdepth" | "-mindepth" | "-mtime" | "-mmin" | "-atime"
            | "-amin" | "-ctime" | "-cmin" | "-Btime" | "-Bmin" | "-newer" | "-anewer"
            | "-cnewer" | "-mnewer" | "-size" | "-user" | "-group" | "-uid" | "-gid" | "-perm"
            | "-links" | "-inum" | "-samefile" | "-regex" | "-iregex" | "-regextype"
            | "-fstype" | "-flags" | "-lname" | "-ilname" | "-used" => {
                self.value()?;
            }
            newer if newer.len() == 8 && newer.starts_with("-newer") => {
                self.value()?;
            }
            _ => return None,
        }
        Some(None)
    }
}
