# Blackletter GPUI App — Character Sheets + Dungeon Tracker

> **For Hermes:** use the subagent-driven-development skill to implement this plan phase-by-phase. Each phase ends green at the gates before the next starts.

**Progress (2026-11-21):** Phase 0 DONE (iced deleted 4c26c64; blackletter_dungeon ported 8233bec; gpui scaffold live-verified 4b32f5f). Phase 1 DONE (app shell c35cd42; session+autosave 140404c — 0% idle CPU; campaign folders + rfd pickers 22e9746; windows-gnu green). Phase 2 DONE (lock/open door upgrade ec155d3; GPUI dungeon view 6817ed7 — canvas map, 41 keyboard actions, undo/redo, pan/zoom, help overlay, jump-centers-camera, same-floor path overlay, DC labels above door bars, dungeon switcher ctrl-,/ctrl-.). Phase 2.5 DONE (f94fbcf: shale Field widget ported to field.rs; enter rename / shift-O notes via Field key context; room info panel). 21 app + 47 dungeon tests.

**Goal:** Replace the jank iced GUI with a single GPUI desktop app (`blackletter`) that (1) creates and manages D&D 5e character sheets as completely as possible, and (2) folds in a rewrite of `~/Documents/Coding/Rust/dungeon-tracker` as a second mode — auto-saving, working on Linux and Windows.

**Architecture:** Headless-first, chonk-style. All logic stays in framework-free crates (`blackletter_types` already is; the dungeon model moves into a new `blackletter_dungeon`); the `blackletter` crate becomes a thin GPUI shell with two modes (Sheet, Dungeon) sharing one app shell, one session/autosave engine, and one SRD browser. The dungeon core is a *port* of dungeon-tracker's framework-free modules (model/, dungeon/, save/ — 46 tests come along for free); only its gui/ is rewritten.

**Tech stack:** Zed `gpui` + `gpui_platform` from git tag **v1.25.3** (exact shale pin, incl. `features = ["wayland", "x11", "font-kit"]`), `serde`/`serde_json`, `glam` (dungeon grid), `directories` (session/config dir), vendored `5e-database/` JSON as the SRD corpus. No new GUI crates — gpui built-ins first (lean-dependency rule).

**Rulings already made (2026-11-20):**
1. GPUI = Zed gpui v1.25.3 from git (like shale), not the gpui-pre crates.io fork.
2. Sheets = full creation wizard + management (level-up, SRD pickers for race/class/feat/spell/item).
3. Dungeon tracker = same app shell, separate modes, light link (party position, per-char HP).
4. The iced GUI is deleted now; GPUI replaces it.
5. The initiative tracker stays in Shale — NOT in scope here.
6. Save layout = **campaign folders**: one folder per campaign containing `characters/` + `dungeons/`.
7. Party panel shows **active conditions + down/stable/dead badges** per member.
8. Dungeon rewrite folds in **lock DC + door open/closed**; room types and per-room enemy lists stay deferred.
9. Rhai house-rules console = **Phase 6+ stretch**, after the core app works.
10. Printable/PDF export = **deferred indefinitely**.
11. **Prepared-caster support is in** (cleric/wizard/paladin) — requires engine work (Task 3.0).
12. Edition scope: 2014 is the default and the only corpus wired end-to-end; the vendored 2024 tree loads behind an edition tag but no 2024-only flows ship.
13. Window model: **single window, mode tabs** (multi-window is YAGNI; gpui multi-window pitfalls from the shale skill stay unused).
14. Keybinding scheme: shale-style context-scoped chords; dungeon keeps vim `hjkl` exactly.
15. **Keyboard navigability is preferred whenever possible** (every action reachable by key; mouse is convenience, not requirement).
16. GUI surface may be **richer than shale's minimalism** (panels, buttons, badges welcome) — but performance rules stay binding (demand-driven frames, no UI-thread work).

---

