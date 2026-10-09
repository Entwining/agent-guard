use super::*;

impl Evaluator<'_, '_> {
    pub(super) fn read(&self, args: &[crate::record::Word], scope: &mut Scope) {
        let mut array_name = None;
        let mut index = 0;
        while let Some(word) = args.get(index) {
            if word == "--" {
                index += 1;
                break;
            }
            let Some(flags) = word.strip_prefix('-').filter(|flags| !flags.is_empty()) else {
                break;
            };
            for (at, option) in flags.char_indices() {
                if matches!(option, 'd' | 'n' | 't' | 'u' | 'a' | 'p') {
                    let attached = &flags[at + option.len_utf8()..];
                    let value = if attached.is_empty() {
                        index += 1;
                        args.get(index).map(|word| word.text.as_str())
                    } else {
                        Some(attached)
                    };
                    if option == 'a' {
                        array_name = value.filter(|value| identifier(value)).map(str::to_owned);
                    }
                    break;
                }
            }
            index += 1;
        }
        let mut names = args[index.min(args.len())..]
            .iter()
            .filter(|word| identifier(&word.text))
            .map(|word| word.text.clone())
            .collect::<Vec<_>>();
        if names.is_empty() && array_name.is_none() && index >= args.len() {
            names.push("REPLY".into());
        }
        for name in names {
            scope.assign(name, vec![BindingValue::RuntimeUnknown(None)]);
        }
        if let Some(name) = array_name {
            self.read_unknown_array(&name, scope);
        }
    }
}
