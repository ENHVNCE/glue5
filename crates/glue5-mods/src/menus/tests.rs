//! The menus' rules, without a game: a Luau VM with `ui`, the shared
//! state as the app, the Studio and MCP use it. Tests run in parallel on
//! the one shared state: each uses a mod name of its own.

use mlua::Lua;

use super::validate::*;
use super::{Event, Handlers};
use crate::{MenuInput, MenuKind};

/// A mod's VM with `ui`, as vm.rs makes it.
struct Mod {
    lua: Lua,
    name: &'static str,
    instance: u64,
    handlers: Handlers,
}

impl Mod {
    fn new(name: &'static str) -> Mod {
        let lua = Lua::new();
        let instance = super::next_instance();
        let handlers = Handlers::default();
        super::install(&lua, name, instance, handlers.clone()).unwrap();
        Mod { lua, name, instance, handlers }
    }

    fn run(&self, code: &str) {
        self.lua.load(code).exec().unwrap_or_else(|e| panic!("{e}"));
    }

    fn fails(&self, code: &str) -> String {
        self.lua.load(code).exec().unwrap_err().to_string()
    }

    fn eval<T: mlua::FromLua>(&self, code: &str) -> T {
        self.lua.load(code).eval().unwrap_or_else(|e| panic!("{e}"))
    }

    /// The mod's turn: the callbacks run (here, to the end).
    fn turn(&self) {
        for (f, args) in super::take_calls(&self.lua, &self.handlers, self.instance).unwrap() {
            f.call::<()>(args).unwrap();
        }
    }

    /// Its item labelled `label`, as players see it.
    fn item(&self, label: &str) -> crate::MenuItem {
        crate::menus()
            .iter()
            .filter(|m| m.instance == self.instance)
            .flat_map(|m| m.items.iter())
            .find(|i| i.label == label)
            .cloned()
            .unwrap_or_else(|| panic!("no item {label}"))
    }

    fn input(&self, label: &str, input: MenuInput) -> Result<(), String> {
        crate::menu_input(self.name, self.item(label).id, input)
    }
}

impl Drop for Mod {
    fn drop(&mut self) {
        super::remove_instance(self.instance);
    }
}

#[test]
fn menus_per_mod() {
    let m = Mod::new("menus-test-limit");
    let err = m.fails(&format!("for i = 1, {} do ui.menu('m' .. i) end", TOP_MENUS + 1));
    assert!(err.contains("ui.menu: a mod has 16 menus at most (menu:remove() one)"), "{err}");
    assert_eq!(crate::menus().iter().filter(|x| x.instance == m.instance).count(), TOP_MENUS);
}

#[test]
fn items_per_menu() {
    let m = Mod::new("menus-test-items");
    m.run(&format!("m = ui.menu('x') for i = 1, {ITEMS} do m:label('l' .. i) end"));
    let err = m.fails("m:toggle('one more')");
    assert!(err.contains("64 items at most"), "{err}");
}

/// The player's value is the menu's at once; the mod's `value` and its
/// callback get it at its turn, together.
#[test]
fn inputs_reach_the_mod_at_its_turn() {
    let m = Mod::new("menus-test-turn");
    m.run("m = ui.menu('x') got = nil t = m:toggle('t', false, function(v) got = v end)");
    m.input("t", MenuInput::Toggle(true)).unwrap();
    assert_eq!(m.item("t").kind, MenuKind::Toggle(true));
    assert!(!m.eval::<bool>("t.value"));
    m.turn();
    assert!(m.eval::<bool>("t.value and got"));
}

/// A dragged slider is one callback per turn, with the last value.
#[test]
fn values_coalesce() {
    let m = Mod::new("menus-test-coalesce");
    m.run("m = ui.menu('x') calls = {} s = m:slider('s', 0, 0, 10, 1, function(v) table.insert(calls, v) end)");
    for v in [1.0, 4.0, 7.2] {
        m.input("s", MenuInput::Number(v)).unwrap();
    }
    m.turn();
    assert_eq!(m.eval::<Vec<f64>>("calls"), vec![7.0]);
    assert_eq!(m.eval::<f64>("s.value"), 7.0);
}

