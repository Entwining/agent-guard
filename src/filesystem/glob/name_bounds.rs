use super::{alternatives, class_end, literal_prefix};

/// The basename patterns that bound every file some positive filter selects,
/// or `None` when there is no filter or one selects any name.
pub(crate) fn filter_bounds<'a>(filters: impl IntoIterator<Item = &'a str>) -> Option<Vec<String>> {
    let mut bounds = Vec::new();
    for filter in filters {
        bounds.extend(name_bounds(filter)?);
    }
    (!bounds.is_empty()).then_some(bounds)
}

/// The basename patterns that bound every file a name filter selects, or
/// `None` when some alternative selects any name. BSD grep and `find -path`
/// match the whole path, where a wildcard crosses `/` and a leading dot, so
/// only the literal text after the last wildcard bounds the basename; that
/// bound also holds for basename and gitignore matching.
pub(crate) fn name_bounds(filter: &str) -> Option<Vec<String>> {
    alternatives(filter, true)
        .ok()?
        .iter()
        .map(|member| {
            let chars: Vec<char> = member.chars().collect();
            let (mut index, mut tail) = (0, 0);
            while index < chars.len() {
                match chars[index] {
                    '*' | '?' => tail = index + 1,
                    '\\' => index += 1,
                    '[' => {
                        if let Some(end) = class_end(&chars, index) {
                            index = end;
                            tail = end + 1;
                        }
                    }
                    _ => {}
                }
                index += 1;
            }
            let rest: String = chars[tail..].iter().collect();
            let (separated, name) = rest
                .rsplit_once('/')
                .map_or((false, rest.as_str()), |(_, name)| (true, name));
            if literal_prefix(name).0.is_empty() {
                return None;
            }
            Some(if tail > 0 && !separated {
                format!("*{name}")
            } else {
                name.to_owned()
            })
        })
        .collect()
}
