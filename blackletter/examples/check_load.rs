use blackletter_types::Character;

/// Dev tool: parse a sheet JSON and report. The path is an explicit
/// argument — nothing in this repo may reference files outside it.
fn main() {
    let Some(p) = std::env::args().nth(1) else {
        eprintln!("usage: check_load <sheet.json>");
        std::process::exit(2);
    };
    match serde_json::from_str::<Character>(&std::fs::read_to_string(&p).unwrap()) {
        Ok(c) => println!("OK {} lvl {}", c.name, c.classes.total_level()),
        Err(e) => println!("PARSE FAIL: {e}"),
    }
}
