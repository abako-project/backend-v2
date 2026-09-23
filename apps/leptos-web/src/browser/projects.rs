use super::{
    Context,
    forms::{Action, Field, Form, Values},
};
use generated_contracts::{
    AcceptMilestoneCompletionRequest, AccountId32, CreateProjectRequest, EntityId,
    EvidenceReference, EvidenceRequest, MilestoneDefinition, MilestoneStatus, MilestoneView,
    Minutes, Money, OpenDisputeRequest, PayloadHash, PlanningQuote, PlanningStatus, ProjectView,
    ProposalDefinition, ProposalStatus, ProposalView, ReasonRequest, RequestChangesRequest,
    RequestMilestoneCompletionRequest, RequirementDefinition, RevisionRequest, Score,
    TaskDefinition, TaskPriority, TaskProgressRequest, TaskStatus, TaskType, TaskView, TeamRating,
    UnixSeconds, WeekWindow, WorkerRating,
};
use leptos::prelude::*;
use leptos_web::{parse_ids, parse_week, week_input};

fn window(values: &Values) -> Result<WeekWindow, String> {
    WeekWindow::new(
        parse_week(&values.text("start"))?,
        parse_week(&values.text("end"))?,
    )
    .map_err(|_| "La semana final no puede ser anterior a la inicial".into())
}

fn score(values: &Values, key: &str) -> Result<Score, String> {
    Score::new(values.parse(key)?)
        .map_err(|_| "La puntuación debe ser un entero entre 0 y 10".into())
}
fn evidence(values: &Values) -> Result<EvidenceReference, String> {
    let hash = values
        .text("sha256")
        .parse::<PayloadHash>()
        .map_err(|_| "El SHA-256 debe tener 32 bytes hexadecimales con prefijo 0x")?;
    EvidenceReference::new(values.text("url"), hash)
        .map_err(|_| "La evidencia debe ser una URL HTTPS válida y sin credenciales".into())
}

fn enum_value<T: serde::de::DeserializeOwned>(values: &Values, key: &str) -> Result<T, String> {
    serde_json::from_value(serde_json::Value::String(values.text(key)))
        .map_err(|_| format!("Valor no válido: {key}"))
}

#[component]
pub(super) fn Projects(account: AccountId32) -> impl IntoView {
    let context = expect_context::<Context>();
    view! {
        <section id="projects"><h2>"Proyectos"</h2><details><summary>"Solicitar un nuevo trabajo"</summary>
        <p class="muted">"Se propondrá un coordinador elegible. Antes de empezar, revisa y acepta su tarifa de planificación."</p>
        <Form fields=vec![Field::text("title", "Título", ""), Field::area("description", "Qué necesitas", "")] submit="Solicitar trabajo"
        on_submit=Callback::new(move |values: Values| context.command("POST", "/projects".into(), &CreateProjectRequest { title: values.text("title"), description: values.text("description") })) /></details></section>
        {move || context.projects.with(Vec::is_empty).then(|| view! { <p class="empty">"Todavía no participas en proyectos."</p> })}
        {move || context.projects.get().into_iter().map(|project| view! { <Project project=project account=account/> }).collect_view()}
    }
}