## Lessons imported from shale & chonk (binding for this repo)

- **Core/app split (chonk):** `blackletter_dungeon` and `blackletter_types` never depend on gpui. CLI bins and headless tests come free from them. The GPUI crate holds only views, input, and rendering.
- **GPUI API truth (shale/gpui skill):** grep the pinned checkout `~/.cargo/git/checkouts/zed-*/<rev>/crates/gpui*/` — never docs.rs, never memory. Cite `file:line` in design notes.
- **No TextInput in gpui (shale):** Zed's `TextInput` lives in Zed's editor crates. **Port shale's `Field` widget near-verbatim**: `~/projects/shale/crates/app/src/field.rs` + `keys.rs::field_keys!` + `field_key_actions!` (one Field behind prompt/picker/filter — blackletter needs the same three surfaces). Key rules that must survive the port: selection is one `anchor` + caret and **every edit takes the selection first**; gpui passes `range: None` for printable keys so the field's own selection is the fallback; one key table, never per-surface `on_key`; clipboard verbs are actions so ctrl-c works while a picker holds the keyboard. The only genuinely new surface is the multi-line notes field (spike task at the top of Phase 3).
- **Layout (gpui skill):** Zed-gpui flex children need explicit grow+basis; every fill slot spelled out. Canvas elements need `.size_full()`. Never bake square-canvas assumptions into the dungeon map view — per-axis `fit = min(ew/docw, eh/doch)`, store view geometry each render, hit-testing mirrors paint math exactly.
- **Custom drawing (gpui skill):** dungeon map = `gpui::canvas` + `paint_quad` (rooms, rounded) + ONE batched `PathBuilder` per color for door strokes (cost scales with path count, not vertices). No radial gradients exist.
- **Repaint discipline (shale):** frames are demand-driven; no `cx.notify()` from bare timers; autosave debounce must not dirty frames.
- **Dirty-state notify (gpui skill):** `Entity::update` from outside the render loop must `cx.notify()` inside the closure or nothing repaints.
- **Styled runs are byte lengths (gpui skill):** any SRD-picker row with bold/highlight runs must keep char-vs-byte ranges apart or gpui panics `invalid text run`.
- **Keybindings (shale/gpui skill):** every new `key_context` rebinds the escape-hatch keys; a keybinding without an `on_action` ancestor is dead; `+`-style shifted symbols arrive shift-folded on Wayland — bind the shifted char plain.
- **Windows (shale):** the wine test gate is a real gate: `cargo test --workspace --target x86_64-pc-windows-gnu` (runner wired in `~/.cargo/config.toml`), run ONCE at the end of each phase, in two stages (build `--tests` first, then per-crate) on a cold tree. Windows-only warnings come from `cfg`-gated code — grep `^warning`.
- **Gates (shale):** `cargo fmt --all --check` → 0, `cargo clippy --workspace --all-targets` → 0 warnings, `cargo test --workspace` green — measured, not remembered; one stage per terminal call. Commit with pathspecs.
- **Status bar (chonk):** every open/save/load/import path reports success AND failure in an in-app bottom status bar, never stderr.
- **File dialogs (chonk/gpui skill):** `rfd` with `default-features = false, features = ["xdg-portal"]`; run the picker on `cx.background_executor()`, deliver via weak-entity update.
- **Undo (chonk):** every mutating feature snapshots before mutating; sheet and dungeon both get snapshot undo/redo from day one (cap ~64, tagged coalescing for held-key bursts).

---

## Phase 0 — Sweep: delete iced, port the dungeon core (no GUI yet)

