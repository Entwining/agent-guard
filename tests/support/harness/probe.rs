use super::*;

pub struct RecordingProbe {
    pub calls: Vec<String>,
    pub stat_calls: Vec<String>,
    pub links: BTreeMap<String, String>,
    pub fault: Option<String>,
    pub home: String,
    pub patterned: bool,
}

impl RecordingProbe {
    pub fn new(fixture: &Fixture, word: &agent_guard_rust::record::Word) -> Self {
        Self {
            calls: Vec::new(),
            stat_calls: Vec::new(),
            links: BTreeMap::new(),
            fault: None,
            home: fixture.home.clone(),
            patterned: word.globs,
        }
    }
    pub fn literal_for_quoted_paths(fixture: &Fixture) -> Self {
        Self::new(
            fixture,
            &agent_guard_rust::record::Word::literal(String::new()),
        )
    }
    fn protected(&self, spelling: &str) -> bool {
        if self.patterned {
            filesystem::lexical(spelling, &self.home).is_some()
        } else {
            filesystem::lexical_literal(spelling, &self.home).is_some()
        }
    }
}
impl Probe for RecordingProbe {
    fn stat(&mut self, path: &Path) -> io::Result<Option<filesystem::Metadata>> {
        let spelling = path.to_str().unwrap().to_owned();
        self.stat_calls.push(spelling.clone());
        assert!(
            !self.protected(&spelling),
            "protected spelling reached stat: {spelling}"
        );
        if self.fault.as_ref() == Some(&spelling) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        filesystem::DiskProbe.stat(path)
    }
    fn read_link(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        let spelling = path.to_str().unwrap().to_owned();
        self.calls.push(spelling.clone());
        assert!(
            !self.protected(&spelling),
            "protected spelling reached probe: {spelling}"
        );
        if self.fault.as_ref() == Some(&spelling) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        if let Some(target) = self.links.get(&spelling) {
            return Ok(Some(PathBuf::from(target)));
        }
        filesystem::DiskProbe.read_link(path)
    }
}
