//! The mods' menus (`ui.menu` in Luau), usable from the browser as in the
//! game: what is done here goes to the mod, which hears it at its next turn.

mod model;
mod rows;

use leptos::prelude::*;
use serde_json::{Value, json};

pub(crate) use model::MenuInfo;
use model::{Item, Kind};

use crate::net::request;
use crate::state::*;

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
    let all = move || s.menus.with(|m| m.iter().map(|m| (m.id, m.module.clone())).collect::<Vec<_>>());
    view! {
        <div class="menus">
            <Show when=move || s.menus.with(Vec::is_empty)>
                <p class="empty">"No menus. A mod makes one with ui.menu(\"Title\"): it shows here and in the game (F7)."</p>
            </Show>
            <For each=all key=|m| m.0 let((id, module))>
                <MenuCard s=s id=id module=module/>
            </For>
        </div>
    }
}

/// A menu's card.
#[component]
fn MenuCard(s: S, id: u32, module: String) -> impl IntoView {
    let title = move || find_menu(s, id).map(|m| m.title).unwrap_or_default();
    let header = {
        let module = module.clone();
        move || {
            let t = title();
            // Its mod, unless the title says it already.
            let by = (!t.eq_ignore_ascii_case(&module)).then(|| view! { <span>{module.clone()}</span> });
            view! { <b>{t}</b>{by}<Note s=s menu=id/> }
        }
    };
    view! {
        <section class="mcard">
            <header>{header}</header>
            <MenuBody s=s module=module menu=id/>
        </section>
    }
}

/// A menu's note, under its title.
#[component]
fn Note(s: S, menu: u32) -> impl IntoView {
    move || find_menu(s, menu).and_then(|m| m.note).map(|n| view! { <small class="mnote">{n}</small> })
}

/// A menu's rows.
#[component]
fn MenuBody(s: S, module: String, menu: u32) -> impl IntoView {
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
            <rows::Row s=s module=module.clone() menu=menu id=id/>
        </For>
    }
}
