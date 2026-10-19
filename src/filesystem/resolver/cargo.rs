use super::super::{CARGO_STORE, CheckError, Probe, Resolution, fold, glob, resolve};
use super::Resolver;

impl Resolver<'_> {
    /// Cargo keeps credentials in its home, which is wherever `~/.cargo`
    /// resolves; the lexical rule only knows the spelling under HOME.
    pub(super) fn resolve_cargo_home(
        &mut self,
        resolved_home: &str,
        probe: &mut dyn Probe,
    ) -> Result<(), CheckError> {
        if self.cargo_home.is_none() {
            let resolution = resolve(
                &format!("{}/{CARGO_STORE}", self.home),
                self.home,
                self.home,
                Some(resolved_home),
                self.table,
                probe,
                &mut self.lexical,
            )?;
            self.cargo_home = Some(match resolution {
                Resolution::Public(path) => Some(fold(&path)),
                _ => None,
            });
        }
        Ok(())
    }

    /// Whether a read takes the resolved Cargo home as a whole: the directory
    /// itself, or a pattern whose last component only matches names in it.
    pub(super) fn cargo_home_read(&self, path: &str, patterned: bool) -> bool {
        let Some(Some(cargo_home)) = &self.cargo_home else {
            return false;
        };
        if !patterned {
            return fold(path) == *cargo_home;
        }
        match path.rsplit_once('/') {
            Some((parent, base)) if base.trim_matches(['*', '?']).is_empty() => {
                matches!(glob::literal_prefix(parent), (parent, "") if fold(&parent) == *cargo_home)
            }
            _ => false,
        }
    }
}
