//! A Resend API double, shared by the transport and decoder tests.
//!
//! Hand-written rather than an HTTP recorder: the interesting cases here are the error arms, and
//! what each test needs is to name the [`ResendApiError`] the provider produced and then assert on
//! the decision it forced.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::adapters::resend_api::client::{
    ReceivedEmail, ResendApi, ResendApiError, ResendApiSendRequest, ResendApiSendResponse,
};

/// Answers each call with a scripted result and records what it was asked.
#[derive(Default)]
pub struct FakeResendApi {
    pub send_result: Mutex<Option<Result<ResendApiSendResponse, ResendApiError>>>,
    pub retrieve_result: Mutex<Option<Result<ReceivedEmail, ResendApiError>>>,
    pub raw_result: Mutex<Option<Result<Vec<u8>, ResendApiError>>>,
    pub sent: Mutex<Vec<ResendApiSendRequest>>,
    pub retrieved: Mutex<Vec<String>>,
    pub downloaded: Mutex<Vec<String>>,
}

impl FakeResendApi {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sending(self, result: Result<ResendApiSendResponse, ResendApiError>) -> Self {
        *self.send_result.lock().unwrap() = Some(result);
        self
    }

    pub fn accepting(self, id: &str) -> Self {
        self.sending(Ok(ResendApiSendResponse { id: id.to_string() }))
    }

    pub fn retrieving(self, result: Result<ReceivedEmail, ResendApiError>) -> Self {
        *self.retrieve_result.lock().unwrap() = Some(result);
        self
    }

    pub fn with_raw(self, result: Result<Vec<u8>, ResendApiError>) -> Self {
        *self.raw_result.lock().unwrap() = Some(result);
        self
    }

    /// The one request this double was asked to send, for the assertions about what went on the
    /// wire. Panics rather than returning an `Option`: a test reaching for it expects a send.
    pub fn only_send(&self) -> ResendApiSendRequest {
        let sent = self.sent.lock().unwrap();
        assert_eq!(sent.len(), 1, "expected exactly one send");
        sent[0].clone()
    }
}

/// A double that is asked for something it was not scripted for is a test that is not testing what
/// it thinks; every arm says so rather than answering a default.
fn scripted<T: Clone>(
    slot: &Mutex<Option<Result<T, ResendApiError>>>,
    what: &str,
) -> Result<T, ResendApiError> {
    slot.lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| panic!("this Resend double was not scripted to {what}"))
}

#[async_trait]
impl ResendApi for FakeResendApi {
    async fn send_email(
        &self,
        request: &ResendApiSendRequest,
    ) -> Result<ResendApiSendResponse, ResendApiError> {
        self.sent.lock().unwrap().push(request.clone());
        scripted(&self.send_result, "send")
    }

    async fn retrieve_received(&self, email_id: &str) -> Result<ReceivedEmail, ResendApiError> {
        self.retrieved.lock().unwrap().push(email_id.to_string());
        scripted(&self.retrieve_result, "retrieve a received email")
    }

    async fn download_raw(&self, url: &str) -> Result<Vec<u8>, ResendApiError> {
        self.downloaded.lock().unwrap().push(url.to_string());
        scripted(&self.raw_result, "download raw mail")
    }
}
