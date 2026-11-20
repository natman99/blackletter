Blackletter
===

Blackletter is an implementation of Dungeons and Dragons 5e mechanics in Rust :rocket:. 

## Project Layout

### Blackletter Types
The home of the core logic.
Should be usable for single classed builds.

Statically typed using Rust's type system. Nearly all mechanics covered and customizable.

- Types implement serde, JsonSchema, and Rhai bindings under the respective `serde`, `schema` and `rhai` features.

### Blackletter Cli
Tui for viewing character sheets.
Abandoned in favor of [Blackletter](#Blackletter).
- To be repurposed into a cli for AI agents to write Blackletter scripts and types.

### Blackletter Scripting
Integration with (rhai)[https://rhai.rs/] for building characters in a scripting language.

- Create a character from rhai script
- Script against a sheet in real time to calculate feature interactions and rolls.

- [ ] Add rhai docs
  - For now, examples can be found in src/tests.rs

```rust
name = "Bob";
classes[0].set_level(2);
classes[0].set_class("warlock");
classes[0].subclass = "cool subclass";
ability_scores.str = 14;
health.max = 42;
let i = spell("Cool Spell");
i.damage();
i.component("V");
i.desc("Cool spell goes boom");
i.cast_time("1 action");
i.level = 2;
i.ritual = true;
i.ranged();
i.damage("2d4", "fire");
spells.add(i);
```

### Blackletter
The GUI culmination of all the projects.

- [ x ] Hot reloading
  - Hot reloading preserves consumable resources and health across changes.
- [ x ] Basic character information 
- [ ] Inventory (partial)
- [ ] Spells + Meters
- [ x ] Integration with [blackletter_scripting](#blackletter-scripting)

## Platform support
- Should work on most Linux distributions
  - Install rust toolchain
  - Follow errors for missing system packages
- Windows 
  - Install rust toolchain
- Mac Os
  - Probably works, but untested

## Building
1. Clone this repo.
2. This.
This project uses info from the D&D Srd. Clone it before building.
```bash
git clone https://github.com/5e-bits/5e-database
cargo build
```
Or this.
To install Blackletter ui to path (what you probably want)
```bash
git clone https://github.com/5e-bits/5e-database
cargo install --path blackletter --locked
```