### Task 0.1 — Delete the iced GUI
- Delete `blackletter/src/` (lib.rs, main.rs, views/) and strip `iced`, `notify-debouncer-full`, `rfd`, `tokio` deps from `blackletter/Cargo.toml`. Keep the crate name `blackletter` (bin + lib).
- Keep the *ideas* worth keeping from the old code (read them first, don't port them): sheet-path tracking, `.bak` backup on save, `UiState` saved next to the sheet (→ becomes the session file), Rhai `Interface` availability (→ later phase).
- Gate: `cargo check --workspace` green. Commit `refactor: remove iced GUI (replaced by GPUI app)`.

### Task 0.2 — New crate `blackletter_dungeon` (headless)
- Create `blackletter_dungeon/` (workspace member): port **verbatim** from `~/Documents/Coding/Rust/dungeon-tracker/src/`:
  - `model/` (mod.rs, direction.rs, states.rs) → `blackletter_dungeon/src/model/`
  - `dungeon/` (mod.rs, invariants.rs, path.rs) → `src/dungeon/`
  - `save/` (mod.rs) → `src/save/`
  - All 46 `#[test]`s come with them. Deps: `glam` (serde feature), `serde`, `serde_json`, `thiserror`. NO eframe/egui/resvg.
- Save format: keep the existing JSON shape byte-compatible so the user's existing saves load unchanged (check `~/Documents/Coding/Rust/dungeon-tracker` for save files; `save/mod.rs` name→path rules move over, with the app-data dir resolved via `directories` instead of the old hard-coded path).
- Gate: `cargo test -p blackletter_dungeon` — expect ≥46 passed. Commit `feat: port dungeon core to blackletter_dungeon`.

### Task 0.3 — GPUI scaffold
- `blackletter/Cargo.toml`: add the exact shale pins:
  ```toml
  gpui = { git = "https://github.com/zed-industries/zed", tag = "v1.25.3" }
  gpui_platform = { git = "https://github.com/zed-industries/zed", tag = "v1.25.3", features = ["wayland", "x11", "font-kit"] }
  ```
  plus `directories`, `rfd = { default-features = false, features = ["xdg-portal"] }`, `anyhow`, `serde*`.
- `blackletter/src/main.rs`: `App::new()` → `Application::new().run(|cx: &mut App| …)` opening one window with a root `RootView` entity. Verify: `cargo run -p blackletter` shows a titled window on niri (grim + vision per the gpui skill). Commit `feat: gpui app scaffold`.

---

## Phase 1 — App shell: modes, session, autosave

The shell is the contract everything later plugs into.

### Task 1.1 — RootView + mode switching
- `blackletter/src/root.rs`: `RootView { mode: Mode, status: Option<String>, … }`, `enum Mode { Sheet, Dungeon }`. Layout: top mode bar (tabs), center = mode view (`flex_1` spelled as `flex_grow(1.) + flex_basis(relative(0.))`), bottom status bar (chonk rule: all success/error lines land here).
- Key context `BLACKLETTER_CTX`: `ctrl-1`/`ctrl-2` switch modes, `ctrl-,` later. Register `on_action` handlers on the root chain (a binding without a handler is dead — gpui skill).
- Headless test (`gpui::test`, `TestAppContext`): press `ctrl-2`, assert the dungeon view entity is the center child.

### Task 1.2 — Session file + autosave engine
- **Save layout (decided):** a *campaign folder* is the unit the app opens: `<root>/<Campaign Name>/characters/*.json` + `<root>/<Campaign Name>/dungeons/*.json`. Root is user-chosen (recommend inside `~/Nextcloud/Documents/Dnd/` so campaign JSON syncs next to the ZenNotes vault — but the app never assumes Nextcloud). "New Campaign" = rfd directory picker + create skeleton + optional `campaign.json` meta (name, created, edition=2014).
- **File format:** pretty JSON, `{"format": 1, ...}` version field on every document; all future fields `#[serde(default)]` (chonk rule: old files keep loading). Sheet file = `blackletter_types::Character` + `format`; dungeon file = `blackletter_dungeon::Dungeon` + `format`.
- **Legacy import:** one-shot "Import old dungeon saves" action copies saves from `~/.local/share/dungeon-tracker/saves/` (the old `directories::ProjectDirs("", "", "dungeon-tracker")` dir) into the chosen campaign's `dungeons/`, leaving originals in place.
- `blackletter/src/session.rs`: session JSON in the platform config dir (`directories` → `~/.config/blackletter/session.json`, `%APPDATA%` on Windows): open campaign path, open sheet/dungeon paths, active mode, window size. Restore on launch (missing files → skip silently, status line says which).
- Autosave: each document entity owns a `save_state` (dirty flag + last-saved Instant). A single foreground task wakes ONLY when something is dirty (shale idle rule: no bare-timer repaints; the task awaits a oneshot woken by the first dirty transition, then a 2 s debounce). Write = serde pretty JSON → temp file → `rename` (atomic on both OSes) → keep one `.bak` of the previous version (idea inherited from the iced app).
- Title bar shows `●` when dirty, clears on save (chonk/shale convention).
- Tests: mark dirty → advance fake clock → assert file on disk + `.bak` rotation; session round-trip test.

### Task 1.3 — File open/save plumbing
- `blackletter/src/files.rs`: `open_sheet()`, `open_dungeon()` via rfd on `background_executor` (chonk pattern: dialog never on UI thread; result via `WeakEntity::update`). Also a recent-files list from the session file. Every path reports to the status bar.

Phase gate: fmt/clippy/test green; app launches, switches modes, restores last session, autosaves a dummy document. Windows gate: `cargo build --workspace --target x86_64-pc-windows-gnu --tests` compiles clean (no wine run needed yet — no window tests on Windows until Phase 2).

---

## Phase 2 — Dungeon mode (the tracker rewrite)

Port the egui GUI's *behavior*, not its code. Reference for behavior: `dungeon-tracker/src/gui/mod.rs` (971 LOC) + its PLAN.md §4 invariants.

### Task 2.1 — Map canvas element
- `blackletter/src/dungeon_view/mod.rs`: `DungeonView` entity holding `Entity<DungeonDoc>` (doc = `blackletter_dungeon::Dungeon` + undo stack + dirty flag).
- `blackletter/src/dungeon_view/canvas.rs`: `gpui::canvas` element. Per-floor view: rooms = rounded `paint_quad` (color-coded: enemies red border, trap yellow, secret dashed, searched dimmed, exit green, current floor's party room highlighted); doors = one batched `PathBuilder` per color (normal white, unsafe yellow, safe-detour blue); party marker = circle quad; path overlay from `path.rs` BFS.
- Geometry contract (chonk/gpui skill, non-negotiable): per-axis fit, uniform display scale, per-axis centering + pan; store `view_size`/`view_disp`/origin each `render()`; paint closure captures them; hit-testing recomputes the SAME math. Zoom = wheel + `=`/`-` (constant 1.25× per press so pairs cancel), pan = drag empty space, `0` resets.
- Tests: headless — open a fixture dungeon JSON, assert `painted_quads()` count; click a room center → selection changes.

### Task 2.2 — Navigation + CRUD
- Vim `hjkl` 8-way movement between rooms on the current floor (the tracker's signature feature — keep it exact): move = select neighbor in that direction; `ctrl+hjkl` = create room+door in that direction (growth by discovery, parent set at creation).
- `n` new room from selected (opens direction picker), `d` delete (rejected if it would orphan rooms — `validate()`), `v` vertical door (floor picker), `f`/`F` floor down/up, `Enter` rename, `o` notes.
- Room/door state toggles: `t` trap cycle, `e` enemy cycle, `l` loot cycle, `s` secret, `x` searched, `X` exit flag, `L` locked (door). Show current flags in a side panel for the selected room (not just key letters — discoverability).
- `p` path-to-entrance, `P` path from selected→party (yellow unsafe + blue detour per `path.rs`).
- Undo/redo `ctrl-z`/`ctrl-y`: snapshot tagged before each mutation (chonk rule).
- Tests: port the interaction rules as doc-level tests (most logic already lives in `blackletter_dungeon`); add view-level tests for hjkl selection and create-in-direction.

### Task 2.3 — Door upgrades folded in (from EXTRA.md, ruled in)
- `Door.locked: bool` → `Door.lock: Option<Lock>` where `Lock { dc: i64 }`. The SRD has exactly one lock DC — **DC 15 Dex (thieves' tools)**, `Wiki/04 equipment.md:316`; there is no SRD easy/medium/hard table (that's DMG). Ship: `L` cycles none→SRD-default(15)→hard(25)→none, plus free-text DC edit in the side panel, with a code comment citing the Wiki line so nobody "fixes" 15 into a made-up table.
- `Door.open: bool` (serde default false = closed): `c` toggles; closed doors draw a solid bar across the gap, open doors a gap. (No "broken" state — that reads as open + unlocked.)
- Both fields `#[serde(default)]` so pre-existing dungeon-tracker saves load untouched; `locked: true` in old saves must migrate to `lock: Some(15)` — write a serde upgrade shim (deserialize old `locked` bool) with a test, don't silently drop it.
- Tests: old-format save round-trip (`locked: true` → `lock: Some(dc 15)`, `open: false`); cycle key lands correct state.

### Task 2.4 — Party link (light)
- `party_position` picker: `g` sets party to selected room.
- Party panel (sidebar): lists open character sheets with name + current/max HP + **active conditions + down/stable/dead badges** (decided): badge derived from `health`/`death_saves`/`is_dead()` — red "DOWN" when at 0 HP with saves active, amber "STABLE", grey "DEAD". Conditions render as small chips with remaining duration.
- Phase gate: full dungeon workflow operable by keyboard on a real session (user eyeball + grim screenshots); wine build gate.

---

## Phase 3 — Sheet mode: management of existing sheets

Read/edit every `Character` field through the engine's derived getters — the UI never recomputes modifiers itself (`modifiers()` is the single source; gpui views just render `armor_class_total()`, `speed_total()`, etc.).

### Task 3.0 — Engine gaps first (in `blackletter_types`, TDD, before any UI)
**DONE (a98823d, 5de9731, 1938ed6).** Prepared casters (`Spell.prepared`, `CastingMechanic` + `casting_mechanic()`, `needs_spell_prep` flipped by `long_rest()`, `toggle_prepared`/`finish_spell_prep`); concentration (`Character.concentrating`, `start_concentration` breaks the old, `concentration_save` vs DC max(10, dmg/2)); typed `Background` grant + `Character.background` absorbed in `modifiers()` + SRD conversion tests. All rhai scope vars wired (`needs_spell_prep`, `concentrating`, `background` — `extract_opt!` for Options).
Three features the rulings need that the engine doesn't have yet. Each = its own commit with tests, SRD text cited from the vault:
1. **Prepared-caster support** (ruled in): `sheet::Spell` gains `prepared: bool` (`#[serde(default)]`); `Character` gains a derived `casting_mechanic()` per class (prepared: cleric/paladin/ranger; known/spontaneous: bard/sorcerer/warlock; wizard = prepared + spellbook distinction). Long rest seam: `long_rest()` marks the prepared list restorable, UI prompts "prepare spells" after a long rest (2014: wizard 1 h prayer after long rest, cleric dawn prayer — cite `Wiki/08 spellcasting.md` lines when implementing; keep the mechanic a UI prompt + bool flip, no timer modeling).
2. **Concentration tracking**: `Character.concentrating: Option<Concentration { spell_name, save_dc }>`. SRD rule (cite `Wiki/08 spellcasting.md` concentration section): casting another concentration spell breaks the old; damage forces a CON save DC 10 or half damage (min 10). Engine: `start_concentration(name)`, `concentration_save(damage) -> Option<RollOutcome>`; UI: a concentration chip on the summary card with the save fired automatically by the damage buttons.
3. **Background type**: new `sheet/background.rs` — typed `Background { name, skill_proficiencies, languages, feat: Option<Feat>, equipment, feature: Option<Feature> }` implementing `Grant` (joins the grant architecture, lossless apply/remove like Race). Parse `5e-SRD-Backgrounds.json` → conversion in `blackletter_types` with tests. Needed by the wizard (Task 5.1).
- Gate: `cargo test -p blackletter_types` green; rhai scope vars for the new seams (follow the `extract_opt!` pitfall for `concentrating: Option`).

### Task 3.1 — Sheet document entity + layout
**DONE (6d89c36).** `sheet_view/{mod.rs,doc.rs}`: SheetDoc (snapshot undo cap 64, try_edit), nav/center/summary layout, Basic section (Field rename, class +/- with PB+slot sync), Abilities display, root switch_sheet on ctrl-,/. Live-verified.
- `blackletter/src/sheet_view/mod.rs`: `SheetDoc { character: Character, undo: … }`. Layout: left = section nav (Basic / Abilities / Skills & Saves / Combat / Spells / Inventory / Features & Feats / Conditions / Notes), center = section form, right = always-visible summary card (AC, HP bar, speed, PB, passive perception, death saves, exhaustion, active conditions).
- Basic section: name, race display, class levels editor (`Classes` supports multiclass — level +/- per class, `sync_proficiency_bonus()` + `sync_spell_slots()` after every level change).

### Task 3.2 — Interactive sections
**DONE (slice 1, `b469413`):** skill/save proficiency toggles (space), damage field via d with damage-type cycle (t) routed through `apply_resistance` + concentration save, death-save roll (r), short/long rest (s/shift-S), section-aware +/- (level / ability score / HP bonus, gold, exhaustion), `[`/`]` section nav (tab is unusable: gpui's default focus traversal fires even with a matching context binding), FieldElement rendered at view root so fields work in every section. Live-verified with ydotool-driven keystrokes + niri window screenshot.
**Remaining slices:** slot pips + hit dice, spells prepared-toggles per `casting_mechanic()`, death-save pip UI, conditions, inventory, features/feats/traits, notes.
- Abilities: 6 scores + derived mods; skills: proficiency toggle list (effective bonus shown, override-until-sync semantics per engine); saves: proficiency toggles.
- Combat: HP edit + temp HP, damage/heal quick buttons (routes through `apply_resistance` with damage-type picker — engine already handles resist/vuln/immunity), AC flat-override toggle (`ArmorClass.flat`), speed, gold.
- Meters/spell slots: slot pips per level (`spend_spell_slot`), pact slots short-rest styled differently; hit-die buttons (`spend_hit_die`).
- Spells section: per-spell **prepared toggle** (only for prepared mechanics per `casting_mechanic()`; spontaneous casters see the known list without toggles), concentration badge on concentration spells, casting via the dice tray (`start_concentration` on cast).
- Death saves: 3+3 pip UI (`roll_death_save` button with advantage picker; `is_dead()` banner).
- Exhaustion: 0–6 stepper.
- Rest buttons: short/long (`short_rest()`/`long_rest()`), snapshot undo before each.
- Conditions: add/remove from `ConditionKind` list (+custom), duration ticks, `tick_conditions()` button; immunity-checked by engine.
- Inventory: item list (equip/unequip toggles — `equipped` gates effects), add item (SRD picker lands in Phase 4; free-text entry now), gold, material-component costs display.
- Features/feats/traits: list + add/remove (`add_feat`, `add_trait`; `Custom(String)` homebrew escape hatch gets a plain editor).
- Every mutation: snapshot → mutate → `cx.notify()` → mark dirty. Tests per section: mutate via view, assert derived getter changed (e.g. toggle proficiency → `skill_check` +2).

### Task 3.3 — Dice tray (engine showcase)
- Bottom-right tray: quick roll buttons — attack (advantage picker, uses `roll_attack`), save (`roll_save`), check (`roll_check`), damage expression input + `apply_resistance` result line ("14 → 7 vs resistance"). Results land in a small log + status bar. This is the daily-use surface; keep it visible in sheet mode.

Phase gate: a real Level-1 sheet can be edited end-to-end; SRD spot-checks against the vault Wiki + vendored JSON per the blackletter skill's audit rules.

---

## Phase 4 — SRD browser + pickers

### Task 4.1 — Corpus loader
- `blackletter/src/srd.rs` (or `blackletter_srd` lib module): load vendored `5e-database/src/2014/en/5e-SRD-*.json` at startup into indexed in-memory tables (spells, items, feats, races, subraces, classes, subclasses, backgrounds, features, traits, **levels**). **Edition-tag every entry; default 2014** (user's table rulings are 2014; never let 2024 leak untagged — blackletter skill data rule). One canonical corpus: vendored 5e-database, NOT Open5e (slug-map idea deferred).
- **`5e-SRD-Levels.json` is the level-up backbone:** it maps class × subclass × level → feature/spell grants (`class_levels` on Classes too). The wizard and level-up flow read these tables instead of hand-coding PHB grant lists — the engine's `class.rs` PHB tables stay for slot/PB math; feature grants come from the corpus. Verify coverage per class with a test that walks levels 1–20 and asserts every `feature` reference resolves in `5e-SRD-Features.json`.
- `blackletter_types` may need `try_from_database` conversions for items/feats/races/classes/backgrounds (spells already have `Spell::try_from_database`). Add conversions in `blackletter_types` with tests, not in the app.
- Known data pitfalls to encode: SRD warlock slot table has a transcription bug (printed PHB wins — already handled in `spell_slots.rs`); spell material costs are absent from SRD JSON (default 0/pouch, hand-entered); `starting_equipment_options` carries the "choose X from Y" structure — parse choices, don't flatten.

### Task 4.2 — Picker UI (one component, many uses)
- `blackletter/src/picker.rs`: searchable list overlay (shale's picker/prompt field architecture — reuse its text-field + filter approach), columns per kind, detail pane below (description, DC, range, components). Byte-safe styled runs (gpui skill). `:`-style command entry optional; `/` opens picker from any section.
- Uses: add spell to spellbook, add item to inventory, add feat, set race, add class level (grants class features from the database), add feature/trait.
- Tests: filter "water walk" returns it; picking a spell produces a `sheet::Spell` via the conversion; edition filter excludes 2024 rows by default.

---

## Phase 5 — Character creation wizard + level-up

### Task 5.1 — New-character wizard
- `blackletter/src/wizard.rs`: linear flow with a step rail: 1) name 2) race (SRD picker + subrace when the race has subraces; applies `Race` grants + speed) 3) class (subclass choice deferred to the class's subclass level if level 1 doesn't grant it — check `Levels.json` per class) 4) **background** (SRD picker; applies `Background` grant: skill proficiencies, languages, feat, starting equipment) 5) ability scores (point-buy 27 + standard array + manual, per PHB) 6) class starting equipment (`starting_equipment_options` "choose X from Y" prompts, plus the package from `starting_equipment`) 7) level-1 features/skills/spells preview → confirm → `Character` with everything granted, `sync_*` called.
- Engine gaps to fill in `blackletter_types` first (with tests): class starting-features lookup from database, equipment-option choice parsing, skill-choice validation (class list × background skills). Flag each gap as its own commit.

