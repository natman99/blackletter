use crate::{App, command};

pub fn tick(_: &str, app: &mut App) -> command::Result<()> {
    app.sheet.tick_conditions();

    Ok(())
}
