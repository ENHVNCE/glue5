//! The mod's side of its menus: `ui.menu(title)` and the items' objects.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{Function, Lua, MultiValue, Table, Value};

use super::validate::{self, cut};
use super::Event;
use crate::{Menu, MenuInput, MenuItem, MenuKind, SHARED};

/// A mod's items, by id: the Lua object (its `value`) and its callback.
pub type Handlers = Rc<RefCell<HashMap<u32, (Table, Option<Function>)>>>;

/// The `ui` table: `ui.menu(title)`, `ui.notify(text)`.
pub fn install(lua: &Lua, module: &str, instance: u64, handlers: Handlers) -> mlua::Result<()> {
    let ui = lua.create_table()?;
    let m = module.to_string();
    ui.set(
        "notify",
        lua.create_function(move |_, text: String| {
            let text: String = text.chars().take(200).collect();
            let mut s = crate::lock(&SHARED.state);
            if s.notices.len() < 16 {
                s.notices.push_back((m.clone(), text));
            }
            Ok(())
        })?,
    )?;
    let m = module.to_string();
    let count = Rc::new(std::cell::Cell::new(0usize));
    ui.set(
        "menu",
        lua.create_function(move |lua, title: String| {
            if count.get() >= validate::TOP_MENUS {
                return Err(mlua::Error::runtime(format!(
                    "ui.menu: a mod has {} menus at most (menu:remove() one)",
                    validate::TOP_MENUS
                )));
            }
            count.set(count.get() + 1);
            let id = super::next_id();
            super::add_menu(Menu {
                module: m.clone(),
                instance,
                id,
                parent: None,
                title: cut(&title, validate::TITLE),
                note: None,
                items: Vec::new(),
                rev: 0,
            });
            menu_table(lua, id, handlers.clone(), count.clone())
        })?,
    )?;
    lua.globals().set("ui", ui)
}

/// A menu's Lua object: `:toggle`, `:slider`, `:choice`, `:button`,
/// `:label`, `:remove`.
fn menu_table(lua: &Lua, menu: u32, handlers: Handlers, count: Rc<std::cell::Cell<usize>>) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("id", menu)?;
    // Adds an item to the menu; its object, with `value` and `:set`.
    let add = {
        let handlers = handlers.clone();
        Rc::new(
            move |lua: &Lua, label: String, note: Option<String>, kind: MenuKind, callback: Option<Function>| -> mlua::Result<Table> {
                let id = super::next_id();
                let note = note.map(|n| cut(&n, validate::NOTE));
                let item = MenuItem { id, label: cut(&label, validate::LABEL), note, enabled: true, visible: true, kind: kind.clone() };
                if !super::add_item(menu, item) {
                    return Err(mlua::Error::runtime("the menu was removed"));
                }
                let obj = lua.create_table()?;
                obj.set("id", id)?;
                obj.set("value", value_of(&kind))?;
                let current = RefCell::new(kind);
                obj.set(
                    "set",
                    lua.create_function(move |_, (this, v): (Table, Value)| {
                        let mut kind = current.borrow_mut();
                        let mut new = Value::Nil;
                        match (&mut *kind, &v) {
                            (MenuKind::Toggle(on), Value::Boolean(b)) => {
                                *on = *b;
                                new = Value::Boolean(*b);
                            }
                            (MenuKind::Slider { value, min, max, step, .. }, Value::Integer(_) | Value::Number(_)) => {
                                *value = validate::slider(number(&v).unwrap_or(*value), *min, *max, *step);
                                new = Value::Number(*value);
                            }
                            (MenuKind::Choice { options, index }, Value::Integer(n)) if (1..=options.len() as i64).contains(n) => {
                                *index = (*n - 1) as usize;
                                new = Value::Integer(*n);
                            }
                            (_, Value::String(s)) => {
                                let label = cut(&s.to_string_lossy(), validate::LABEL);
                                super::edit_item(id, |i| i.label = label);
                            }
                            _ => {}
                        }
                        if !new.is_nil() {
                            super::set_kind(id, kind.clone());
                            this.set("value", new)?;
                        }
                        Ok(())
                    })?,
                )?;
                handlers.borrow_mut().insert(id, (obj.clone(), callback));
                Ok(obj)
            },
        )
    };
    let full = || mlua::Error::runtime(format!("a menu holds {} items at most", validate::ITEMS));
    let room = move |menu: u32| super::snapshot().iter().find(|m| m.id == menu).is_none_or(|m| m.items.len() < validate::ITEMS);
    let a = add.clone();
    t.set(
        "toggle",
        lua.create_function(move |lua, (_, label, on, a3, a4): (Table, String, Option<bool>, Value, Value)| {
            if !room(menu) {
                return Err(full());
            }
            let (note, cb) = note_and_callback(a3, a4);
            a(lua, label, note, MenuKind::Toggle(on.unwrap_or(false)), cb)
        })?,
    )?;
    let a = add.clone();
    t.set(
        "slider",
        lua.create_function(
            move |lua, (_, label, value, min, max, step, cb): (Table, String, f64, f64, f64, Option<f64>, Option<Function>)| {
                if !room(menu) {
                    return Err(full());
                }
                // NaN fails too.
                if !(min.is_finite() && max.is_finite() && min < max) {
                    return Err(mlua::Error::runtime("menu:slider: min must be below max"));
                }
                let step = step.filter(|s| *s > 0.0).unwrap_or((max - min) / 100.0);
                let value = validate::slider(value, min, max, step);
                a(lua, label, None, MenuKind::Slider { value, min, max, step, unit: None }, cb)
            },
        )?,
    )?;
    let a = add.clone();
    t.set(
        "choice",
        lua.create_function(move |lua, (_, label, options, index, cb): (Table, String, Vec<String>, Option<usize>, Option<Function>)| {
            if !room(menu) {
                return Err(full());
            }
            if options.is_empty() {
                return Err(mlua::Error::runtime("menu:choice: give it options"));
            }
            let options: Vec<String> = options.into_iter().take(validate::OPTIONS).map(|o| cut(&o, validate::OPTION)).collect();
            let index = index.unwrap_or(1).clamp(1, options.len()) - 1;
            a(lua, label, None, MenuKind::Choice { options: options.into(), index }, cb)
        })?,
    )?;
    let a = add.clone();
    t.set(
        "button",
        lua.create_function(move |lua, (_, label, a3, a4): (Table, String, Value, Value)| {
            if !room(menu) {
                return Err(full());
            }
            let (note, cb) = note_and_callback(a3, a4);
            a(lua, label, note, MenuKind::Button { action: None }, cb)
        })?,
    )?;
    let a = add.clone();
    t.set(
        "label",
        lua.create_function(move |lua, (_, text): (Table, String)| {
            if !room(menu) {
                return Err(full());
            }
            a(lua, text, None, MenuKind::Label, None)
        })?,
    )?;
    t.set(
        "remove",
        lua.create_function(move |_, _: Table| {
            let items: Vec<u32> = super::snapshot().iter().filter(|m| m.id == menu).flat_map(|m| m.items.iter().map(|i| i.id)).collect();
            let mut h = handlers.borrow_mut();
            for i in &items {
                h.remove(i);
            }
            if super::snapshot().iter().any(|m| m.id == menu) {
                count.set(count.get().saturating_sub(1));
            }
            super::remove(&items, &[menu]);
            Ok(())
        })?,
    )?;
    Ok(t)
}