### Task 5.2 — Level-up flow
- From sheet mode: "Level Up" action → per-level steps driven by `5e-SRD-Levels.json` (feature/spell grants per class × subclass × level): HP gain (roll / fixed / level-1 max−1 rules per PHB), subclass choice at the class's subclass level, feat at ASI levels (SRD feats picker), ability bumps. Multiclass: "Add Class" action runs the wizard's class steps minus scores.
- Tests: level 1→20 fighter ends with exactly the PHB feature list (assert against `Wiki/02 classes.md` lines AND the Levels.json grant set — they must agree); warlock gets Pact slots per the corrected PHB table; cleric level 1 starts with prepared list = all cleric spells (2014 prepared rule).

---

## Phase 6 — Polish + platform hardening

- Command palette / key-help overlay (`?` like chonk's help screen) listing all bindings per context.
- Session restore edge cases: file deleted since last run; campaign switch (close all docs, open the new campaign's set).
- Windows: full wine test gate per phase from here (`cargo test --workspace --target x86_64-pc-windows-gnu`, two-stage on cold tree, watch for `cfg`-gated warnings). `directories` paths via `PathBuf::join`, never string separators (chonk rule).
- README rewrite: user-facing controls table (chonk rule: docs updated in the same commit as the feature).
- Final gates: fmt 0 / clippy 0 / Linux tests / wine tests / live screenshots of both modes.

## Phase 7+ — Stretch (ruled "after core works", not scheduled)

- **Rhai house-rules console tab** on the sheet (expose `blackletter_scripting::Interface`; the old iced app had a Rhai `Mode` — the console is its successor).
- Party/session deep view (all sheets + map on one screen) — only if the light link proves insufficient at the table.
- 2024-edition flows (corpus already loads tagged; nothing ships 2024-only until asked).

---

## Decided (round 2, 2026-11-20) — see rulings 6–14 above

Campaign folders ✓ · party conditions+badges ✓ · lock DC + door open/closed ✓ · Rhai console → Phase 7+ · PDF export → deferred · prepared-caster support ✓.

## Still open (low stakes — agent decides unless you object)

- Theme: dark palette first (shale-style named palettes), light theme later.
- Autosave debounce: 2 s after last change + save-on-close; no periodic timer.
- Sheet file naming: `<Character Name>.json` in `characters/`, collision → ` (2)` suffix; rename = file rename + session update.
- Notes field: multi-line plain text only (no markdown rendering) until asked.

## Risks / open questions

- **gpui text fields:** de-risked — shale's `Field` widget + key table ports over (see Lessons). Remaining unknown: multi-line notes (spike at Phase 3 start; worst case = single-line notes v1).
- **SRD coverage:** vendored 5e-database 2014 lacks some PHB-only content (subclass feature granularity varies; `Levels.json` feature refs must be verified per class — Task 4.1 test). Wizard grants what the corpus has; missing features show as "add manually" rather than blocking.
- **Old egui dungeon saves:** format kept compatible + `locked`→`lock` shim (Task 2.3); legacy import copies from the old app-data dir (Task 1.2).
- **Roadmap drift:** the blackletter skill's roadmap said "GUI last, pick none of the three UIs" — this plan supersedes that ruling (user decision 2026-11-20, skill already updated); the CLI/MCP roadmap items stay valid but are not in this plan.

## File map (end state)

```
blackletter/            # GPUI app (replaces iced crate)
  src/main.rs root.rs session.rs files.rs picker.rs srd.rs
  src/sheet_view/ dungeon_view/ wizard/
blackletter_dungeon/    # headless dungeon core (ported from dungeon-tracker)
blackletter_types/      # engine (unchanged role; new conversions as needed)
blackletter_cli/ blackletter_scripting/   # untouched
5e-database/            # vendored SRD corpus (2014 default)
```

## Verification per phase

1. `cargo fmt --all --check` → 0; `cargo clippy --workspace --all-targets` → 0 warnings; `cargo test --workspace` green (one stage per terminal call).
2. Windows: build `--tests` for `x86_64-pc-windows-gnu` every phase; full wine test run from Phase 2 on, once per phase at the end.
3. Live: launch under `timeout`, locate window via `niri msg windows`, `grim` screenshot, vision-check both modes; keyboard recipes handed to the user (keys can't be injected into gpui).
