//! CSR-only browser boundary. No provider, custody, or database dependency.
// Leptos generates public props builders for crate-local components.
#![allow(unreachable_pub)]
mod forms;
mod projects;
mod workers;

use forms::{Action, Field, Form};
use generated_contracts::{
    ApiError, BalanceView, CatalogView, ChangePasswordRequest, LoginRequest, NotificationView,
    NotificationsPage, OperationId, OperationRef, OperationView, ProjectView, RegisterRequest,
    SessionView, WorkerSummaryView,
};
use gloo_net::http::RequestBuilder;
use leptos::{prelude::*, task::spawn_local};
use leptos_web::{OperationResult, operation_result};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use wasm_bindgen::{JsCast, closure::Closure};

#[derive(Clone, Copy)]
struct Context {
    base: StoredValue<String>,
    session: RwSignal<Option<SessionView>>,
    generation: RwSignal<u64>,
    busy: RwSignal<bool>,
    loading: RwSignal<bool>,
    error: RwSignal<Option<String>>,
    notice: RwSignal<String>,
    catalog: RwSignal<Option<CatalogView>>,
    workers: RwSignal<Vec<WorkerSummaryView>>,
    projects: RwSignal<Vec<ProjectView>>,
    balance: RwSignal<Option<BalanceView>>,
    notifications: RwSignal<Vec<NotificationView>>,
    operations: RwSignal<Vec<Tracked>>,
    polling: RwSignal<bool>,
}

#[derive(Clone, PartialEq)]
struct Mutation {
    method: &'static str,
    path: String,
    body: Option<Value>,
}

#[derive(Clone)]
struct Tracked {
    id: OperationId,
    mutation: Option<Mutation>,
    view: Option<OperationView>,
    checks: u8,
    response_lost: bool,
}

impl Context {
    fn new() -> Self {
        let base = web_sys::window()
            .and_then(|window| {
                js_sys::Reflect::get(&window, &"KUNVENO_API_BASE".into())
                    .ok()?
                    .as_string()
            })
            .unwrap_or_else(|| "/api".into());
        Self {
            base: StoredValue::new(base.trim_end_matches('/').into()),
            session: RwSignal::new(None),
            generation: RwSignal::new(0),
            busy: RwSignal::new(false),
            loading: RwSignal::new(true),
            error: RwSignal::new(None),
            notice: RwSignal::new(String::new()),
            catalog: RwSignal::new(None),
            workers: RwSignal::new(Vec::new()),
            projects: RwSignal::new(Vec::new()),
            balance: RwSignal::new(None),
            notifications: RwSignal::new(Vec::new()),
            operations: RwSignal::new(Vec::new()),
            polling: RwSignal::new(false),
        }
    }

    fn endpoint(self, path: &str) -> String {
        format!("{}{path}", self.base.get_value())
    }

    async fn request<T: DeserializeOwned>(
        self,
        method: &str,
        path: &str,
        body: Option<&Value>,
        key: Option<OperationId>,
    ) -> Result<T, String> {
        let mut builder = RequestBuilder::new(&self.endpoint(path))
            .method(method.parse().map_err(|_| "Método no válido")?)
            .credentials(web_sys::RequestCredentials::Include)
            .header("Accept", "application/json");
        if method != "GET" {
            if let Some(session) = self.session.get_untracked() {
                builder = builder.header("X-CSRF-Token", &session.csrf_token);
            }
            if let Some(key) = key {
                builder = builder.header("Idempotency-Key", &key.to_string());
            }
        }
        let abort =
            web_sys::AbortController::new().map_err(|_| "No se pudo iniciar la petición")?;
        builder = builder.abort_signal(Some(&abort.signal()));
        let timeout = leptos::leptos_dom::helpers::set_timeout_with_handle(
            move || abort.abort(),
            std::time::Duration::from_secs(15),
        )
        .map_err(|_| "No se pudo iniciar el tiempo máximo de petición")?;
        let response = match body {
            Some(body) => {
                builder
                    .json(body)
                    .map_err(|_| "Petición no válida")?
                    .send()
                    .await
            }
            None => builder.send().await,
        };
        timeout.clear();
        let response = response.map_err(|_| "Conexión interrumpida. Conserva la referencia de operación antes de repetir una acción.")?;
        if !response.ok() {
            let status = response.status();
            return Err(response.json::<ApiError>().await.map_or_else(
                |_| format!("La API devolvió HTTP {status}"),
                |error| format!("{}: {}", error.code, error.message),
            ));
        }
        if response.status() == 204 {
            return serde_json::from_value(Value::Null)
                .map_err(|_| "Respuesta vacía inesperada".into());
        }
        response
            .json()
            .await
            .map_err(|_| "Respuesta incompatible con el contrato de la API".into())
    }