/// `(label, [note], [callback])`, either may be left out.
fn note_and_callback(a: Value, b: Value) -> (Option<String>, Option<Function>) {
    match (a, b) {
        (Value::String(s), Value::Function(f)) => (Some(s.to_string_lossy()), Some(f)),
        (Value::String(s), _) => (Some(s.to_string_lossy()), None),
        (Value::Function(f), _) => (None, Some(f)),
        (_, Value::Function(f)) => (None, Some(f)),
        _ => (None, None),
    }
}

fn number(v: &Value) -> Option<f64> {
    match *v {
        Value::Integer(i) => Some(i as f64),
        Value::Number(n) => Some(n),
        _ => None,
    }
}

fn value_of(kind: &MenuKind) -> Value {
    match kind {
        MenuKind::Toggle(on) => Value::Boolean(*on),
        MenuKind::Slider { value, .. } => Value::Number(*value),
        MenuKind::Choice { index, .. } => Value::Integer(*index as i64 + 1),
        _ => Value::Nil,
    }
}

/// What waits for this run of the mod, as callbacks to run as tasks: the
/// items' objects take the players' values first.
pub fn take_calls(lua: &Lua, handlers: &Handlers, instance: u64) -> mlua::Result<Vec<(Function, MultiValue)>> {
    let mut calls = Vec::new();
    for event in super::take(instance) {
        let Event::Input(item, input) = event else { continue };
        let Some((obj, cb)) = handlers.borrow().get(&item).cloned() else { continue };
        let args = match input {
            MenuInput::Toggle(on) => {
                obj.set("value", on)?;
                MultiValue::from_vec(vec![Value::Boolean(on)])
            }
            MenuInput::Number(v) => {
                obj.set("value", v)?;
                MultiValue::from_vec(vec![Value::Number(v)])
            }
            MenuInput::Choose(i) => {
                obj.set("value", i as i64 + 1)?;
                let label = super::snapshot()
                    .iter()
                    .flat_map(|m| m.items.iter())
                    .find(|x| x.id == item)
                    .and_then(|x| match &x.kind {
                        MenuKind::Choice { options, .. } => options.get(i).cloned(),
                        _ => None,
                    })
                    .unwrap_or_default();
                MultiValue::from_vec(vec![Value::Integer(i as i64 + 1), Value::String(lua.create_string(label)?)])
            }
            _ => MultiValue::new(),
        };
        if let Some(f) = cb {
            calls.push((f, args));
        }
    }
    Ok(calls)
}
