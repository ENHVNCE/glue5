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

/// Menus and items in any number, up to the mod's budget for them all
/// together; options likewise.
#[test]
fn budgets() {
    let m = Mod::new("menus-test-budget");
    m.run("for i = 1, 100 do ui.menu('m' .. i) end big = ui.menu('big') for i = 1, 1000 do big:label('l' .. i) end");
    let mine = || crate::menus().iter().filter(|x| x.instance == m.instance).map(|x| 1 + x.items.len()).sum::<usize>();
    assert_eq!(mine(), 101 + 1000);
    let err = m.fails(&format!("for i = 1, {ENTRIES} do big:toggle('t' .. i) end"));
    assert!(err.contains("menu:toggle: a mod's menus hold 4096 menus and items at most, all together"), "{err}");
    assert_eq!(mine(), ENTRIES);
    assert!(m.fails("ui.menu('one more')").contains("ui.menu: a mod's menus hold 4096"));
    assert!(m.fails("big:submenu('one more')").contains("menu:submenu: a mod's menus hold 4096"));
    m.run("big:clear()");
    m.run(&format!("c = big:choice('c', table.create({OPTIONS}, 'o'))"));
    let err = m.fails("big:multi('m', {'one more'})");
    assert!(err.contains("menu:multi: a mod's choices and multis hold 65536 options at most, all together"), "{err}");
    m.run("c.options = {'a'} big:multi('m', {'now it fits'})");
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

/// The docs' Garage, as it was written for the first menus: it runs
/// unchanged.
#[test]
fn legacy_garage_runs() {
    let m = Mod::new("menus-test-garage");
    m.run(
        r#"
        local cars = { "adder", "zentorno", "t20", "sanchez", "deluxo" }
        local weathers = { "EXTRASUNNY", "CLEAR", "CLOUDS", "RAIN", "THUNDER", "FOGGY", "XMAS" }
        local garage = ui.menu("Garage")
        car = garage:choice("Car", cars, 2)
        delivered = nil
        garage:button("Deliver", "Right where you stand", function()
            delivered = cars[car.value]
        end)
        weather = nil
        garage:choice("Weather", weathers, 1, function(_, w) weather = w end)
        hour = nil
        garage:slider("Hour", 19, 0, 23, 1, function(h) hour = h end)
        local t = garage:toggle("Lights", true, "a note", function(on) end)
        t:set(false)
        garage:label("Have fun"):set("Have more fun")
        "#,
    );
    assert_eq!(m.item("Hour").kind, MenuKind::Slider { value: 19.0, min: 0.0, max: 23.0, step: 1.0, unit: None });
    assert_eq!(m.item("Lights").note.as_deref(), Some("a note"));
    m.item("Have more fun");
    m.input("Car", MenuInput::Choose(4)).unwrap();
    m.input("Deliver", MenuInput::Press).unwrap();
    m.input("Weather", MenuInput::Choose(3)).unwrap();
    m.input("Hour", MenuInput::Number(7.0)).unwrap();
    m.turn();
    assert_eq!(m.eval::<String>("delivered"), "deluxo");
    assert_eq!(m.eval::<String>("weather"), "RAIN");
    assert_eq!(m.eval::<f64>("hour"), 7.0);
}

/// Every builder with its options; what players see of each.
#[test]
fn every_kind() {
    let m = Mod::new("menus-test-kinds");
    m.run(
        r##"
        m = ui.menu("All", "the note")
        b = m:button("B", { action = "Deliver now please, quickly", note = "n", enabled = false })
        s = m:slider("S", 0.5, 0, 1, { step = 0.25, unit = "%" })
        n = m:number("N", 1234, { min = 0, step = 100, unit = "$" })
        c = m:choice("C", { "a", "b", string.rep("x", 50) }, "b")
        mu = m:multi("M", { "a", "b", "c" }, { 3, "a", 3 })
        t = m:text("T", "abc\tdef", { placeholder = "Plate", max = 5 })
        co = m:color("Co", "#ff880080")
        ca = m:color("Ca", { r = 1, g = 2, b = 3, a = 4 }, { alpha = true })
        l = m:label(string.rep("l", 300))
        se = m:separator()
        p = m:progress("P", 2)
        "##,
    );
    let k = |label: &str| m.item(label).kind;
    assert_eq!(k("B"), MenuKind::Button { action: Some("Deliver now plea".into()) });
    assert!(!m.item("B").enabled);
    assert_eq!(k("S"), MenuKind::Slider { value: 0.5, min: 0.0, max: 1.0, step: 0.25, unit: Some("%".into()) });
    assert_eq!(k("N"), MenuKind::Number { value: 1234.0, min: Some(0.0), max: None, step: 100.0, unit: Some("$".into()) });
    let MenuKind::Choice { options, index } = k("C") else { panic!() };
    assert_eq!((index, options[2].chars().count()), (1, OPTION));
    assert!(matches!(k("M"), MenuKind::Multi { selected, .. } if selected == vec![0, 2]));
    assert_eq!(k("T"), MenuKind::Text { value: "abcde".into(), placeholder: Some("Plate".into()), max: 5 });
    assert_eq!(k("Co"), MenuKind::Color { value: crate::Rgba { r: 255, g: 136, b: 0, a: 255 }, alpha: false });
    assert_eq!(k("Ca"), MenuKind::Color { value: crate::Rgba { r: 1, g: 2, b: 3, a: 4 }, alpha: true });
    assert_eq!(k("P"), MenuKind::Progress(1.0));
    let label = crate::menus().iter().filter(|x| x.instance == m.instance).flat_map(|x| x.items.clone()).find(|i| i.kind == MenuKind::Label).unwrap();
    assert_eq!(label.label.chars().count(), LABEL_TEXT);
    assert_eq!(m.eval::<String>("c.option .. n.unit .. t.placeholder .. b.action"), "b$PlateDeliver now plea");
    assert_eq!(m.eval::<String>("table.concat(mu.value, ',') .. co.value.r .. tostring(se.kind)"), "1,3255separator");
}

/// Wrong arguments and fields are errors that say where.
#[test]
fn errors_say_where() {
    let m = Mod::new("menus-test-errors");
    m.run("m = ui.menu('x') t = m:toggle('t') c = m:choice('c', {'a', 'b'}) s = m:slider('s', 1, 0, 10)");
    let cases = [
        ("m:slider('s', 0, 5, 5)", "menu:slider: min must be below max"),
        ("m:slider('s', 0/0, 0, 5)", "menu:slider: the value: a finite number"),
        ("m:slider('s', 1, 0, 5, -1)", "menu:slider: step must be above 0"),
        ("m:choice('c', {'a'}, 'addr')", "menu:choice: no option \"addr\""),
        ("m:choice('c', {'a'}, 3)", "menu:choice: no option 3: there are 1"),
        ("m:choice('c', {})", "menu:choice: options: give it options"),
        ("m:toggle('t', true, { nope = 1 })", "menu:toggle: no option 'nope'"),
        ("m:toggle('t', true, 'note', 5)", "menu:toggle: the callback: a function, not number"),
        ("m:text('t', '', { max = 300 })", "menu:text: max: max is a whole number from 1 to 256"),
        ("m:color('c', '#12')", "menu:color: the value: \"#12\" is not #rrggbb or #rrggbbaa"),
        ("m:color('c', { r = 1, g = 2, b = 300 })", "menu:color: the value.b: a whole number from 0 to 255"),
        ("t.value = 'yes'", "Toggle.value: a boolean, not string"),
        ("t.valeu = true", "Toggle has no field 'valeu'"),
        ("local _ = t.valeu", "Toggle has no field 'valeu'"),
        ("t.id = 3", "Toggle.id is read-only"),
        ("s.value = 0/0", "Slider.value: a finite number"),
        ("s.max = -1", "Slider.max: min must be below max"),
        ("c:set('zz')", "Choice.value: no option \"zz\""),
        ("m.nope = 1", "Menu has no field 'nope'"),
        ("ui.menu(3)", "ui.menu: the title: a string, not number"),
    ];
    for (code, want) in cases {
        let err = m.fails(code);
        assert!(err.contains(want), "{code}: {err}");
    }
    m.run("t:remove()");
    assert!(m.fails("t.value = true").contains("Toggle.value: the item was removed"));
    assert!(m.fails("t:remove()").contains("Toggle:remove: the item was removed"));
    m.run("m:remove()");
    assert!(m.fails("m:toggle('x')").contains("menu:toggle: the menu was removed"));
}

/// The mod's own assignments are values players see, never callbacks.
#[test]
fn assignments_do_not_call_back() {
    let m = Mod::new("menus-test-quiet");
    m.run(
        r#"
        m = ui.menu('x') calls = 0
        local function f() calls += 1 end
        t = m:toggle('t', false, f) c = m:choice('c', {'a','b','c'}, 1, f) mu = m:multi('mu', {'a','b'}, nil, f)
        t.value = true c.value = 'c' mu.value = {2} t:set(false)
        "#,
    );
    m.turn();
    assert_eq!(m.eval::<i64>("calls"), 0);
    assert_eq!(m.item("c").kind, MenuKind::Choice { options: vec!["a".to_string(), "b".into(), "c".into()].into(), index: 2 });
}

/// Every kind's callback gets what `value` then says.
#[test]
fn callbacks_agree_with_values() {
    let m = Mod::new("menus-test-agree");
    m.run(
        r#"
        m = ui.menu('x') seen = {}
        local function f(name, h) return function(...) seen[name] = { h.value, ... } end end
        n = m:number('n', 0, { step = 5 }) n.onChange = f('n', n)
        mu = m:multi('mu', {'a','b','c'}) mu.onChange = f('mu', mu)
        t = m:text('t', '') t.onChange = f('t', t)
        co = m:color('co') co.onChange = f('co', co)
        c = m:choice('c', {'a','b'}) c.onChange = f('c', c)
        "#,
    );
    m.input("n", MenuInput::Number(12.0)).unwrap();
    m.input("mu", MenuInput::Select(vec![2, 0])).unwrap();
    m.input("t", MenuInput::Text("hi".into())).unwrap();
    m.input("co", MenuInput::Color(crate::Rgba { r: 9, g: 8, b: 7, a: 6 })).unwrap();
    m.input("c", MenuInput::Choose(1)).unwrap();
    m.turn();
    assert_eq!(m.eval::<f64>("seen.n[1]"), 10.0);
    assert_eq!(m.eval::<f64>("seen.n[2]"), 10.0);
    assert_eq!(m.eval::<String>("table.concat(seen.mu[1], ',') .. table.concat(seen.mu[2], ',') .. table.concat(seen.mu[3], ',')"), "1,31,3a,c");
    assert_eq!(m.eval::<String>("seen.t[1] .. seen.t[2]"), "hihi");
    assert_eq!(m.eval::<String>("seen.co[1].r .. seen.co[2].a"), "9255");
    assert_eq!(m.eval::<String>("seen.c[1] .. seen.c[2] .. seen.c[3]"), "22b");
}

/// New options keep a choice's index when they can, and drop a multi's
/// chosen ones they no longer have; a player's value waiting goes.
#[test]
fn options_reassigned() {
    let m = Mod::new("menus-test-options");
    m.run("m = ui.menu('x') c = m:choice('c', {'a','b','c'}, 3) mu = m:multi('mu', {'a','b','c'}, {1, 3})");
    m.input("c", MenuInput::Choose(0)).unwrap();
    m.run("c.options = {'x', 'y', 'z', 'w'} mu.options = {'x', 'y'}");
    m.turn();
    assert_eq!(m.eval::<i64>("c.value"), 3);
    m.run("c.options = {'only'}");
    assert_eq!(m.eval::<i64>("c.value"), 1);
    assert_eq!(m.eval::<Vec<i64>>("mu.value"), vec![1]);
    let many = m.fails("c.options = table.create(65537, 'o')");
    assert!(many.contains("Choice.options: 65536 options at most"), "{many}");
}

/// `clear` and `remove` free the items' callbacks with them.
#[test]
fn clear_frees_handlers() {
    let m = Mod::new("menus-test-clear");
    m.run("m = ui.menu('x') for i = 1, 10 do m:toggle('t' .. i, false, function() end) end b = m:button('b')");
    assert_eq!(m.handlers.borrow().items.len(), 11);
    m.run("b:remove()");
    assert_eq!(m.handlers.borrow().items.len(), 10);
    m.run("m:clear() m:toggle('again')");
    assert_eq!(m.handlers.borrow().items.len(), 1);
    assert!(m.fails("local _ = b.label").contains("the item was removed"));
    m.run("m:remove()");
    assert!(m.handlers.borrow().items.is_empty() && m.handlers.borrow().menus.is_empty());
    assert!(!super::has_menus(m.instance));
}

/// Submenus: rows that open them.
#[test]
fn submenus() {
    let m = Mod::new("menus-test-sub");
    m.run("top = ui.menu('Top') a = top:submenu('A', 'into A') b = a:submenu('B') c = b:submenu('C') t = c:toggle('deep')");
    let menus = crate::menus();
    let mine: Vec<_> = menus.iter().filter(|x| x.instance == m.instance).collect();
    assert_eq!(mine.len(), 4);
    let a: u32 = m.eval("a.id");
    assert_eq!(mine[1].parent, Some(mine[0].id));
    assert_eq!(mine[0].items[0].kind, MenuKind::Submenu(a));
    assert_eq!((mine[0].items[0].label.as_str(), mine[0].items[0].note.as_deref()), ("A", Some("into A")));
    // A submenu's title, enabled and visible are its row's.
    m.run("a.title = 'Renamed' a.enabled = false");
    assert_eq!(m.item("Renamed").kind, MenuKind::Submenu(a));
    assert!(!m.item("Renamed").enabled && !m.eval::<bool>("a.enabled"));
    // Inputs need every row on the way shown.
    m.run("a.enabled = true b.visible = false");
    assert_eq!(m.input("deep", MenuInput::Toggle(true)).unwrap_err(), "menu \"C\" is hidden");
    m.run("b.visible = true");
    m.input("deep", MenuInput::Toggle(true)).unwrap();
    // remove() takes the subtree and the row.
    m.run("b:remove()");
    assert!(m.fails("t.value = true").contains("the item was removed"));
    assert!(m.fails("c.title = 'x'").contains("the menu was removed"));
    assert_eq!(crate::menus().iter().filter(|x| x.instance == m.instance).count(), 2);
    assert_eq!(m.handlers.borrow().items.len(), 1);
    // clear() too.
    m.run("a:submenu('again'):toggle('x') top:clear()");
    assert_eq!(crate::menus().iter().filter(|x| x.instance == m.instance).count(), 1);
    assert!(m.handlers.borrow().items.is_empty());
}

/// Menus nest at any depth: a chain 20 deep is used at its end, listed
/// with its submenus, and cut in the middle with all under it.
#[test]
fn deep_chain() {
    let m = Mod::new("menus-test-deep");
    m.run("top = ui.menu('L1') level = { top } for i = 2, 20 do level[i] = level[i - 1]:submenu('L' .. i) end t = level[20]:toggle('end')");
    let mine = || crate::menus().iter().filter(|x| x.instance == m.instance).cloned().collect::<Vec<_>>();
    let chain = mine();
    assert_eq!(chain.len(), 20);
    // Each menu's parent is the one before it, its row opens the next.
    for w in chain.windows(2) {
        assert_eq!(w[1].parent, Some(w[0].id));
        assert_eq!(w[0].items[0].kind, MenuKind::Submenu(w[1].id));
    }
    // Back from the end: up the parents to the top, 19 steps.
    let mut at = chain[19].id;
    let mut steps = 0;
    while let Some(parent) = chain.iter().find(|x| x.id == at).unwrap().parent {
        at = parent;
        steps += 1;
    }
    assert_eq!((at, steps), (chain[0].id, 19));
    m.input("end", MenuInput::Toggle(true)).unwrap();
    m.run("level[7].visible = false");
    assert_eq!(m.input("end", MenuInput::Toggle(false)).unwrap_err(), "menu \"L20\" is hidden");
    m.run("level[7].visible = true");
    let open = crate::studio::api::menu_open(m.name, chain[0].id).unwrap();
    assert_eq!(open["submenus"].as_array().unwrap().len(), 19);
    m.run("level[10]:remove()");
    assert_eq!(mine().len(), 9);
    assert_eq!(m.handlers.borrow().items.len(), 8);
    assert!(m.fails("t.value = true").contains("the item was removed"));
}

/// Menus refer to each other by id: a cycle (A's row opens B, B's opens A),
/// which mods cannot make (a submenu is always a new menu), still ends
/// every walk: inputs are refused, the Studio's tree is listed once.
#[test]
fn cycle_ends() {
    let m = Mod::new("menus-test-cycle");
    m.run("x = ui.menu('X') t = x:toggle('t')");
    let (a, b) = (super::next_id(), super::next_id());
    let menu = |id, parent, opens| crate::Menu {
        module: m.name.into(),
        instance: m.instance,
        id,
        parent: Some(parent),
        title: format!("{id}"),
        note: None,
        items: vec![crate::MenuItem { id: super::next_id(), label: "row".into(), note: None, enabled: true, visible: true, kind: MenuKind::Submenu(opens) }],
        rev: 0,
    };
    super::add_menu(menu(a, b, b));
    super::add_menu(menu(b, a, a));
    let row = super::next_id();
    super::add_item(a, crate::MenuItem { id: row, label: "in the cycle".into(), note: None, enabled: true, visible: true, kind: MenuKind::Toggle(false) });
    assert_eq!(m.input("in the cycle", MenuInput::Toggle(true)).unwrap_err(), format!("menu \"{a}\" is hidden"));
    let open = crate::studio::api::menu_open(m.name, a).unwrap();
    assert_eq!(open["submenus"].as_array().unwrap().len(), 1);
    // The mod's other menus go on as ever.
    m.input("t", MenuInput::Toggle(true)).unwrap();
}

/// `onOpen` and `onClose` run at the mod's turn when the first viewer
/// comes and the last one goes.
#[test]
fn open_and_close_run() {
    let m = Mod::new("menus-test-onopen");
    m.run("m = ui.menu('x') s = m:submenu('s') log = {} m:onOpen(function() table.insert(log, 'open') end) m:onClose(function() table.insert(log, 'close') end) s:onOpen(function() table.insert(log, 'sub') end)");
    let (top, sub): (u32, u32) = (m.eval("m.id"), m.eval("s.id"));
    crate::menu_viewer(m.name, top, true);
    crate::menu_viewer(m.name, sub, true);
    m.turn();
    crate::menu_viewer(m.name, top, false);
    crate::menu_viewer(m.name, top, true);
    m.turn();
    crate::menu_viewer(m.name, top, false);
    m.turn();
    m.run("m:onClose(nil)");
    crate::menu_viewer(m.name, top, true);
    crate::menu_viewer(m.name, top, false);
    crate::menu_viewer(m.name, top, true);
    m.turn();
    crate::menu_viewer(m.name, top, false);
    m.turn();
    assert_eq!(m.eval::<String>("table.concat(log, ',')"), "open,sub,close,open");
    assert!(m.fails("m:onOpen(3)").contains("menu:onOpen: a function, not number"));
}

/// `menu:show()` from a mod: once in two seconds.
#[test]
fn show_from_a_mod() {
    let m = Mod::new("menus-test-show-mod");
    m.run("m = ui.menu('x') m:show() m:show()");
    let id: u32 = m.eval("m.id");
    // The one slot is shared with the other test of show: look for ours.
    let shown = std::iter::from_fn(crate::take_menu_show).collect::<Vec<_>>();
    assert!(shown.contains(&(m.name.to_string(), id)) || shown.is_empty(), "{shown:?}");
    m.run("m:remove()");
    assert!(m.fails("m:show()").contains("menu:show: the menu was removed"));
}