    fn command<T: Serialize>(
        self,
        method: &'static str,
        path: String,
        body: &T,
    ) -> Result<(), String> {
        self.submit(Mutation {
            method,
            path,
            body: Some(serde_json::to_value(body).map_err(|_| "Datos no válidos")?),
        })
    }

    fn empty(self, method: &'static str, path: String) -> Result<(), String> {
        self.submit(Mutation {
            method,
            path,
            body: None,
        })
    }

    fn submit(self, mutation: Mutation) -> Result<(), String> {
        if self.busy.get_untracked() {
            return Err("Hay una petición en curso".into());
        }
        if self.operations.with_untracked(|items| {
            items.iter().any(|item| {
                item.mutation.as_ref() == Some(&mutation)
                    && item
                        .view
                        .as_ref()
                        .is_none_or(|view| operation_result(view) == OperationResult::Pending)
            })
        }) {
            return Err("Esta acción ya está pendiente. Consulta su referencia en Operaciones; no crees otra.".into());
        }
        if self.operations.with_untracked(Vec::len) >= 32 {
            return Err("Hay 32 operaciones en esta vista. Revisa y limpia las terminadas antes de continuar.".into());
        }
        let mut bytes = [0; 16];
        web_sys::window()
            .ok_or("Navegador no disponible")?
            .crypto()
            .map_err(|_| "Entropía no disponible")?
            .get_random_values_with_u8_array(&mut bytes)
            .map_err(|_| "Entropía no disponible")?;
        let id = OperationId::from_bytes(bytes);
        self.operations.update(|items| {
            items.push(Tracked {
                id,
                mutation: Some(mutation.clone()),
                view: None,
                checks: 0,
                response_lost: false,
            });
        });
        self.send(id, mutation);
        Ok(())
    }

    fn send(self, id: OperationId, mutation: Mutation) {
        self.busy.set(true);
        self.error.set(None);
        let generation = self.generation.get_untracked();
        spawn_local(async move {
            let result = self
                .request::<OperationRef>(
                    mutation.method,
                    &mutation.path,
                    mutation.body.as_ref(),
                    Some(id),
                )
                .await;
            if self.generation.get_untracked() != generation {
                return;
            }
            self.busy.set(false);
            match result {
                Ok(reference) if reference.operation_id == id => {
                    self.notice
                        .set(format!("Operación {id} recibida; pendiente de ejecución."));
                    self.operations.update(|items| {
                        if let Some(item) = items.iter_mut().find(|item| item.id == id) {
                            item.response_lost = false;
                            item.checks = 0;
                        }
                    });
                    self.poll();
                }
                Ok(_) => self.error.set(Some(
                    "La API devolvió otra referencia; no repitas la acción".into(),
                )),
                Err(error) => {
                    self.operations.update(|items| {
                        if let Some(item) = items.iter_mut().find(|item| item.id == id) {
                            item.response_lost = true;
                        }
                    });
                    self.error.set(Some(error));
                }
            }
        });
    }

