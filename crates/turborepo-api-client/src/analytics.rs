use async_trait::async_trait;
use reqwest::Method;
use serde::Serialize;

use crate::{retry, APIAuth, APIClient, Error};

#[derive(Serialize)]
enum CacheSource {
    Http,
    Fs,
}

#[derive(Serialize)]
enum CacheEvent {
    Hit,
    Miss,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsEvent {
    session_id: Option<String>,
    source: CacheSource,
    event: CacheEvent,
    hash: String,
    duration: i32,
}

impl AnalyticsEvent {
    pub fn set_session_id(&mut self, id: String) {
        self.session_id = Some(id);
    }
}

#[async_trait]
pub trait AnalyticsClient {
    async fn record_analytics(
        &self,
        api_auth: &APIAuth,
        events: Vec<AnalyticsEvent>,
    ) -> Result<(), Error>;
}

#[async_trait]
impl AnalyticsClient for APIClient {
    async fn record_analytics(
        &self,
        api_auth: &APIAuth,
        events: Vec<AnalyticsEvent>,
    ) -> Result<(), Error> {
        let request_builder = self
            .create_request_builder("/v8/artifacts/events", api_auth, Method::POST)
            .await?
            .json(&events);

        retry::make_retryable_request(request_builder)
            .await?
            .error_for_status()?;

        Ok(())
    }
}
