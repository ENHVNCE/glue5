//! A menu's rows, one per item. Each row's
//! control follows the game's value in place; what the player does is sent
//! at once (a text or a typed number when it is committed: Enter or
//! leaving the field; Esc puts back the game's value).

use leptos::html;
use leptos::prelude::*;
use serde_json::{Value, json};

use super::model::{Item, Kind, decimals, parse_hex, shown, split_color};
use super::{Level, find_item, send};
use crate::state::*;

#[component]
pub(super) fn Row(s: S, module: String, menu: u32, id: u32, push: Callback<Level>) -> impl IntoView {
    // Kept when it goes (the row goes with it).
    let item = Memo::new(move |prev: Option<&Option<Item>>| find_item(s, menu, id).or_else(|| prev.cloned().flatten()));
    let Some(first) = item.get_untracked() else { return ().into_any() };
    let first = StoredValue::new(first);
    // Puts the controls back as the game has them (after a refused input).
    let resync = Trigger::new();
    let it = move || {
        resync.track();
        item.get().unwrap_or_else(|| first.get_value())
    };
    let label = move || it().label;
    let note = move || it().note.map(|n| view! { <small>{n}</small> });
    let off = Signal::derive(move || !it().enabled);
    let text = move || view! { <div class="mtext"><span>{label}</span>{note}</div> };
    let row_class = move |extra: &'static str| move || format!("mitem{extra}{}", if off.get() { " off" } else { "" });
    let module = StoredValue::new(module);
    let input = move |value: Value| send(s, &module.read_value(), id, value, resync);

    match item.get_untracked().map(|i| i.kind) {
        Some(Kind::Button { .. }) => {
            let action = move || match it().kind {
                Kind::Button { action: Some(a) } if !a.is_empty() => a,
                _ => "Run".to_string(),
            };
            view! {
                <div class=row_class("")>
                    {text}
                    <button class="btn small" prop:disabled=off on:click=move |_| input(Value::Null)>{action}</button>
                </div>
            }
            .into_any()
        }
        Some(Kind::Toggle { .. }) => {
            let on = move || matches!(it().kind, Kind::Toggle { value: true });
            view! {
                <div class=row_class("")>
                    {text}
                    <label class="switch">
                        <input type="checkbox" aria-label=label prop:checked=on prop:disabled=off
                            on:change=move |ev| input(json!(event_target_checked(&ev)))/>
                        <i></i>
                    </label>
                </div>
            }
            .into_any()
        }
        Some(Kind::Slider { .. }) => {
            let get = move || match it().kind {
                Kind::Slider { value, min, max, step, unit } => (value, min, max, step, unit),
                _ => (0.0, 0.0, 1.0, 0.01, None),
            };
            // Shown while dragging; sent when let go.
            let dragging = RwSignal::new(None::<f64>);
            view! {
                <div class=row_class("")>
                    {text}
                    <div class="mslider">
                        <input type="range" aria-label=label
                            min=move || get().1 max=move || get().2 step=move || get().3
                            prop:value=move || dragging.get().unwrap_or_else(|| get().0) prop:disabled=off
                            on:input=move |ev| dragging.set(event_target_value(&ev).parse().ok())
                            on:change=move |ev| {
                                dragging.set(None);
                                if let Ok(v) = event_target_value(&ev).parse::<f64>() {
                                    input(json!(v));
                                }
                            }/>
                        <span>{move || { let (v, _, _, step, unit) = get(); shown(dragging.get().unwrap_or(v), step, unit.as_deref()) }}</span>
                    </div>
                </div>
            }
            .into_any()
        }
        Some(Kind::Number { .. }) => {
            let get = move || match it().kind {
                Kind::Number { value, min, max, step, unit } => (value, min, max, step, unit),
                _ => (0.0, None, None, 1.0, None),
            };
            // `%` shows (and takes) the value times 100, as the slider.
            let k = move || if get().4.as_deref() == Some("%") { 100.0 } else { 1.0 };
            let field = NodeRef::<html::Input>::new();
            let focused = RwSignal::new(false);
            follow(field, focused, move || {
                let (v, _, _, step, _) = get();
                format!("{:.*}", decimals(step * k()), v * k())
            });
            let by = move |steps: f64| {
                let (v, min, max, step, _) = get();
                let next = (v + steps * step).clamp(min.unwrap_or(f64::MIN), max.unwrap_or(f64::MAX));
                input(json!(next));
            };
            let unit = move || get().4.filter(|u| u != "%").map(|u| view! { <span class="munit">{u}</span> });
            view! {
                <div class=row_class("")>
                    {text}
                    <div class="mnumber">
                        <button class="mstep" aria-label="Less" title="Less" prop:disabled=off on:click=move |_| by(-1.0)>"−"</button>
                        <input type="number" node_ref=field aria-label=label inputmode="decimal" prop:disabled=off
                            step=move || get().3 * k() min=move || get().1.map(|m| m * k()) max=move || get().2.map(|m| m * k())
                            on:focus=move |_| focused.set(true)
                            on:blur=move |_| { focused.set(false); resync.notify(); }
                            on:keydown=move |ev| commit_keys(&ev, field, move || format!("{:.*}", decimals(get().3 * k()), get().0 * k()))
                            on:change=move |ev| {
                                if let Ok(v) = event_target_value(&ev).trim().parse::<f64>() {
                                    input(json!(v / k()));
                                }
                            }/>
                        <button class="mstep" aria-label="More" title="More" prop:disabled=off on:click=move |_| by(1.0)>"+"</button>
                        {unit}
                    </div>
                </div>
            }
            .into_any()
        }
        Some(Kind::Choice { .. }) => {
            let get = move || match it().kind {
                Kind::Choice { value, options } => (value, options),
                _ => (1, Vec::new()),
            };
            view! {
                <div class=row_class("")>
                    {text}
                    <select class="mchoice" aria-label=label prop:disabled=off
                        on:change=move |ev| input(json!(event_target_value(&ev).parse::<u64>().unwrap_or(1)))>
                        {move || {
                            let (chosen, options) = get();
                            options.into_iter().enumerate().map(|(n, o)| view! { <option value=n + 1 selected=n + 1 == chosen>{o}</option> }).collect_view()
                        }}
                    </select>
                </div>
            }
            .into_any()
        }
        Some(Kind::Text { .. }) => {
            let get = move || match it().kind {
                Kind::Text { value, placeholder, max } => (value, placeholder, max),
                _ => (String::new(), None, 64),
            };
            let field = NodeRef::<html::Input>::new();
            let focused = RwSignal::new(false);
            follow(field, focused, move || get().0);
            view! {
                <div class=row_class(" wide")>
                    {text}
                    <input type="text" class="mtextin" node_ref=field aria-label=label spellcheck="false" autocomplete="off"
                        maxlength=move || get().2 placeholder=move || get().1.unwrap_or_default() prop:disabled=off
                        on:focus=move |_| focused.set(true)
                        on:blur=move |_| { focused.set(false); resync.notify(); }
                        on:keydown=move |ev| commit_keys(&ev, field, move || get().0)
                        on:change=move |ev| input(json!(event_target_value(&ev)))/>
                </div>
            }
            .into_any()
        }
        Some(Kind::Color { .. }) => {
            let get = move || match it().kind {
                Kind::Color { value, alpha } => (value, alpha),
                _ => ("#ffffff".to_string(), false),
            };
            let has_alpha = move || get().1;
            let rgb = move || split_color(&get().0).0;
            let opacity = move || split_color(&get().0).1;
            let with_alpha = move |rgb: &str, a: u8| if has_alpha() { format!("{rgb}{a:02x}") } else { rgb.to_string() };
            let hex = NodeRef::<html::Input>::new();
            let focused = RwSignal::new(false);
            follow(hex, focused, move || get().0);
            view! {
                <div class=row_class("")>
                    {text}
                    <div class="mcolor">
                        <Show when=has_alpha>
                            <input type="range" class="malpha" min="0" max="255" title="Opacity" aria-label="Opacity"
                                prop:value=opacity prop:disabled=off
                                on:change=move |ev| input(json!(with_alpha(&rgb(), event_target_value(&ev).parse().unwrap_or(255))))/>
                        </Show>
                        <input type="text" class="mhex" node_ref=hex aria-label=move || format!("{} (hex)", label()) spellcheck="false"
                            maxlength=move || if has_alpha() { 9 } else { 7 } prop:disabled=off
                            on:focus=move |_| focused.set(true)
                            on:blur=move |_| { focused.set(false); resync.notify(); }
                            on:keydown=move |ev| commit_keys(&ev, hex, move || get().0)
                            on:change=move |ev| match parse_hex(&event_target_value(&ev), has_alpha()) {
                                Some(c) => input(json!(c)),
                                None => {
                                    s.notice.set(Some(format!("{}: a colour is #rrggbb{}", label(), if has_alpha() { " or #rrggbbaa" } else { "" })));
                                    resync.notify();
                                }
                            }/>
                        <span class="mswatch" style:background=move || get().0>
                            <input type="color" aria-label=label prop:value=rgb prop:disabled=off
                                on:change=move |ev| input(json!(with_alpha(&event_target_value(&ev), opacity())))/>
                        </span>
                    </div>
                </div>
            }
            .into_any()
        }
        Some(Kind::Label) => view! { <div class="mitem label"><p>{label}</p>{note}</div> }.into_any(),
        Some(Kind::Separator) => view! { <h3 class=move || if label().is_empty() { "msep gap" } else { "msep" }>{label}</h3> }.into_any(),
        Some(Kind::Progress { .. }) => {
            let part = move || match it().kind {
                Kind::Progress { value } => value.clamp(0.0, 1.0),
                _ => 0.0,
            };
            view! {
                <div class=row_class("")>
                    {text}
                    <div class="mprogress" role="progressbar" aria-label=label aria-valuemin="0" aria-valuemax="100"
                        aria-valuenow=move || (part() * 100.0).round()>
                        <div class="mrail"><i style:width=move || format!("{:.1}%", part() * 100.0)></i></div>
                        <span>{move || format!("{:.0}%", part() * 100.0)}</span>
                    </div>
                </div>
            }
            .into_any()
        }
        Some(Kind::Submenu { menu: opens }) => view! {
            <button class=row_class(" mpush") prop:disabled=off on:click=move |_| push.run(Level::Menu(opens))>
                {text}
                <span class="mchev" aria-hidden="true">"›"</span>
            </button>
        }
        .into_any(),
        Some(Kind::Multi { .. } | Kind::Other) | None => ().into_any(),
    }
}

/// Keeps a field showing the game's value, except while it is being typed in.
fn follow(field: NodeRef<html::Input>, focused: RwSignal<bool>, value: impl Fn() -> String + 'static) {
    Effect::new(move |_| {
        let v = value();
        if let Some(el) = field.get()
            && !focused.get_untracked()
        {
            el.set_value(&v);
        }
    });
}

/// Enter commits a field (it loses the focus, which sends it); Esc puts the
/// game's value back first.
fn commit_keys(ev: &web_sys::KeyboardEvent, field: NodeRef<html::Input>, value: impl Fn() -> String) {
    let Some(el) = field.get_untracked() else { return };
    match ev.key().as_str() {
        "Enter" => {
            ev.prevent_default();
            let _ = el.blur();
        }
        "Escape" => {
            ev.prevent_default();
            el.set_value(&value());
            let _ = el.blur();
        }
        _ => {}
    }
}
