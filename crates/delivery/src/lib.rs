//! Durable-outbox-friendly notification adapters.
//!
//! A provider accepting an HTTP request is not the same as a human receiving
//! a message. [`DeliveryStage::Accepted`] therefore remains distinct from
//! [`DeliveryStage::Delivered`]; the WhatsApp sender only reports the former.

mod http;
mod secret;
mod types;
mod whatsapp;

use async_trait::async_trait;

pub use http::{
    HttpError, HttpExecutor, HttpRequest, HttpResponse, MAX_RESPONSE_BODY_BYTES, ReqwestExecutor,
};
pub use secret::{
    EnvironmentSecretResolver, SecretError, SecretRef, SecretResolver, SecretValue, TokenSource,
};
pub use types::{
    DeliveryFailure, DeliveryFailureKind, DeliveryReason, DeliveryReceipt, DeliveryRequest,
    DeliveryStage, Destination, EtaRange, MessagingConsent, Notice, NoticeState, RequestError,
};
pub use whatsapp::{WhatsAppCloud, WhatsAppConfig};

/// One configured delivery provider.
#[async_trait]
pub trait DeliveryAdapter: Send + Sync {
    /// Stable adapter identifier for outbox diagnostics.
    fn adapter_id(&self) -> &'static str;

    /// Attempts one outbox request exactly once.
    async fn deliver(&self, request: &DeliveryRequest) -> Result<DeliveryReceipt, DeliveryFailure>;
}
