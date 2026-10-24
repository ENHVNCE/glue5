//! The mods' menus (`ui.menu` in Luau), usable from the browser as in the
//! game: what is done here goes to the mod, which hears it at its next turn.
//! One card per top-level menu; a submenu, or a long list's picker, opens
//! inside its card, with a way back.

mod model;
mod rows;

use std::sync::{Arc, Mutex};

use leptos::prelude::*;
use serde_json::{Value, json};

pub(crate) use model::MenuInfo;
use model::{Item, Kind};

use crate::net::request;
use crate::state::*;

/// What a card shows over its menu: a submenu, or an item's picker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Level {
    Menu(u32),
    Pick(u32),
}

/// Sends a player's input to item `item` of `module`; when the game refuses
/// it the page says why and `resync` puts the control back as the game has it.
pub(super) fn send(s: S, module: &str, item: u32, value: Value, resync: Trigger) {
    request("menus.input", json!({ "mod": module, "item": item, "value": value }), move |r| {
        if let Err(e) = r {
            s.notice.set(Some(e));
            resync.notify();
        }
    });
}

/// A menu as the game has it now.
pub(super) fn find_menu(s: S, id: u32) -> Option<MenuInfo> {
    s.menus.with(|m| m.iter().find(|m| m.id == id).cloned())
}

/// Item `item` of menu `menu`, as the game has it now.
pub(super) fn find_item(s: S, menu: u32, item: u32) -> Option<Item> {
    s.menus.with(|m| m.iter().find(|m| m.id == menu).and_then(|m| m.items.iter().find(|i| i.id == item).cloned()))
}

#[component]
pub(crate) fn Menus(s: S) -> impl IntoView {
    let top = move || s.menus.with(|m| m.iter().filter(|m| m.parent.is_none()).map(|m| (m.id, m.module.clone())).collect::<Vec<_>>());
    view! {
        <div class="menus">
            <Show when=move || s.menus.with(|m| !m.iter().any(|m| m.parent.is_none()))>
                <p class="empty">"No menus. A mod makes one with ui.menu(\"Title\"): it shows here and in the game (F7)."</p>
            </Show>
            <For each=top key=|m| m.0 let((id, module))>
                <MenuCard s=s root=id module=module/>
            </For>
        </div>
    }
}