    fn poll(self) {
        if self.polling.get_untracked() || self.session.get_untracked().is_none() {
            return;
        }
        let pending = self.operations.with_untracked(|items| {
            items
                .iter()
                .filter(|item| {
                    item.checks < 30
                        && item
                            .view
                            .as_ref()
                            .is_none_or(|view| operation_result(view) == OperationResult::Pending)
                })
                .cloned()
                .collect::<Vec<_>>()
        });
        if pending.is_empty() {
            return;
        }
        self.polling.set(true);
        let generation = self.generation.get_untracked();
        spawn_local(async move {
            let mut changed = false;
            for item in pending {
                let result = self
                    .request::<OperationView>(
                        "GET",
                        &format!("/operations/{}", item.id),
                        None,
                        None,
                    )
                    .await;
                if self.generation.get_untracked() != generation {
                    return;
                }
                self.operations.update(|items| {
                    if let Some(tracked) = items.iter_mut().find(|tracked| tracked.id == item.id) {
                        tracked.checks = tracked.checks.saturating_add(1);
                        if let Ok(view) = result {
                            changed |= operation_result(&view) != OperationResult::Pending;
                            tracked.view = Some(view);
                        }
                    }
                });
            }
            self.polling.set(false);
            if changed {
                self.refresh();
            }
        });
    }

    fn refresh(self) {
        if self.loading.get_untracked() || self.session.get_untracked().is_none() {
            return;
        }
        self.loading.set(true);
        let generation = self.generation.get_untracked();
        spawn_local(async move {
            let result = async {
                let catalog = self
                    .request::<CatalogView>("GET", "/catalog", None, None)
                    .await?;
                let workers = self
                    .request::<Vec<WorkerSummaryView>>("GET", "/workers", None, None)
                    .await?;
                let projects = self
                    .request::<Vec<ProjectView>>("GET", "/projects", None, None)
                    .await?;
                let balance = self
                    .request::<BalanceView>("GET", "/balance", None, None)
                    .await?;
                Ok::<_, String>((catalog, workers, projects, balance))
            }
            .await;
            if self.generation.get_untracked() != generation {
                return;
            }
            self.loading.set(false);
            match result {
                Ok((catalog, workers, projects, balance)) => {
                    self.catalog.set(Some(catalog));
                    self.workers.set(workers);
                    self.projects.set(projects);
                    self.balance.set(Some(balance));
                }
                Err(error) => self.error.set(Some(error)),
            }
        });
    }

    fn authenticate<T: Serialize>(self, path: &'static str, body: &T) -> Result<(), String> {
        let body = serde_json::to_value(body).map_err(|_| "Credenciales no válidas")?;
        self.busy.set(true);
        self.error.set(None);
        spawn_local(async move {
            let result = self
                .request::<SessionView>("POST", path, Some(&body), None)
                .await;
            self.busy.set(false);
            match result {
                Ok(session) => {
                    self.replace_session(Some(session));
                    self.refresh();
                }
                Err(error) => self.error.set(Some(error)),
            }
        });
        Ok(())
    }

    fn replace_session(self, session: Option<SessionView>) {
        self.generation
            .update(|value| *value = value.wrapping_add(1));
        self.notifications.set(Vec::new());
        self.operations.set(Vec::new());
        self.projects.set(Vec::new());
        self.workers.set(Vec::new());
        self.balance.set(None);
        self.catalog.set(None);
        self.notice.set(String::new());
        self.busy.set(false);
        self.loading.set(false);
        self.polling.set(false);
        self.session.set(session);
    }

    fn ordinary<T: Serialize>(
        self,
        path: String,
        body: Option<&T>,
        logout: bool,
    ) -> Result<(), String> {
        let body = body
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| "Datos no válidos")?;
        self.busy.set(true);
        let generation = self.generation.get_untracked();
        spawn_local(async move {
            let result = self
                .request::<Value>("POST", &path, body.as_ref(), None)
                .await;
            if self.generation.get_untracked() != generation {
                return;
            }
            self.busy.set(false);
            match result {
                Ok(value) => {
                    if logout {
                        self.replace_session(None);
                    } else if let Ok(notification) =
                        serde_json::from_value::<NotificationView>(value)
                    {
                        self.notifications
                            .update(|items| merge_notification(items, notification));
                    } else {
                        self.notice.set("Cambio guardado".into());
                    }
                }
                Err(error) => self.error.set(Some(error)),
            }
        });
        Ok(())
    }
}