#[component]
fn Project(project: ProjectView, account: AccountId32) -> impl IntoView {
    let context = expect_context::<Context>();
    let id = project.project_id;
    let coordinator = account == project.coordinator;
    let client = account == project.client;
    let active = !project.cancelled && project.active_dispute_id.is_none();
    let planning = project.planning.clone();
    let quote = planning.quote.clone();
    let revision = planning.revision;
    let planning_status = planning.status;
    let planning_complete = planning_status == PlanningStatus::Completed;
    view! {
        <article class="card"><div class="split"><h2>{project.title}</h2><span class="badge">{if project.cancelled { "Cancelado" } else if client { "Eres cliente" } else if coordinator { "Eres coordinador" } else { "Equipo" }}</span></div>
        <p>{project.description}</p><p class="muted">"Proyecto: "<code>{id.to_string()}</code></p>
        <p class="muted">"Coordinador: "<code>{project.coordinator.to_string()}</code></p>
        <div class="compact"><h3>"1. Acuerdo de planificación"</h3><p>"Estado: "{format!("{planning_status:?}")} " · Revisión "{revision}</p>
        {quote.clone().map(|quote| view! { <p>{format!("Precio: {} KVN · {} minutos · {} a {}", quote.fee.units(), quote.minutes.get(), week_input(quote.window.start()), week_input(quote.window.end()))}</p> })}
        <p>{format!("Fondos bloqueados de planificación: {} KVN{}", planning.escrow.units(), if planning.frozen { " · congelados" } else { "" })}</p>
        {(coordinator && active && matches!(planning_status, PlanningStatus::AwaitingQuote | PlanningStatus::Quoted)).then(|| view! {
            <Form fields=vec![Field::number("fee", "Tarifa fija de planificación (KVN)", quote.as_ref().map_or(0, |q| q.fee.units())), Field::number("minutes", "Minutos comprometidos de planificación", quote.as_ref().map_or(60, |q| q.minutes.get())), Field::week("start", "Semana inicial", quote.as_ref().map_or_else(String::new, |q| week_input(q.window.start()))), Field::week("end", "Semana final (incluida)", quote.as_ref().map_or_else(String::new, |q| week_input(q.window.end())))] submit="Proponer tarifa de planificación"
            on_submit=Callback::new(move |values: Values| context.command("POST", format!("/projects/{id}/planning/quote"), &PlanningQuote { fee: Money::new(values.parse("fee")?), minutes: Minutes::new(values.parse("minutes")?), window: window(&values)? })) />
        })}
        {(client && active && planning_status == PlanningStatus::Quoted).then(|| view! { <p class="muted">"Aceptar bloquea esta tarifa y reserva el tiempo del coordinador; aún no le paga."</p><Action label="Aceptar esta tarifa y reservar planificación" on_click=Callback::new(move |()| context.command("POST", format!("/projects/{id}/planning/accept"), &RevisionRequest { expected_revision: revision }))/> })}
        {(client && active && planning_status == PlanningStatus::Delivered).then(|| view! { <p class="muted">"Revisa el plan abajo. Aceptar la entrega paga la planificación, pero NO contrata la ejecución."</p><Action label="Aceptar entrega y pagar planificación" on_click=Callback::new(move |()| context.command("POST", format!("/projects/{id}/planning/accept-delivery"), &RevisionRequest { expected_revision: revision }))/> })}
        {((client || coordinator) && active).then(|| view! {
            <details><summary>"Abrir disputa de planificación"</summary><p class="muted">"Congela los fondos no liquidados. No hay pago ni reembolso automático."</p>
            <Form fields=vec![Field::text("reason", "Motivo de la disputa", "")] submit="Disputar planificación" on_submit=Callback::new(move |values: Values| context.command("POST", format!("/projects/{id}/planning/dispute"), &ReasonRequest { reason: values.text("reason") })) /></details>
        })}
        </div>
        <h3>"2. Propuestas y ejecución"</h3><p>{format!("Fondos bloqueados de ejecución: {} KVN", project.execution_escrow.units())}</p>
        {(coordinator && active && matches!(planning_status, PlanningStatus::Accepted | PlanningStatus::Delivered | PlanningStatus::Completed)).then(|| view! { <details><summary>"Preparar una nueva propuesta"</summary><ProposalEditor project_id=id initial=None/></details> })}
        {project.proposals.into_iter().map(|proposal| view! { <Proposal project_id=id proposal=proposal coordinator=coordinator client=client active=active planning_complete=planning_complete account=account/> }).collect_view()}
        {((client || coordinator) && active).then(|| view! {
            <details><summary>"Cancelar proyecto"</summary><p class="muted">"Los fondos pendientes se congelan. No se revierten pagos realizados ni se liberan compromisos automáticamente."</p>
            <Form fields=vec![Field::text("reason", "Motivo de cancelación", "")] submit="Confirmar cancelación" on_submit=Callback::new(move |values: Values| context.command("POST", format!("/projects/{id}/cancel"), &ReasonRequest { reason: values.text("reason") })) /></details>
        })}
        </article>
    }
}