/// A top-level menu's card, with what is open in it (its stack).
#[component]
fn MenuCard(s: S, root: u32, module: String) -> impl IntoView {
    let stack = Memo::new(move |_| s.menu_stacks.with(|m| m.get(&root).cloned().unwrap_or_default()));
    let set_stack = move |levels: Vec<Level>| {
        s.menu_stacks.update(|m| {
            m.insert(root, levels);
        });
    };
    let push = Callback::new(move |level: Level| {
        let mut levels = stack.get_untracked();
        levels.push(level);
        set_stack(levels);
    });
    let pop = Callback::new(move |()| {
        let mut levels = stack.get_untracked();
        levels.pop();
        set_stack(levels);
    });

    // What the mod removed or hid goes from the stack (and what was over it).
    Effect::new(move |_| {
        let levels = stack.get();
        let kept = s.menus.with(|menus| {
            let mut under = root;
            let mut kept = 0;
            for level in &levels {
                let parent = menus.iter().find(|m| m.id == under);
                let ok = match *level {
                    Level::Menu(id) => {
                        let row = parent.and_then(|p| p.items.iter().find(|i| i.kind == Kind::Submenu { menu: id }));
                        row.is_some_and(|i| i.visible) && menus.iter().any(|m| m.id == id)
                    }
                    Level::Pick(item) => parent
                        .and_then(|p| p.items.iter().find(|i| i.id == item))
                        .is_some_and(|i| i.visible && i.enabled && matches!(i.kind, Kind::Choice { .. } | Kind::Multi { .. })),
                };
                if !ok {
                    break;
                }
                if let Level::Menu(id) = *level {
                    under = id;
                }
                kept += 1;
            }
            kept
        });
        if kept < levels.len() {
            set_stack(levels[..kept].to_vec());
        }
    });

    // The menus this card shows are looked at (their onOpen / onClose run in
    // the mod): its own, and each submenu open in it, while the page shows
    // the menus and is connected.
    let opened = Arc::new(Mutex::new(Vec::<u32>::new()));
    {
        let (opened, module) = (opened.clone(), module.clone());
        Effect::new(move |_| {
            let looking = s.connected.get() && (!s.phone.get() || s.screen.get() == Screen::Menus);
            let want: Vec<u32> = if looking {
                std::iter::once(root)
                    .chain(stack.get().into_iter().filter_map(|l| match l {
                        Level::Menu(id) => Some(id),
                        Level::Pick(_) => None,
                    }))
                    .collect()
            } else {
                Vec::new()
            };
            let mut had = opened.lock().unwrap();
            for id in had.iter().filter(|id| !want.contains(id)) {
                view(&module, *id, false);
            }
            for id in want.iter().filter(|id| !had.contains(id)) {
                view(&module, *id, true);
            }
            *had = want;
        });
    }
    {
        let module = module.clone();
        on_cleanup(move || {
            for id in opened.lock().unwrap().drain(..) {
                view(&module, id, false);
            }
        });
    }

    // The level shown: the card's menu, a submenu or a picker.
    let top = Memo::new(move |_| stack.get().last().copied());
    let shown_menu = Memo::new(move |_| {
        stack.get().iter().rev().find_map(|l| match l {
            Level::Menu(id) => Some(*id),
            Level::Pick(_) => None,
        })
        .unwrap_or(root)
    });
    let title = move |id: u32| find_menu(s, id).map(|m| m.title).unwrap_or_default();
    let header = {
        let module = module.clone();
        move || {
            let levels = stack.get();
            if levels.is_empty() {
                let t = title(root);
                // Its mod, unless the title says it already.
                let by = (!t.eq_ignore_ascii_case(&module)).then(|| view! { <span>{module.clone()}</span> });
                return view! { <b>{t}</b>{by}<Note s=s menu=root/> }.into_any();
            }
            // ‹ Trainer › Vehicles: each crumb but the last goes back to it.
            let mut under = root;
            let mut names = vec![title(root)];
            for l in &levels {
                names.push(match *l {
                    Level::Menu(id) => {
                        under = id;
                        title(id)
                    }
                    Level::Pick(item) => find_item(s, under, item).map(|i| i.label).unwrap_or_default(),
                });
            }
            let last = names.len() - 1;
            view! {
                <button class="mback" title="Back" aria-label="Back" on:click=move |_| pop.run(())>"‹"</button>
                <nav class="mcrumbs" aria-label="Where">
                    {names.into_iter().enumerate().map(|(n, name)| {
                        let sep = (n > 0).then(|| view! { <i aria-hidden="true">"›"</i> });
                        if n == last {
                            view! { {sep}<b aria-current="page">{name}</b> }.into_any()
                        } else {
                            let back_to = levels[..n].to_vec();
                            view! { {sep}<button on:click=move |_| set_stack(back_to.clone())>{name}</button> }.into_any()
                        }
                    }).collect_view()}
                </nav>
            }
            .into_any()
        }
    };
    view! {
        <section class="mcard">
            <header class=move || if stack.with(Vec::is_empty) { "" } else { "nav" }>{header}</header>
            {move || match top.get() {
                Some(Level::Pick(item)) => view! { <rows::Picker s=s module=module.clone() menu=shown_menu.get_untracked() item=item pop=pop/> }.into_any(),
                _ => view! { <MenuBody s=s module=module.clone() menu=shown_menu.get() push=push/> }.into_any(),
            }}
        </section>
    }
}

/// A top-level menu's note, under its title.
#[component]
fn Note(s: S, menu: u32) -> impl IntoView {
    move || find_menu(s, menu).and_then(|m| m.note).map(|n| view! { <small class="mnote">{n}</small> })
}

/// A menu's rows.
#[component]
fn MenuBody(s: S, module: String, menu: u32, push: Callback<Level>) -> impl IntoView {
    // A row is made once per item (and kind): its values follow the game in
    // place, so a field being typed in keeps its text and focus.
    let rows = Memo::new(move |_| {
        s.menus.with(|m| {
            m.iter()
                .find(|m| m.id == menu)
                .map(|m| m.items.iter().filter(|i| i.visible && i.kind != Kind::Other).map(|i| (i.id, i.kind.name())).collect::<Vec<_>>())
                .unwrap_or_default()
        })
    });
    view! {
        <Show when=move || rows.with(Vec::is_empty)>
            <p class="mempty">"Nothing in it yet"</p>
        </Show>
        <For each=move || rows.get() key=|r| *r let((id, _))>
            <rows::Row s=s module=module.clone() menu=menu id=id push=push/>
        </For>
    }
}

/// Tells the game this page starts or stops looking at a menu.
fn view(module: &str, menu: u32, open: bool) {
    // Nothing to say when it fails: the game closes a page's menus as it goes.
    request("menus.view", json!({ "mod": module, "menu": menu, "open": open }), |_| {});
}