fn merge_notification(items: &mut Vec<NotificationView>, notification: NotificationView) {
    if let Some(existing) = items
        .iter_mut()
        .find(|item| item.notification_id == notification.notification_id)
    {
        // A delayed SSE frame cannot erase an explicitly acknowledged read.
        let read_at = existing.read_at.or(notification.read_at);
        *existing = notification;
        existing.read_at = read_at;
    } else {
        items.push(notification);
    }
    items.sort_by_key(|item| item.notification_id);
    // ponytail: bounded browser window; older history stays queryable in the API.
    if items.len() > 500 {
        items.drain(..items.len() - 500);
    }
}

#[component]
pub(super) fn App() -> impl IntoView {
    let context = Context::new();
    provide_context(context);
    spawn_local(async move {
        let session = context
            .request::<SessionView>("GET", "/auth/session", None, None)
            .await
            .ok();
        context.replace_session(session);
        context.refresh();
    });
    match leptos::leptos_dom::helpers::set_interval_with_handle(
        move || context.poll(),
        std::time::Duration::from_secs(2),
    ) {
        Ok(timer) => on_cleanup(move || timer.clear()),
        Err(_) => context.error.set(Some(
            "La consulta automática no está disponible; usa Consultar operaciones.".into(),
        )),
    }
    view! {
        <a href="#main" class="skip">"Saltar al contenido"</a>
        <header><strong>"kunveno"</strong><span class="badge">"Prueba de concepto · KVN simulado"</span></header>
        <main id="main">
            <h1>"Tu espacio de trabajo"</h1>
            <p class="muted">"Planifica por semanas, acuerda precios y sigue cada hito. Las operaciones se firman en el backend."</p>
            <div role="status" aria-live="polite">{move || if context.loading.get() { "Cargando datos…".into() } else { context.notice.get() }}</div>
            {move || context.error.get().map(|message| view! { <div role="alert" class="notice error">{message}<button class="secondary" on:click=move |_| context.error.set(None)>"Cerrar aviso"</button></div> })}
            {move || context.session.get().map_or_else(|| view! { <Authentication/> }.into_any(), |session| view! {
                <section><div class="split"><div><h2>{session.display_name.clone()}</h2><p>"Cuenta: "<code>{session.account_id.to_string()}</code></p>
                <p>"Disponible: "{move || context.balance.get().map_or_else(|| "—".into(), |balance| format!("{} KVN", balance.available.units()))}</p></div>
                <div class="actions"><Action label="Actualizar datos" on_click=Callback::new(move |()| { context.refresh(); Ok(()) })/>
                <Action label="Cerrar sesión" on_click=Callback::new(move |()| context.ordinary::<Value>("/auth/logout".into(), None, true))/></div></div>
                <nav aria-label="Secciones"><a href="#projects">"Proyectos"</a><a href="#workers">"Perfil y disponibilidad"</a><a href="#notifications">"Notificaciones"</a><a href="#operations">"Operaciones"</a></nav>
                <details><summary>"Cambiar contraseña"</summary><Form submit="Guardar contraseña" fields=vec![Field::password("current_password", "Contraseña actual"), Field::password("new_password", "Contraseña nueva")]
                on_submit=Callback::new(move |values: forms::Values| context.ordinary("/auth/password".into(), Some(&ChangePasswordRequest { current_password: values.text("current_password"), new_password: values.text("new_password") }), false))/></details>
                </section>
                <Operations/>
                <projects::Projects account=session.account_id/>
                <workers::Workers account=session.account_id/>
                {session.is_admin.then(|| view! { <workers::Administration/> })}
                <Notifications/>
            }.into_any())}
            <footer class="muted">"Sin fondos reales. La aceptación de la planificación y la contratación de la ejecución son acciones distintas."</footer>
        </main>
    }
}

