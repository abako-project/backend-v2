use crate::{
    auth::{self, Session},
    state::{App, Error, now},
};
use axum::response::sse::{Event, KeepAlive, Sse};
use generated_contracts::{
    AccountId32, NotificationView, NotificationsPage, ProviderEvents, ProviderInfo, UnixSeconds,
};
use sqlx::{Row, postgres::PgRow};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    sync::{OwnedSemaphorePermit, mpsc, watch},
    task::JoinHandle,
};
use tokio_stream::{Stream, wrappers::ReceiverStream};

pub(crate) async fn ingest(app: &App) -> Result<(), Error> {
    let info: ProviderInfo = app.get(&app.config.provider_url, "/internal/info").await?;
    let stored = sqlx::query("SELECT cursor FROM event_cursors WHERE provider_instance_id = $1")
        .bind(info.provider_instance_id.to_string())
        .fetch_optional(&app.db)
        .await?;
    let after = stored
        .map(|row| row.try_get::<i64, _>("cursor"))
        .transpose()?
        .unwrap_or(0);
    let page: ProviderEvents = app
        .get(
            &app.config.provider_url,
            &format!("/internal/events?after={after}&limit=100"),
        )
        .await?;
    if page.provider_instance_id != info.provider_instance_id {
        return Err(Error::Dependency);
    }
    persist(app, after, page).await
}
pub(crate) async fn persist(app: &App, after: i64, page: ProviderEvents) -> Result<(), Error> {
    let next = i64::try_from(page.next_cursor).map_err(|_| Error::Dependency)?;
    let mut previous = u64::try_from(after).map_err(|_| Error::Internal)?;
    if page.events.len() > 100 {
        return Err(Error::Dependency);
    }
    for event in &page.events {
        if event.provider_instance_id != page.provider_instance_id || event.cursor <= previous {
            return Err(Error::Dependency);
        }
        previous = event.cursor;
    }
    // A page cannot advance past events that were not durably received.
    if page.next_cursor != previous || next < after {
        return Err(Error::Dependency);
    }
    let mut tx = app.db.begin().await?;
    // Commit notification IDs in ingestion order so an SSE cursor cannot skip a late commit.
    sqlx::query("SELECT pg_advisory_xact_lock(621008)")
        .execute(&mut *tx)
        .await?;
    for event in page.events {
        let json = serde_json::to_string(&event)?;
        for recipient in &event.recipients {
            sqlx::query("INSERT INTO notifications(account_id, provider_instance_id, provider_cursor, event_json) VALUES ($1, $2, $3, $4) ON CONFLICT(account_id, provider_instance_id, provider_cursor) DO NOTHING")
                .bind(recipient.to_string()).bind(event.provider_instance_id.to_string()).bind(i64::try_from(event.cursor).map_err(|_| Error::Dependency)?)
                .bind(&json).execute(&mut *tx).await?;
        }
    }
    sqlx::query("INSERT INTO event_cursors(provider_instance_id, cursor) VALUES ($1, $2) ON CONFLICT(provider_instance_id) DO UPDATE SET cursor = GREATEST(event_cursors.cursor, excluded.cursor)")
        .bind(page.provider_instance_id.to_string()).bind(next).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
fn view(row: &PgRow) -> Result<NotificationView, Error> {
    Ok(NotificationView {
        notification_id: u64::try_from(row.try_get::<i64, _>("notification_id")?)
            .map_err(|_| Error::Internal)?,
        event: serde_json::from_str(row.try_get("event_json")?)?,
        read_at: row
            .try_get::<Option<i64>, _>("read_at")?
            .map(|value| {
                u64::try_from(value)
                    .map(UnixSeconds::new)
                    .map_err(|_| Error::Internal)
            })
            .transpose()?,
    })
}
pub(crate) async fn page(
    app: &App,
    account: AccountId32,
    after: u64,
) -> Result<NotificationsPage, Error> {
    let rows = sqlx::query("SELECT notification_id, event_json, read_at FROM notifications WHERE account_id = $1 AND notification_id > $2 ORDER BY notification_id LIMIT 100")
        .bind(account.to_string()).bind(i64::try_from(after).map_err(|_| Error::Invalid)?).fetch_all(&app.db).await?;
    let notifications = rows.iter().map(view).collect::<Result<Vec<_>, _>>()?;
    let next_cursor = notifications
        .last()
        .map_or(after, |notification| notification.notification_id);
    Ok(NotificationsPage {
        notifications,
        next_cursor,
    })
}
pub(crate) async fn mark_read(
    app: &App,
    account: AccountId32,
    id: u64,
) -> Result<NotificationView, Error> {
    let mut tx = app.db.begin().await?;
    let row = sqlx::query("UPDATE notifications SET read_at = coalesce(read_at, $1) WHERE notification_id = $2 AND account_id = $3 RETURNING notification_id, event_json, read_at")
        .bind(now()?).bind(i64::try_from(id).map_err(|_| Error::Invalid)?).bind(account.to_string())
        .fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
    let notification = view(&row)?;
    tx.commit().await?;
    Ok(notification)
}
pub(crate) async fn run(app: Arc<App>, mut stop: watch::Receiver<bool>) -> Result<(), Error> {
    let mut delay = 1;
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        match ingest(&app).await {
            Ok(()) => delay = 1,
            Err(Error::Dependency | Error::NotFound) => {
                app.audit(None, None, "event_ingestion", "dependency_unavailable")
                    .await?;
                delay = (delay * 2).min(30);
            }
            Err(error) => return Err(error),
        }
        tokio::select! {
            changed = stop.changed() => { if changed.is_err() { return Ok(()); } }
            () = tokio::time::sleep(Duration::from_secs(delay)) => {}
        }
    }
}

