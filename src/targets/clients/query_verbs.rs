use super::*;

// Verbs that act on disks, backups or directory records without reading file
// contents, from the bundled diskutil(8), tmutil(8) and dscl(1) manuals.
// Other verbs stay unknown programs.
const DISKUTIL: &[&str] = &["list", "info", "information", "activity", "listFilesystems"];
const DISKUTIL_GROUPS: &[(&[&str], &[&str])] = &[
    (
        &["apfs", "ap"],
        &[
            "list",
            "listCryptoUsers",
            "listUsers",
            "listCryptoKeys",
            "listKeys",
            "listSnapshots",
            "listVolumeSnapshots",
            "listVolumeGroups",
            "listGroups",
        ],
    ),
    (&["coreStorage", "cs", "appleRAID", "ar"], &["list"]),
];
const TMUTIL: &[&str] = &[
    "destinationinfo",
    "isexcluded",
    "addexclusion",
    "removeexclusion",
    "latestbackup",
    "listbackups",
    "machinedirectory",
    "listlocalsnapshots",
    "listlocalsnapshotdates",
    "version",
];
const DSCL: &[&str] = &[
    "-read", "-readall", "-readpl", "-readpli", "-list", "-search",
];
const DSCL_VALUES: &[&str] = &["-u", "-P", "-f"];

fn operands(words: &[Word]) -> impl Iterator<Item = &str> {
    words
        .iter()
        .map(Word::as_str)
        .filter(|word| !word.starts_with('-'))
}

// diskutil and tmutil match verbs without regard to case; dscl does not.
fn listed(verbs: &[&str], word: &str) -> bool {
    verbs.iter().any(|verb| verb.eq_ignore_ascii_case(word))
}

fn spec_with(operand: Effect, options: &'static [(&'static str, Effect)]) -> Spec {
    Spec {
        operand,
        walk: Walk::None,
        options,
        remote: false,
        destination: None,
    }
}

pub(super) fn spec(program: &str, words: &[Word]) -> Option<Spec> {
    let mut verbs = operands(words);
    let verb = verbs.next().unwrap_or("");
    match program {
        "diskutil" => (listed(DISKUTIL, verb)
            || DISKUTIL_GROUPS.iter().any(|(group, verbs_in_group)| {
                listed(group, verb) && verbs.next().is_some_and(|sub| listed(verbs_in_group, sub))
            }))
        .then(|| spec_with(Effect::Meta, &[])),
        "tmutil" => listed(TMUTIL, verb).then(|| spec_with(Effect::Meta, &[])),
        _ => {
            // dscl [options] datasource command; directory service paths name
            // records, not files.
            let mut index = 0;
            while words.get(index).is_some_and(|word| word.starts_with('-')) {
                index += if DSCL_VALUES.contains(&words[index].as_str()) {
                    2
                } else {
                    1
                };
            }
            words
                .get(index + 1)
                .is_some_and(|command| DSCL.contains(&command.as_str()))
                .then(|| spec_with(Effect::Name, &[("-f", Effect::Use)]))
        }
    }
}