#[component]
fn Authentication() -> impl IntoView {
    let context = expect_context::<Context>();
    view! {
        <div class="grid">
            <section><h2>"Entrar"</h2><Form submit="Iniciar sesión" fields=vec![Field::text("username", "Usuario", ""), Field::password("password", "Contraseña")]
            on_submit=Callback::new(move |values: forms::Values| context.authenticate("/auth/login", &LoginRequest { username: values.text("username"), password: values.text("password") })) /></section>
            <section><h2>"Crear cuenta"</h2><p class="muted">"Puedes solicitar proyectos. El registro como trabajador se hace después."</p>
            <Form submit="Crear cuenta" fields=vec![Field::text("username", "Usuario", ""), Field::text("display_name", "Nombre visible", ""), Field::password("password", "Contraseña (mínimo 12 caracteres)")]
            on_submit=Callback::new(move |values: forms::Values| context.authenticate("/auth/register", &RegisterRequest { username: values.text("username"), password: values.text("password"), display_name: values.text("display_name") })) /></section>
        </div>
    }
}

#[component]
fn Operations() -> impl IntoView {
    let context = expect_context::<Context>();
    view! {
        <section id="operations"><h2>"Operaciones"</h2><p class="muted">"Recibida no significa ejecutada. Si el resultado es desconocido, consulta la misma referencia; no crees otra operación. Las consultas automáticas se pausan tras 30 intentos."</p>
            <Form fields=vec![Field::text("operation", "Consultar una referencia existente", "")] submit="Consultar referencia"
                on_submit=Callback::new(move |values: forms::Values| {
                    let id: OperationId = values.parse("operation")?;
                    if context.operations.with_untracked(Vec::len) >= 32 { return Err("Limpia las operaciones terminadas primero".into()); }
                    context.operations.update(|items| { if !items.iter().any(|item| item.id == id) { items.push(Tracked { id, mutation: None, view: None, checks: 0, response_lost: false }); } });
                    context.poll(); Ok(())
                })/>
            <div class="actions"><Action label="Consultar operaciones" on_click=Callback::new(move |()| { context.operations.update(|items| { for item in items { item.checks = 0; } }); context.poll(); Ok(()) })/>
            <Action label="Limpiar terminadas" on_click=Callback::new(move |()| { context.operations.update(|items| items.retain(|item| item.view.as_ref().is_none_or(|view| operation_result(view) == OperationResult::Pending))); Ok(()) })/></div>
            <ul class="list">{move || context.operations.get().into_iter().map(|item| {
                let result = item.view.as_ref().map(operation_result);
                let label = match result { Some(OperationResult::Success) => "Completada correctamente".into(), Some(OperationResult::Failed(code)) => format!("No ejecutada: {code}"), _ => item.view.as_ref().map_or_else(|| "Esperando respuesta / sin confirmar".into(), |view| format!("{:?}", view.status)) };
                view! { <li><code>{item.id.to_string()}</code><p>{label}</p>
                    {item.response_lost.then(|| item.mutation.map(|mutation| view! { <Action label="Reenviar exactamente la misma operación" on_click=Callback::new(move |()| { context.send(item.id, mutation.clone()); Ok(()) })/> })).flatten()}
                </li> }
            }).collect_view()}</ul>
        </section>
    }
}

struct EventStream {
    source: web_sys::EventSource,
    callback: Closure<dyn FnMut(web_sys::MessageEvent)>,
    _error: Closure<dyn FnMut(web_sys::Event)>,
}