#[component]
fn Proposal(
    project_id: EntityId,
    proposal: ProposalView,
    coordinator: bool,
    client: bool,
    active: bool,
    planning_complete: bool,
    account: AccountId32,
) -> impl IntoView {
    let context = expect_context::<Context>();
    let id = proposal.proposal_id;
    let revision = proposal.revision;
    let definition = ProposalDefinition {
        title: proposal.title.clone(),
        description: proposal.description.clone(),
        milestones: proposal
            .milestones
            .iter()
            .map(|milestone| milestone.definition.clone())
            .collect(),
    };
    let total = definition.total().map_or_else(
        |_| "Importe no válido".into(),
        |money| format!("{} KVN", money.units()),
    );
    let draft = proposal.status == ProposalStatus::Draft;
    let pending = proposal.status == ProposalStatus::PendingApproval;
    let editable = proposal.clone();
    view! {
        <div class="compact"><h3>{proposal.title}</h3><p>{proposal.description}</p><p><span class="badge">{format!("{:?}", proposal.status)}</span>{format!(" · Revisión {revision} · Total de ejecución: {total}")}</p>
        <p class="muted"><code>{id.to_string()}</code></p>
        {proposal.change_request.map(|reference| view! { <p class="notice">"Cambios solicitados: "{reference}</p> })}
        {(coordinator && active && draft).then(|| view! {
            <details><summary>"Editar propuesta"</summary><ProposalEditor project_id=project_id initial=Some(editable)/></details>
            <div class="actions"><Action label="Entregar plan al cliente" on_click=Callback::new(move |()| context.empty("POST", format!("/projects/{project_id}/proposals/{id}/submit")))/>
            <Action label="Eliminar este borrador" on_click=Callback::new(move |()| context.empty("DELETE", format!("/projects/{project_id}/proposals/{id}")))/></div>
        })}
        {(client && active && pending).then(|| view! {
            <p class="muted">"La contratación bloquea el importe total y asigna todo el equipo. Revisa cada semana, duración y precio antes de aceptar."</p>
            <Action label="Contratar ejecución de esta propuesta" disabled=!planning_complete on_click=Callback::new(move |()| context.command("POST", format!("/projects/{project_id}/proposals/{id}/approve"), &RevisionRequest { expected_revision: revision })) />
            {(!planning_complete).then(|| view! { <p class="muted">"Primero acepta la entrega de planificación."</p> })}
            <details><summary>"Solicitar cambios"</summary><Form fields=vec![Field::text("reference", "Explicación o referencia de los cambios", "")] submit="Devolver a borrador" on_submit=Callback::new(move |values: Values| context.command("POST", format!("/projects/{project_id}/proposals/{id}/changes"), &RequestChangesRequest { reference: values.text("reference") })) /></details>
        })}
        {proposal.milestones.into_iter().map(|milestone| view! { <Milestone project_id=project_id milestone=milestone coordinator=coordinator client=client active=active account=account/> }).collect_view()}
        </div>
    }
}

