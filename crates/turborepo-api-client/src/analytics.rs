use std::collections::HashMap;

use reqwest::Method;

use crate::{retry, APIAuth, APIClient, Error};

struct AnalyticsEvent {}

impl APIClient {
    async fn record_analytics(
        &self,
        api_auth: &APIAuth,
        events: HashMap<String, AnalyticsEvent>,
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