impl Drop for EventStream {
    fn drop(&mut self) {
        self.source.set_onmessage(None);
        self.source.set_onerror(None);
        self.source.close();
        // Removing the named listener avoids a callback into released Rust state.
        if self
            .source
            .remove_event_listener_with_callback(
                "notification",
                self.callback.as_ref().unchecked_ref(),
            )
            .is_err()
        {
            self.source.close();
        }
    }
}

#[component]
fn Notifications() -> impl IntoView {
    let context = expect_context::<Context>();
    let stream = StoredValue::new_local(None::<EventStream>);
    let connection = RwSignal::new("Cargando notificaciones…".to_owned());
    let generation = context.generation.get_untracked();
    spawn_local(async move {
        let result = context
            .request::<NotificationsPage>("GET", "/notifications?after=0", None, None)
            .await;
        if context.generation.get_untracked() != generation {
            return;
        }
        let mut cursor = 0;
        match result {
            Ok(page) => {
                cursor = page.next_cursor;
                context.notifications.update(|items| {
                    for item in page.notifications {
                        merge_notification(items, item);
                    }
                });
            }
            Err(error) => {
                context.error.set(Some(error));
            }
        }
        let options = web_sys::EventSourceInit::new();
        options.set_with_credentials(true);
        match web_sys::EventSource::new_with_event_source_init_dict(
            &context.endpoint(&format!("/events?after={cursor}")),
            &options,
        ) {
            Ok(source) => {
                let callback = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
                    move |event: web_sys::MessageEvent| {
                        if context.generation.get_untracked() != generation {
                            return;
                        }
                        if let Some(text) = event.data().as_string() {
                            match serde_json::from_str::<NotificationView>(&text) {
                                Ok(notification) => {
                                    context
                                        .notifications
                                        .update(|items| merge_notification(items, notification));
                                    connection
                                        .set("Conectado · recibir no marca como leído".into());
                                }
                                Err(_) => connection.set(
                                    "Evento incompatible; consulta las notificaciones por REST"
                                        .into(),
                                ),
                            }
                        }
                    },
                );
                source.set_onmessage(Some(callback.as_ref().unchecked_ref()));
                if source
                    .add_event_listener_with_callback(
                        "notification",
                        callback.as_ref().unchecked_ref(),
                    )
                    .is_err()
                {
                    source.close();
                    connection.set("No se pudo suscribir a los eventos".into());
                    return;
                }
                let error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                    connection.set(
                        "Conexión interrumpida; el navegador reintentará con el último cursor"
                            .into(),
                    );
                });
                source.set_onerror(Some(error.as_ref().unchecked_ref()));
                connection
                    .set("Suscrito a eventos; pendientes de lectura hasta tu confirmación".into());
                stream.set_value(Some(EventStream {
                    source,
                    callback,
                    _error: error,
                }));
            }
            Err(_) => connection
                .set("SSE no disponible. Las notificaciones siguen guardadas en la API.".into()),
        }
    });
    on_cleanup(move || {
        stream.update_value(|value| {
            value.take();
        });
    });
    view! {
        <section id="notifications"><h2>"Notificaciones"</h2><p class="muted" role="status">{move || connection.get()}</p>
        <p class="muted">"Se muestran las últimas 500 notificaciones recibidas en esta vista."</p>
        {move || context.notifications.with(Vec::is_empty).then(|| view! { <p class="empty">"No hay notificaciones en esta vista."</p> })}
        <ul class="list">{move || context.notifications.get().into_iter().rev().map(|notification| view! {
            <li><span class="badge">{format!("{:?}", notification.event.kind)}</span>" · "{if notification.read_at.is_some() { "Leída" } else { "Sin leer" }}
                <p class="muted">"Referencia: "<code>{notification.event.operation_id.to_string()}</code></p>
                {notification.read_at.is_none().then(|| view! { <Action label="Marcar como leída" on_click=Callback::new(move |()| context.ordinary::<Value>(format!("/notifications/{}/read", notification.notification_id), None, false))/> })}
            </li>
        }).collect_view()}</ul></section>
    }
}