#[component]
fn ProposalEditor(project_id: EntityId, initial: Option<ProposalView>) -> impl IntoView {
    let context = expect_context::<Context>();
    let id = initial.as_ref().map(|view| view.proposal_id);
    let definition = initial.map_or_else(
        || ProposalDefinition {
            title: String::new(),
            description: String::new(),
            milestones: Vec::new(),
        },
        |view| ProposalDefinition {
            title: view.title,
            description: view.description,
            milestones: view
                .milestones
                .into_iter()
                .map(|milestone| milestone.definition)
                .collect(),
        },
    );
    let title = definition.title.clone();
    let description = definition.description.clone();
    let draft = RwSignal::new(definition);
    view! {
        <p class="muted">"Compón el borrador local: título, hitos y requisitos. Solo «Guardar propuesta» lo envía. Cada requisito representa una persona distinta y exige todas sus habilidades."</p>
        <Form fields=vec![Field::text("title", "Título del plan", title), Field::area("description", "Descripción del plan", description)] submit="Aplicar título al borrador"
        on_submit=Callback::new(move |values: Values| { draft.update(|definition| { definition.title = values.text("title"); definition.description = values.text("description"); }); Ok(()) }) />
        <p>"Título aplicado: "{move || draft.get().title}</p>
        {move || draft.get().milestones.into_iter().map(|milestone| {
            let key = milestone.key;
            view! {
                <div class="card"><h4>{format!("Hito {key}: {}", milestone.title)}</h4>
                <p>{format!("{} a {} · Coordinación: {} minutos / {} KVN", week_input(milestone.window.start()), week_input(milestone.window.end()), milestone.coordinator_minutes.get(), milestone.coordinator_fee.units())}</p>
                <details><summary>"Editar datos del hito"</summary><Form fields=vec![Field::text("title", "Nombre", milestone.title.clone()), Field::week("start", "Semana inicial", week_input(milestone.window.start())), Field::week("end", "Semana final", week_input(milestone.window.end())), Field::number("minutes", "Minutos de coordinación", milestone.coordinator_minutes.get()), Field::number("fee", "Tarifa del coordinador (KVN)", milestone.coordinator_fee.units())] submit="Aplicar cambios al hito" on_submit=Callback::new(move |values: Values| {
                    let window = window(&values)?; let minutes = Minutes::new(values.parse("minutes")?); let fee = Money::new(values.parse("fee")?);
                    draft.update(|definition| { if let Some(item) = definition.milestones.iter_mut().find(|item| item.key == key) { item.title = values.text("title"); item.window = window; item.coordinator_minutes = minutes; item.coordinator_fee = fee; } }); Ok(())
                })/></details>
                <ul class="list">{milestone.requirements.into_iter().map(|requirement| {
                    let requirement_key = requirement.key;
                    view! { <li>{format!("Puesto {} · rol {} · skills {:?} · {} minutos · {} KVN", requirement.key, requirement.role_id, requirement.skill_ids, requirement.minutes.get(), requirement.budget.units())}
                        <Action label="Quitar puesto del borrador" on_click=Callback::new(move |()| { draft.update(|definition| { if let Some(item) = definition.milestones.iter_mut().find(|item| item.key == key) { item.requirements.retain(|item| item.key != requirement_key); } }); Ok(()) })/>
                    </li> }
                }).collect_view()}</ul>
                <Form fields=vec![Field::number("role", "ID del rol (metadato)", 3), Field::text("skills", "IDs de TODAS las habilidades requeridas", ""), Field::number("minutes", "Minutos comprometidos del puesto", 60), Field::number("budget", "Pago del puesto (KVN)", 0)] submit="Añadir puesto al hito"
                on_submit=Callback::new(move |values: Values| {
                    let current = draft.get_untracked();
                    let milestone = current.milestones.iter().find(|milestone| milestone.key == key).ok_or("Hito no encontrado")?;
                    let next_key = milestone.requirements.iter().map(|item| item.key).max().unwrap_or(0).checked_add(1).ok_or("Demasiados requisitos")?;
                    let requirement = RequirementDefinition { key: next_key, role_id: values.parse("role")?, skill_ids: parse_ids(&values.text("skills"))?, minutes: Minutes::new(values.parse("minutes")?), budget: Money::new(values.parse("budget")?) };
                    draft.update(|definition| { if let Some(item) = definition.milestones.iter_mut().find(|item| item.key == key) { item.requirements.push(requirement); } }); Ok(())
                })/>
                <Action label="Quitar hito del borrador" on_click=Callback::new(move |()| { draft.update(|definition| definition.milestones.retain(|milestone| milestone.key != key)); Ok(()) })/>
                </div>
            }
        }).collect_view()}
        <details><summary>"Añadir hito"</summary><Form fields=vec![Field::text("title", "Nombre del hito", ""), Field::week("start", "Semana inicial", ""), Field::week("end", "Semana final (incluida)", ""), Field::number("minutes", "Minutos de coordinación", 60), Field::number("fee", "Tarifa de coordinación del hito (KVN)", 0)] submit="Añadir hito al borrador"
        on_submit=Callback::new(move |values: Values| {
            let key = draft.with_untracked(|definition| definition.milestones.iter().map(|item| item.key).max().unwrap_or(0)).checked_add(1).ok_or("Demasiados hitos")?;
            let milestone = MilestoneDefinition { key, title: values.text("title"), window: window(&values)?, coordinator_fee: Money::new(values.parse("fee")?), coordinator_minutes: Minutes::new(values.parse("minutes")?), requirements: Vec::new() };
            draft.update(|definition| definition.milestones.push(milestone)); Ok(())
        })/></details>
        <p>"Total del borrador: "{move || draft.get().total().map_or_else(|_| "Revisa los importes".into(), |money| format!("{} KVN", money.units()))}</p>
        <Action label="Guardar propuesta" on_click=Callback::new(move |()| {
            let definition = draft.get_untracked(); definition.validate().map_err(|error| error.to_string())?;
            let (method, path) = id.map_or_else(|| ("POST", format!("/projects/{project_id}/proposals")), |id| ("PUT", format!("/projects/{project_id}/proposals/{id}")));
            context.command(method, path, &definition)
        })/>
    }
}