/// The mod setting a value after a player did, before its turn: the mod's
/// value stays and no callback runs for the player's.
#[test]
fn the_mods_assignment_wins() {
    let m = Mod::new("menus-test-assign");
    m.run("m = ui.menu('x') calls = 0 t = m:toggle('t', false, function() calls += 1 end)");
    m.input("t", MenuInput::Toggle(true)).unwrap();
    m.run("t:set(false)");
    m.turn();
    assert_eq!(m.eval::<i64>("calls"), 0);
    assert_eq!(m.item("t").kind, MenuKind::Toggle(false));
}

/// Presses are never coalesced; past the cap the oldest goes.
#[test]
fn presses_queue_up_to_the_cap() {
    let m = Mod::new("menus-test-press");
    m.run("m = ui.menu('x') n = 0 b = m:button('b', function() n += 1 end)");
    for _ in 0..PRESSES + 10 {
        m.input("b", MenuInput::Press).unwrap();
    }
    m.turn();
    assert_eq!(m.eval::<usize>("n"), PRESSES);
}

/// What players send is checked: kind, range, items that are disabled,
/// hidden or gone, another mod's.
#[test]
fn inputs_are_checked() {
    let m = Mod::new("menus-test-check");
    m.run("m = ui.menu('x') c = m:choice('c', {'a', 'b'}) t = m:toggle('t')");
    assert_eq!(m.input("c", MenuInput::Choose(2)).unwrap_err(), "no option 3: there are 2");
    assert_eq!(m.input("c", MenuInput::Toggle(true)).unwrap_err(), "a choice takes one option");
    let id = m.item("t").id;
    assert!(crate::menu_input("another-mod", id, MenuInput::Toggle(true)).unwrap_err().contains("has no menu item"));
    super::edit_item(id, |i| i.enabled = false);
    assert_eq!(m.input("t", MenuInput::Toggle(true)).unwrap_err(), "\"t\" is disabled");
    super::edit_item(id, |i| i.visible = false);
    assert_eq!(m.input("t", MenuInput::Toggle(true)).unwrap_err(), "\"t\" is hidden");
    m.run("m:remove()");
    assert!(crate::menu_input(m.name, id, MenuInput::Toggle(true)).is_err());
}

/// The first viewer opens a menu, the last one gone closes it; an open and
/// close before the mod's turn is neither.
#[test]
fn viewers_count() {
    let m = Mod::new("menus-test-viewers");
    m.run("m = ui.menu('x')");
    let id: u32 = m.eval("m.id");
    crate::menu_viewer(m.name, id, true);
    crate::menu_viewer(m.name, id, true);
    assert_eq!(super::take(m.instance), vec![Event::Viewer(id, true)]);
    crate::menu_viewer(m.name, id, false);
    assert_eq!(super::take(m.instance), vec![]);
    crate::menu_viewer(m.name, id, false);
    crate::menu_viewer(m.name, id, false);
    assert_eq!(super::take(m.instance), vec![Event::Viewer(id, false)]);
    crate::menu_viewer(m.name, id, true);
    crate::menu_viewer(m.name, id, false);
    assert_eq!(super::take(m.instance), vec![]);
    // Another mod's name: nothing.
    crate::menu_viewer("another-mod", id, true);
    assert_eq!(super::take(m.instance), vec![]);
}

/// `menu:show()` is honoured once in two seconds per mod.
#[test]
fn show_is_rate_limited() {
    assert!(super::request_show("menus-test-show", 7));
    assert!(!super::request_show("menus-test-show", 8));
    assert!(super::request_show("menus-test-show-2", 9));
    assert_eq!(crate::take_menu_show(), Some(("menus-test-show-2".into(), 9)));
    assert_eq!(crate::take_menu_show(), None);
}

/// Every change bumps its menu's `rev`; readers share one snapshot until
/// then.
#[test]
fn snapshot_and_rev() {
    let m = Mod::new("menus-test-rev");
    m.run("m = ui.menu('x') t = m:toggle('t')");
    let rev = |menus: &[crate::Menu]| menus.iter().find(|x| x.instance == m.instance).unwrap().rev;
    let a = crate::menus();
    m.input("t", MenuInput::Toggle(true)).unwrap();
    let b = crate::menus();
    assert!(rev(&b) > rev(&a));
    assert_eq!(rev(&a), rev(&a.clone()));
}
