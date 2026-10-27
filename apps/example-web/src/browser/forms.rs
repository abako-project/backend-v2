use super::Context;
use leptos::prelude::*;
use std::{collections::BTreeMap, str::FromStr};

#[derive(Clone)]
pub(super) struct Field {
    pub name: String,
    label: String,
    kind: &'static str,
    value: String,
    required: bool,
    choices: Vec<(String, String)>,
}

impl Field {
    // Form declarations accept both owned defaults and numeric literals.
    #[allow(clippy::needless_pass_by_value)]
    pub(super) fn text(
        name: impl Into<String>,
        label: impl Into<String>,
        value: impl ToString,
    ) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            kind: "text",
            value: value.to_string(),
            required: true,
            choices: Vec::new(),
        }
    }
    pub fn number(name: impl Into<String>, label: impl Into<String>, value: impl ToString) -> Self {
        Self {
            kind: "number",
            ..Self::text(name, label, value)
        }
    }
    pub fn week(name: &str, label: &str, value: impl ToString) -> Self {
        Self {
            kind: "week",
            ..Self::text(name, label, value)
        }
    }
    pub fn area(name: &str, label: &str, value: impl ToString) -> Self {
        Self {
            kind: "textarea",
            required: false,
            ..Self::text(name, label, value)
        }
    }
    pub fn password(name: &str, label: &str) -> Self {
        Self {
            kind: "password",
            ..Self::text(name, label, "")
        }
    }
    pub fn select(name: &str, label: &str, value: impl ToString, choices: &[&str]) -> Self {
        Self {
            kind: "select",
            choices: choices.iter().map(|s| ((*s).into(), (*s).into())).collect(),
            ..Self::text(name, label, value)
        }
    }
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }
}

pub(super) struct Values(BTreeMap<String, String>);

impl Values {
    pub fn text(&self, name: &str) -> String {
        self.0.get(name).cloned().unwrap_or_default()
    }
    pub fn parse<T: FromStr>(&self, name: &str) -> Result<T, String> {
        self.text(name)
            .trim()
            .parse()
            .map_err(|_| format!("Valor inválido: {name}"))
    }
}

#[component]
pub(super) fn Form(
    fields: Vec<Field>,
    submit: &'static str,
    on_submit: Callback<Values, Result<(), String>>,
) -> impl IntoView {
    let context = expect_context::<Context>();
    let form = NodeRef::<leptos::html::Form>::new();
    let error = RwSignal::new(None::<String>);
    let names = fields
        .iter()
        .map(|field| field.name.clone())
        .collect::<Vec<_>>();
    view! {
        <form node_ref=form on:submit=move |event| {
            event.prevent_default();
            let result = (|| {
                let element = form.get().ok_or("Formulario no disponible")?;
                let data = web_sys::FormData::new_with_form(&element).map_err(|_| "No se pudo leer el formulario")?;
                let values = Values(names.iter().map(|name| (name.clone(), data.get(name).as_string().unwrap_or_default())).collect());
                let result = on_submit.run(values);
                // Credentials never remain in a hidden form after an attempted login.
                if names.iter().any(|name| name.contains("password")) { element.reset(); }
                result
            })();
            error.set(result.err());
        }>
            <fieldset disabled=move || context.busy.get()>
                <div class="fields">{fields.into_iter().map(|field| {
                    let input = match field.kind {
                        "textarea" => view! { <textarea name=field.name required=field.required>{field.value}</textarea> }.into_any(),
                        "select" => view! { <select name=field.name required=field.required>{field.choices.into_iter().map(|(value, label)| {
                            let selected = value == field.value;
                            view! { <option value=value selected=selected>{label}</option> }
                        }).collect_view()}</select> }.into_any(),
                        _ => view! { <input name=field.name type=field.kind value=field.value required=field.required min="0" step="1" autocomplete=if field.kind == "password" { "current-password" } else { "off" } /> }.into_any(),
                    };
                    view! { <label>{field.label}{input}</label> }
                }).collect_view()}</div>
                <button type="submit">{submit}</button>
            </fieldset>
            {move || error.get().map(|message| view! { <p class="notice error" role="alert">{message}</p> })}
        </form>
    }
}

#[component]
pub(super) fn Action(
    label: &'static str,
    on_click: Callback<(), Result<(), String>>,
    #[prop(default = false)] disabled: bool,
) -> impl IntoView {
    let context = expect_context::<Context>();
    view! {
        <button type="button" class="secondary" disabled=move || disabled || context.busy.get() on:click=move |_| {
            if let Err(error) = on_click.run(()) { context.error.set(Some(error)); }
        }>{label}</button>
    }
}
