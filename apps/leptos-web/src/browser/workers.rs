use super::{
    Context,
    forms::{Action, Field, Form, Values},
};
use generated_contracts::{
    AccountId32, CalendarDefinition, CatalogKind, FundAccountRequest, Minutes, Money, Percentage,
    PromoteCoordinatorRequest, Qualifications, RegisterWorkerRequest, ScorePolicy,
    SetWorkerModeRequest, UpdateQualificationsRequest, UpsertCatalogEntryRequest, WeekOverride,
    WorkerMode,
};
use leptos::prelude::*;
use leptos_web::{parse_ids, parse_week, reputation_label, week_input};

fn ids_text(ids: &[u32]) -> String {
    ids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn qualifications(values: &Values) -> Result<Qualifications, String> {
    Ok(Qualifications {
        role_ids: parse_ids(&values.text("roles"))?,
        skill_ids: parse_ids(&values.text("skills"))?,
    })
}

fn calendar(values: &Values) -> Result<CalendarDefinition, String> {
    let overrides = values
        .text("overrides")
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (week, minutes) = line
                .split_once('=')
                .ok_or("Cada excepción debe tener formato YYYY-Www=minutos")?;
            Ok(WeekOverride {
                week: parse_week(week.trim())?,
                capacity: Minutes::new(minutes.trim().parse().map_err(|_| "Minutos no válidos")?),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let definition = CalendarDefinition {
        default_weekly_minutes: Minutes::new(values.parse("capacity")?),
        overrides,
    };
    definition
        .validate()
        .map_err(|_| "Calendario no válido o semanas duplicadas")?;
    Ok(definition)
}

#[component]
pub(super) fn Workers(account: AccountId32) -> impl IntoView {
    let context = expect_context::<Context>();
    view! {
        <section id="workers"><h2>"Perfil y disponibilidad"</h2><p class="muted">"La disponibilidad se define en minutos semanales. 60 minutos = 1 hora. Una excepción de 0 representa vacaciones; no borra compromisos existentes."</p>
        {move || {
            let own = context.workers.get().into_iter().find(|worker| worker.account == account);
            own.map_or_else(|| view! {
                <h3>"Registrarme como trabajador"</h3>
                <Form submit="Crear perfil y calendario" fields=vec![Field::text("name", "Nombre visible", ""), Field::text("roles", "IDs de roles, separados por coma", "").optional(), Field::text("skills", "IDs de todas tus habilidades", ""), Field::number("capacity", "Minutos disponibles por semana", 2400)]
                on_submit=Callback::new(move |values: Values| context.command("POST", "/workers".into(), &RegisterWorkerRequest { display_name: values.text("name"), qualifications: qualifications(&values)?, calendar: calendar(&values)? })) />
            }.into_any(), |worker| {
                let definition = &worker.calendar.definition;
                let overrides = definition.overrides.iter().map(|value| format!("{}={}", week_input(value.week), value.capacity.get())).collect::<Vec<_>>().join("\n");
                let capacity = definition.default_weekly_minutes.get();
                view! {
                    <div class="grid"><div><h3>"Habilidades"</h3>
                    <Form submit="Guardar habilidades" fields=vec![Field::text("roles", "IDs de roles (metadatos)", ids_text(&worker.qualifications.role_ids)).optional(), Field::text("skills", "IDs de habilidades", ids_text(&worker.qualifications.skill_ids))]
                    on_submit=Callback::new(move |values: Values| context.command("PUT", "/workers/me/qualifications".into(), &UpdateQualificationsRequest { qualifications: qualifications(&values)? })) />
                    <p>"Modo actual: "<span class="badge">{format!("{:?}", worker.mode)}</span></p>
                    {worker.coordinator_eligible.then(|| view! { <Form submit="Cambiar modo" fields=vec![Field::select("mode", "Modo activo (solo uno)", format!("{:?}", worker.mode), &["Worker", "Coordinator"])]
                    on_submit=Callback::new(move |values: Values| context.command("PUT", "/workers/me/mode".into(), &SetWorkerModeRequest { mode: if values.text("mode") == "Coordinator" { WorkerMode::Coordinator } else { WorkerMode::Worker } })) /> })}
                    <p class="muted">"La promoción a coordinador requiere permisos especiales. Cambiar el modo no cancela compromisos."</p></div>
                    <div><h3>"Calendario semanal"</h3>
                    <Form submit="Guardar disponibilidad" fields=vec![Field::number("capacity", "Minutos semanales por defecto", capacity), Field::area("overrides", "Excepciones: una línea YYYY-Www=minutos por semana", overrides)]
                    on_submit=Callback::new(move |values: Values| context.command("PUT", "/workers/me/calendar".into(), &calendar(&values)?)) />
                    <h4>"Compromisos"</h4><ul>{worker.calendar.committed_minutes.iter().map(|reservation| view! { <li>{format!("{}: {} minutos", week_input(reservation.week), reservation.minutes.get())}</li> }).collect_view()}</ul>
                    </div></div>
                }.into_any()
            })
        }}
        <details><summary>"Catálogo de roles y habilidades"</summary>{move || context.catalog.get().map(|catalog| view! {
            <div class="grid"><div><h3>"Roles"</h3><ul>{catalog.roles.into_iter().map(|role| view! { <li>{format!("{} · {}{}", role.id, role.name, if role.fixed { " (fijo)" } else { "" })}</li> }).collect_view()}</ul></div>
            <div><h3>"Habilidades"</h3><ul>{catalog.skills.into_iter().map(|skill| view! { <li>{format!("{} · {}", skill.id, skill.name)}</li> }).collect_view()}</ul></div></div>
        })}</details>
        <details><summary>"Directorio de trabajadores"</summary><div class="scroll"><table><thead><tr><th>"Persona / cuenta"</th><th>"Modo"</th><th>"Habilidades"</th><th>"Score trabajador"</th><th>"Score coordinador"</th></tr></thead><tbody>{move || context.workers.get().into_iter().map(|worker| view! {
            <tr><td>{worker.display_name}<br/><code>{worker.account.to_string()}</code></td><td>{format!("{:?}", worker.mode)}</td><td>{ids_text(&worker.qualifications.skill_ids)}</td><td>{reputation_label(&worker.worker_score)}</td><td>{reputation_label(&worker.coordinator_score)}</td></tr>
        }).collect_view()}</tbody></table></div></details></section>
    }
}

#[component]
pub(super) fn Administration() -> impl IntoView {
    let context = expect_context::<Context>();
    view! {
        <section><h2>"Administración local"</h2><p class="muted">"Acciones privilegiadas verificadas también en el proveedor. La financiación solo crea unidades simuladas."</p>
        <div class="grid"><div><h3>"Promover coordinador"</h3><Form fields=vec![Field::text("account", "Cuenta del trabajador (0x…)", "")] submit="Conceder elegibilidad" on_submit=Callback::new(move |values: Values| context.command("POST", "/admin/coordinators".into(), &PromoteCoordinatorRequest { account: values.parse("account")? })) />
        <h3>"Financiar una cuenta"</h3><Form fields=vec![Field::text("account", "Cuenta destinataria (0x…)", ""), Field::number("amount", "KVN enteros simulados", 1000)] submit="Añadir fondos de prueba" on_submit=Callback::new(move |values: Values| context.command("POST", "/admin/fund".into(), &FundAccountRequest { account: values.parse("account")?, amount: Money::new(values.parse("amount")?) })) /></div>
        <div><h3>"Editar catálogo"</h3><Form fields=vec![Field::select("kind", "Catálogo", "Skill", &["Skill", "Role"]), Field::number("id", "ID", 34), Field::text("name", "Nombre", "")] submit="Guardar entrada" on_submit=Callback::new(move |values: Values| {
            let kind = if values.text("kind") == "Role" { CatalogKind::Role } else { CatalogKind::Skill };
            let id = values.parse("id")?;
            if kind == CatalogKind::Role && id == 1 { return Err("El rol coordinator (1) es fijo".into()); }
            context.command("PUT", "/admin/catalog".into(), &UpsertCatalogEntryRequest { kind, id, name: values.text("name") })
        })/>
        <Form fields=vec![Field::select("kind", "Catálogo", "Skill", &["Skill", "Role"]), Field::number("id", "ID a eliminar", 34)] submit="Eliminar entrada" on_submit=Callback::new(move |values: Values| {
            let id: u32 = values.parse("id")?; let kind = values.text("kind");
            if kind == "Role" && id == 1 { return Err("El rol coordinator (1) es fijo".into()); }
            context.empty("DELETE", format!("/admin/catalog/{kind}/{id}"))
        })/>
        {move || context.catalog.get().map(|catalog| view! { <h3>"Pesos de reputación"</h3><Form fields=vec![Field::number("coordinator", "Coordinador (%)", catalog.score_policy.coordinator_percent().get()), Field::number("client", "Cliente (%)", catalog.score_policy.client_percent().get())] submit="Guardar pesos" on_submit=Callback::new(move |values: Values| {
            let policy = ScorePolicy::new(Percentage::new(values.parse("coordinator")?).map_err(|_| "Porcentaje entre 0 y 100")?, Percentage::new(values.parse("client")?).map_err(|_| "Porcentaje entre 0 y 100")?).map_err(|_| "Los porcentajes deben sumar 100")?;
            context.command("PUT", "/admin/score-policy".into(), &policy)
        })/> })}</div></div>
        <Action label="Actualizar catálogo y perfiles" on_click=Callback::new(move |()| { context.refresh(); Ok(()) })/>
        </section>
    }
}
