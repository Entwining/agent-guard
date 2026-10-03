use std::{env, error::Error, io::Cursor, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("expected bytes|nesting and a public sample size".into());
    }
    let size: usize = args[1].parse()?;
    if size > 1_048_576 {
        return Err("sample exceeds the public sweep range".into());
    }
    let command = match args[0].as_str() {
        "bytes" => {
            let base = "cat './public-fixture.txt'";
            format!("{base}{}", " ".repeat(size.saturating_sub(base.len())))
        }
        "nesting" => {
            if size > 512 {
                return Err("nesting sample exceeds the public sweep range".into());
            }
            format!(
                "{}cat './public-fixture.txt'{}",
                "printf '%s' $(".repeat(size),
                ")".repeat(size)
            )
        }
        _ => return Err("unknown public sweep".into()),
    };
    let event = serde_json::json!({"tool_name":"Bash","tool_input":{"command":command}});
    let bytes = serde_json::to_vec(&event)?;
    let started = Instant::now();
    let _: serde_json::Value = serde_json::from_slice(&bytes)?;
    let json_us = started.elapsed().as_micros();
    let started = Instant::now();
    let brush_ok = brush_parser::Parser::builder()
        .build(Cursor::new(command.as_bytes()))
        .parse_program()
        .is_ok();
    let brush_us = started.elapsed().as_micros();
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_bash::LANGUAGE.into())?;
    let started = Instant::now();
    let tree_ok = parser
        .parse(&command, None)
        .is_some_and(|tree| !tree.root_node().has_error());
    let tree_us = started.elapsed().as_micros();
    println!(
        "{}",
        serde_json::json!({"kind":args[0],"size":size,"command_bytes":command.len(),"event_bytes":bytes.len(),"json_us":json_us,"brush_ok":brush_ok,"brush_us":brush_us,"tree_ok":tree_ok,"tree_us":tree_us})
    );
    Ok(())
}