pub(crate) struct NotificationStream {
    receiver: ReceiverStream<Result<Event, Error>>,
    worker: JoinHandle<()>,
    finished: bool,
    _permit: OwnedSemaphorePermit,
}
impl Stream for NotificationStream {
    type Item = Result<Event, Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.finished {
            return Poll::Ready(None);
        }
        match Pin::new(&mut self.receiver).poll_next(cx) {
            Poll::Ready(None) => match std::future::Future::poll(Pin::new(&mut self.worker), cx) {
                Poll::Ready(Ok(())) => {
                    self.finished = true;
                    Poll::Ready(None)
                }
                Poll::Ready(Err(error)) => {
                    self.finished = true;
                    Poll::Ready(Some(Err(error.into())))
                }
                Poll::Pending => Poll::Pending,
            },
            result => result,
        }
    }
}
impl Drop for NotificationStream {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

pub(crate) fn stream(
    app: Arc<App>,
    session: Session,
    after: u64,
) -> Result<impl axum::response::IntoResponse, Error> {
    if i64::try_from(after).is_err() {
        return Err(Error::Invalid);
    }
    let permit = app
        .stream_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Capacity)?;
    let (sender, receiver) = mpsc::channel(32);
    let worker = tokio::spawn(async move {
        if let Err(error) = deliver(&app, &session, after, &sender).await {
            // The receiving stream observes errors; a closed client owns cancellation.
            let _closed =
                tokio::time::timeout(Duration::from_secs(5), sender.send(Err(error))).await;
        }
    });
    Ok(Sse::new(NotificationStream {
        receiver: ReceiverStream::new(receiver),
        worker,
        finished: false,
        _permit: permit,
    })
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
async fn deliver(
    app: &App,
    session: &Session,
    mut after: u64,
    sender: &mpsc::Sender<Result<Event, Error>>,
) -> Result<(), Error> {
    loop {
        if sender.is_closed() {
            return Ok(());
        }
        // Revalidate expiry and revocation for long-lived streams, not just on connect.
        auth::authenticate_hash(app, session.token_hash).await?;
        let page = page(app, session.view.account_id, after).await?;
        for notification in page.notifications {
            let event = Event::default()
                .event("notification")
                .id(notification.notification_id.to_string())
                .json_data(&notification)
                .map_err(|_| Error::Internal)?;
            match tokio::time::timeout(Duration::from_secs(5), sender.send(Ok(event))).await {
                Ok(Ok(())) => after = notification.notification_id,
                _ => return Ok(()),
            }
        }
        tokio::select! {
            () = sender.closed() => return Ok(()),
            () = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
    }
}
