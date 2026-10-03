mod support;
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

struct OwnedWriter {
    root: PathBuf,
    commits: usize,
    serial: usize,
}

impl OwnedWriter {
    fn new(root: PathBuf) -> Self {
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("out.txt"), "KEEP\n").unwrap();
        Self {
            root,
            commits: 0,
            serial: 0,
        }
    }
    fn write(&mut self, expected: &str, proposal: &str, partial: bool) -> Value {
        self.serial += 1;
        let proposal_path = format!("proposal-{}", self.serial);
        let staging = format!("staging-{}", self.serial);
        durable(&self.root.join(&proposal_path), proposal);
        let current = fs::read_to_string(self.root.join("out.txt")).unwrap();
        let mut io_error = None;
        let state = if current != expected {
            "Conflict"
        } else {
            let mut sink = StagedSink {
                file: fs::File::create(self.root.join(&staging)).unwrap(),
                remaining: partial.then_some(5),
            };
            match sink
                .write_all(proposal.as_bytes())
                .and_then(|()| sink.file.sync_all())
            {
                Err(error) => {
                    io_error = Some(format!("{:?}", error.kind()));
                    "PartialStagedFailure"
                }
                Ok(()) => {
                    fs::rename(self.root.join(&staging), self.root.join("out.txt")).unwrap();
                    self.commits += 1;
                    "Committed"
                }
            }
        };
        let receipt = json!({"state":state,"target":"out.txt","proposal":proposal_path,"partial_artifact":if io_error.is_some() {Some(staging)} else {None},"commits":self.commits,"io_error":io_error});
        durable(&self.root.join("state.json"), &receipt.to_string());
        receipt
    }
    fn state(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.root.join("state.json")).unwrap()).unwrap()
    }
    fn owner_edit(&mut self, bytes: &str) {
        durable(&self.root.join("out.txt"), bytes);
    }
}

struct StagedSink {
    file: fs::File,
    remaining: Option<usize>,
}

impl Write for StagedSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = match self.remaining {
            Some(0) => {
                return Err(io::Error::other(
                    "synthetic write fault after accepted prefix",
                ));
            }
            Some(remaining) => bytes.len().min(remaining),
            None => bytes.len(),
        };
        let written = self.file.write(&bytes[..count])?;
        if let Some(remaining) = &mut self.remaining {
            *remaining -= written;
        }
        Ok(written)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn durable(path: &Path, bytes: &str) {
    let mut file = fs::File::create(path).unwrap();
    file.write_all(bytes.as_bytes()).unwrap();
    file.sync_all().unwrap();
}

#[test]
fn owned_writer_states() {
    for row in support::rows()
        .iter()
        .filter(|r| r["consumer"] == "owned-writer")
    {
        let fixture = support::Fixture::new();
        let mut writer = OwnedWriter::new(fixture.root.join("copy"));
        if let Some(edit) = row["input"]["intervening_owner_write"].as_str() {
            writer.owner_edit(edit);
        }
        let receipt = writer.write(
            "KEEP\n",
            row["input"]["proposal"].as_str().unwrap(),
            row["input"].get("fault").is_some(),
        );
        assert_eq!(receipt["state"], row["writer_state"]);
        assert_eq!(writer.state(), receipt);
        assert_eq!(
            fs::read_to_string(writer.root.join(receipt["proposal"].as_str().unwrap())).unwrap(),
            "KEEP\nnew\n"
        );
        match receipt["state"].as_str().unwrap() {
            "Committed" => {
                assert_eq!(
                    fs::read_to_string(writer.root.join("out.txt")).unwrap(),
                    "KEEP\nnew\n"
                );
                assert_eq!(receipt["io_error"], Value::Null);
            }
            "Conflict" => {
                assert_eq!(writer.commits, 0);
                let current = fs::read_to_string(writer.root.join("out.txt")).unwrap();
                assert_eq!(current, "KEEP\nuser\n");
                assert_eq!(writer.state()["state"], "Conflict");
                assert_eq!(
                    writer.write(&current, "KEEP\nuser\nnew\n", false)["state"],
                    "Committed"
                );
                assert_eq!(
                    fs::read_to_string(writer.root.join("out.txt")).unwrap(),
                    "KEEP\nuser\nnew\n"
                );
            }
            "PartialStagedFailure" => {
                assert_eq!(receipt["io_error"], "Other");
                assert_eq!(writer.commits, 0);
                let current = fs::read_to_string(writer.root.join("out.txt")).unwrap();
                assert_eq!(current, "KEEP\n");
                assert_eq!(
                    fs::read_to_string(writer.root.join("staging-1")).unwrap(),
                    "KEEP\n"
                );
                assert_eq!(writer.state()["state"], "PartialStagedFailure");
                assert_eq!(
                    writer.write(&current, "KEEP\nnew\n", false)["state"],
                    "Committed"
                );
                assert_eq!(
                    fs::read_to_string(writer.root.join("out.txt")).unwrap(),
                    "KEEP\nnew\n"
                );
                assert_eq!(
                    fs::read_to_string(writer.root.join("staging-1")).unwrap(),
                    "KEEP\n"
                );
            }
            _ => panic!("unexpected durable writer state"),
        }
        println!(
            "{}",
            json!({"id":row["id"],"initial_receipt":receipt,"final_receipt":writer.state(),"final_bytes":fs::read_to_string(writer.root.join("out.txt")).unwrap(),"scope":"single owned synthetic writer; injected Write error, no crash/concurrency claim; excluded from protection denominators"})
        );
    }
}
