use blackletter_types::Character;
fn main() {
    let p = "/home/nat/Documents/Campaign/characters/Aldric Underwood.json";
    match serde_json::from_str::<Character>(&std::fs::read_to_string(p).unwrap()) {
        Ok(c) => println!("OK {} lvl {}", c.name, c.classes.total_level()),
        Err(e) => println!("PARSE FAIL: {e}"),
    }
}
