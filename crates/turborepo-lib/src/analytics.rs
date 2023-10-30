use std::time::Duration;

use thiserror::Error;
use tokio::{
    select,
    sync::{mpsc, oneshot},
    task::{JoinError, JoinHandle},
};
use tracing::debug;
use turborepo_api_client::{
    analytics::{AnalyticsClient, AnalyticsEvent},
    APIAuth,
};
use uuid::Uuid;

const BUFFER_THRESHOLD: usize = 10;

static EVENT_TIMEOUT: Duration = Duration::from_millis(200);
static NO_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
static REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CHANNEL_SIZE: usize = 100;

#[derive(Debug, Error)]
enum Error {
    #[error("Failed to send analytics event")]
    SendError(#[from] mpsc::error::SendError<AnalyticsEvent>),
    #[error("Failed to record analytics")]
    Join(#[from] JoinError),
}

struct AnalyticsRecorder {
    tx: mpsc::Sender<AnalyticsEvent>,
    cancel_tx: oneshot::Sender<()>,
    handle: JoinHandle<()>,
}

impl AnalyticsRecorder {
    pub fn new(api_auth: APIAuth, client: impl AnalyticsClient + Send + Sync) -> Self {
        let (tx, rx) = mpsc::channel(CHANNEL_SIZE);
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let session_id = Uuid::new_v4();
        let worker = Worker {
            rx,
            buffer: Vec::new(),
            session_id,
            api_auth,
            cancel_rx,
            client,
        };
        let handle = worker.start();

        Self {
            tx,
            cancel_tx,
            handle,
        }
    }

    async fn close(self) -> Result<(), Error> {
        // If we fail to send, the worker has already been canceled.
        let _ = self.cancel_tx.send(());
        self.handle.await?;

        Ok(())
    }

    pub async fn close_with_timeout(self) {
        let _ = tokio::time::timeout(EVENT_TIMEOUT, self.close()).await;
    }

    pub async fn log_event(&self, event: AnalyticsEvent) -> Result<(), Error> {
        self.tx.send(event).await?;

        Ok(())
    }
}

struct Worker<R> {
    rx: mpsc::Receiver<AnalyticsEvent>,
    buffer: Vec<AnalyticsEvent>,
    session_id: Uuid,
    api_auth: APIAuth,
    cancel_rx: oneshot::Receiver<()>,
    client: R,
}

impl<R: AnalyticsClient + Send + Sync> Worker<R> {
    pub fn start(mut self) -> JoinHandle<()> {
        tokio::spawn(async {
            let mut timeout = tokio::time::sleep(NO_TIMEOUT);
            loop {
                select! {
                    event = self.rx.recv() => {
                        if let Some(event) = event {
                            self.buffer.push(event);
                        }
                        if self.buffer.len() == BUFFER_THRESHOLD {
                            self.flush();
                            timeout = tokio::time::sleep(NO_TIMEOUT);
                        } else {
                            timeout = tokio::time::sleep(REQUEST_TIMEOUT);
                        }
                    }
                    _ = timeout => {
                        self.flush();
                        timeout = tokio::time::sleep(NO_TIMEOUT);
                    }
                    _ = self.cancel_rx => {
                        self.flush();
                        return;
                    }
                }
            }
        })
    }
    pub fn flush(&mut self) {
        if !self.buffer.is_empty() {
            let events = std::mem::take(&mut self.buffer);
            self.send_events(events);
        }
    }

    fn send_events(&self, mut events: Vec<AnalyticsEvent>) {
        let session_id = self.session_id.clone();
        tokio::spawn(async {
            add_session_id(session_id, &mut events);
            // We don't log an error for a timeout because
            // that's what the Go code does.
            if let Err(err) = tokio::time::timeout(
                REQUEST_TIMEOUT,
                self.client.record_analytics(&self.api_auth, events),
            )
            .await?
            {
                debug!("failed to record cache usage analytics. error: {}", err)
            }

            Ok(())
        });
    }
}

fn add_session_id(id: Uuid, events: &mut Vec<AnalyticsEvent>) {
    for event in events {
        event.set_session_id(id.to_string());
    }
}