#[component]
fn Milestone(
    project_id: EntityId,
    milestone: MilestoneView,
    coordinator: bool,
    client: bool,
    active: bool,
    account: AccountId32,
) -> impl IntoView {
    let context = expect_context::<Context>();
    let id = milestone.milestone_id;
    let status = milestone.status;
    let storage = milestone.task_storage.task_storage_id;
    let assignments = milestone.assignments.clone();
    let mut rating_fields = assignments
        .iter()
        .map(|assignment| {
            Field::number(
                assignment.worker.to_string(),
                format!("Score 0–10 para {}", assignment.worker),
                5,
            )
        })
        .collect::<Vec<_>>();
    rating_fields.extend([
        Field::text("url", "URL HTTPS del entregable", ""),
        Field::text("sha256", "SHA-256 del entregable (0x…)", ""),
    ]);
    let submission_id = milestone
        .submissions
        .last()
        .map(|submission| submission.submission_id);
    view! {
        <details><summary>{format!("Hito {} · {} · {}", milestone.definition.key, milestone.definition.title, status.map_or_else(|| "Sin ejecutar".into(), |status| format!("{status:?}")))}</summary>
        <p>{format!("{} a {} · Coordinador: {} minutos, {} KVN", week_input(milestone.definition.window.start()), week_input(milestone.definition.window.end()), milestone.definition.coordinator_minutes.get(), milestone.definition.coordinator_fee.units())}</p>
        {milestone.frozen.then(|| view! { <p class="notice">"Fondos congelados. No hay resolución automática de disputas."</p> })}
        {((client || coordinator) && active && status == Some(MilestoneStatus::ChangesRequested)).then(|| view! {
            <details><summary>"Abrir disputa por el rechazo vigente"</summary>
            <Form fields=vec![Field::text("url", "URL HTTPS de los argumentos públicos", ""), Field::text("sha256", "SHA-256 de los argumentos (0x…)", "")] submit="Abrir disputa y congelar el proyecto"
            on_submit=Callback::new(move |values: Values| {
                let rejected_submission_id = submission_id.ok_or("No existe una entrega rechazada vigente")?;
                context.command("POST", "/disputes".into(), &OpenDisputeRequest { project_id, milestone_id: id, rejected_submission_id, evidence: evidence(&values)? })
            }) /></details>
        })}
        <div class="scroll"><table><thead><tr><th>"Puesto"</th><th>"Habilidades"</th><th>"Minutos"</th><th>"KVN"</th><th>"Asignado"</th></tr></thead><tbody>{milestone.definition.requirements.into_iter().map(|requirement| {
            let assigned = milestone.assignments.iter().find(|item| item.requirement_key == requirement.key).map_or_else(|| "Sin asignar".into(), |item| item.worker.to_string());
            view! { <tr><td>{requirement.key}</td><td>{format!("{:?}", requirement.skill_ids)}</td><td>{requirement.minutes.get()}</td><td>{requirement.budget.units().to_string()}</td><td><code>{assigned}</code></td></tr> }
        }).collect_view()}</tbody></table></div>
        {(coordinator && active && matches!(status, Some(MilestoneStatus::InProgress | MilestoneStatus::ChangesRequested))).then(|| view! {
            <h4>"Entregar una versión para revisión"</h4><Form fields=rating_fields submit="Pedir aceptación del hito" on_submit=Callback::new(move |values: Values| {
                let worker_ratings = assignments.iter().map(|assignment| Ok(WorkerRating { worker: assignment.worker, score: score(&values, &assignment.worker.to_string())? })).collect::<Result<Vec<_>, String>>()?;
                context.command("POST", format!("/projects/{project_id}/milestones/{id}/request-completion"), &RequestMilestoneCompletionRequest { worker_ratings, deliverable: evidence(&values)? })
            })/>
        })}
        {(client && active && status == Some(MilestoneStatus::CompletionRequested)).then(|| view! {
            <h4>"Revisar la entrega vigente"</h4><p class="muted">"Aceptar liquida los pagos y registra la reputación una sola vez. Rechazar solicita cambios; no abre una disputa."</p>
            <Form fields=vec![Field::number("coordinator", "Score del coordinador (0–10)", 5), Field::select("team_mode", "Valoración del equipo", "Client", &["Client", "DelegateToCoordinator"]), Field::number("team", "Score del equipo (ignorado si delegas)", 5)] submit="Aceptar hito terminado y pagar"
            on_submit=Callback::new(move |values: Values| {
                let submission_id = submission_id.ok_or("No existe una entrega pendiente")?;
                context.command("POST", format!("/projects/{project_id}/milestones/{id}/accept-completion"), &AcceptMilestoneCompletionRequest { submission_id, coordinator_score: score(&values, "coordinator")?, team_rating: if values.text("team_mode") == "DelegateToCoordinator" { TeamRating::DelegateToCoordinator } else { TeamRating::Client(score(&values, "team")?) } })
            }) />
            <details><summary>"Rechazar y solicitar cambios"</summary>
            <Form fields=vec![Field::text("url", "URL HTTPS del motivo público", ""), Field::text("sha256", "SHA-256 del motivo (0x…)", "")] submit="Rechazar esta entrega"
            on_submit=Callback::new(move |values: Values| {
                let submission_id = submission_id.ok_or("No existe una entrega pendiente")?;
                context.command("POST", format!("/completion-submissions/{submission_id}/rejection"), &EvidenceRequest { evidence: evidence(&values)? })
            }) /></details>
        })}
        <h4>"Tareas de seguimiento"</h4><p class="muted">"Editar tareas no cambia asignaciones contractuales, reservas ni pagos. Storage: "<code>{storage.to_string()}</code></p>
        {(coordinator && active).then(|| view! { <details><summary>"Crear tarea"</summary><TaskEditor project_id=project_id storage=storage initial=None/></details> })}
        {milestone.task_storage.tasks.into_iter().map(|task| {
            let task_id = task.task_id;
            let assignee = task.task.assignees.contains(&account);
            let editable = task.clone();
            view! {
                <div class="card"><h4>{format!("#{} · {}", task.task_id, task.task.title)}</h4><p>{task.task.description.clone()}</p>
                <p>{format!("{:?} · {:?} · {:?} · {} / {} minutos", task.task.task_type, task.task.priority, task.task.status, task.task.logged_minutes.get(), task.task.estimated_minutes.get())}</p>
                <p class="muted">"Asignados: "{task.task.assignees.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")}</p>
                {(coordinator && active).then(|| view! { <details><summary>"Editar tarea"</summary><TaskEditor project_id=project_id storage=storage initial=Some(editable)/></details> })}
                {(assignee && !coordinator && active).then(|| view! {
                    <Form fields=vec![Field::select("status", "Estado", format!("{:?}", task.task.status), &["ToDo", "Open", "InProgress", "InReview", "Done", "Closed"]), Field::number("minutes", "Minutos registrados", task.task.logged_minutes.get())] submit="Actualizar mi progreso"
                    on_submit=Callback::new(move |values: Values| context.command("PATCH", format!("/projects/{project_id}/task-storages/{storage}/tasks/{task_id}/progress"), &TaskProgressRequest { status: enum_value(&values, "status")?, logged_minutes: Minutes::new(values.parse("minutes")?) })) />
                })}
                </div>
            }
        }).collect_view()}
        </details>
    }
}

#[component]
fn TaskEditor(project_id: EntityId, storage: EntityId, initial: Option<TaskView>) -> impl IntoView {
    let context = expect_context::<Context>();
    let id = initial.as_ref().map(|task| task.task_id);
    let task = initial.map_or_else(
        || TaskDefinition {
            title: String::new(),
            description: String::new(),
            task_type: TaskType::Task,
            priority: TaskPriority::Medium,
            status: TaskStatus::ToDo,
            assignees: Vec::new(),
            estimated_minutes: Minutes::ZERO,
            logged_minutes: Minutes::ZERO,
            due_at: None,
        },
        |view| view.task,
    );
    view! {
        <Form fields=vec![Field::text("title", "Título", task.title), Field::area("description", "Descripción", task.description), Field::select("type", "Tipo", format!("{:?}", task.task_type), &["Feature", "Bug", "Task", "Epic", "Story"]), Field::select("priority", "Prioridad", format!("{:?}", task.priority), &["Lowest", "Low", "Medium", "High", "Highest", "Blocker"]), Field::select("status", "Estado", format!("{:?}", task.status), &["ToDo", "Open", "InProgress", "InReview", "Done", "Closed"]), Field::text("assignees", "Cuentas asignadas (0x…, separadas por coma)", task.assignees.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")).optional(), Field::number("estimated", "Minutos estimados", task.estimated_minutes.get()), Field::number("logged", "Minutos registrados", task.logged_minutes.get()), Field::number("due", "Vencimiento Unix en segundos (opcional)", task.due_at.map_or_else(String::new, |due| due.get().to_string())).optional()] submit="Guardar tarea"
        on_submit=Callback::new(move |values: Values| {
            let text = values.text("assignees");
            let assignees = if text.trim().is_empty() { Vec::new() } else { text.split(',').map(|account| account.trim().parse().map_err(|_| "Cuenta asignada no válida".to_owned())).collect::<Result<Vec<AccountId32>, _>>()? };
            let task = TaskDefinition { title: values.text("title"), description: values.text("description"), task_type: enum_value(&values, "type")?, priority: enum_value(&values, "priority")?, status: enum_value(&values, "status")?, assignees, estimated_minutes: Minutes::new(values.parse("estimated")?), logged_minutes: Minutes::new(values.parse("logged")?), due_at: if values.text("due").trim().is_empty() { None } else { Some(UnixSeconds::new(values.parse("due")?)) } };
            let (method, path) = id.map_or_else(|| ("POST", format!("/projects/{project_id}/task-storages/{storage}/tasks")), |id| ("PUT", format!("/projects/{project_id}/task-storages/{storage}/tasks/{id}")));
            context.command(method, path, &task)
        })/>
    }
}
