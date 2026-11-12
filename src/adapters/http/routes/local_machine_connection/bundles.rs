use super::*;
use crate::application::local_agents::harness::NativeBundleRequest;
use crate::domain::local_agents::{ConnectionGeneration, CredentialGeneration, OperationId};
use axum::extract::Path;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH};

pub(super) fn credential(headers: &HeaderMap) -> AppResult<MachineCredential> {
    let secret = one_header(headers, "authorization")?
        .strip_prefix("Bearer ")
        .filter(|value| value.len() == 64)
        .ok_or(AppError::InvalidCredentials)?;
    MachineCredential::try_from(secret.to_owned()).map_err(|_| AppError::InvalidCredentials)
}

pub(super) async fn get(
    State(service): State<Arc<LocalMachineConnections>>,
    Path(operation): Path<OperationId>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let request = request(&headers, operation)?;
    let _permit = service
        .bundle_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::ServiceUnavailable("Native bundle capacity exhausted".into()))?;
    let bundle = tokio::select! { biased;
        _ = service.shutdown.cancelled() => return Ok(StatusCode::SERVICE_UNAVAILABLE.into_response()),
        result = timeout(Duration::from_secs(IO_SECONDS), service.store.native_bundle(&request)) =>
            result.map_err(|_| AppError::ServiceUnavailable("Native bundle read timed out".into()))??,
    };
    response(bundle, &headers)
}

pub(super) fn request(
    headers: &HeaderMap,
    operation: OperationId,
) -> AppResult<NativeBundleRequest> {
    if one_header(headers, "x-openbot-protocol-version")? != PROTOCOL_VERSION.to_string() {
        return Err(AppError::BadRequest(
            "unsupported local-machine protocol".into(),
        ));
    }
    Ok(NativeBundleRequest {
        verifier: credential(headers)?.verifier(),
        operation,
        identity: ConnectionIdentity {
            installation_id: InstallationId::try_from(
                one_header(headers, "x-openbot-installation-id")?.to_owned(),
            )
            .map_err(|_| AppError::InvalidCredentials)?,
            credential_generation: CredentialGeneration::try_from(number(
                headers,
                "x-openbot-credential-generation",
            )?)
            .map_err(|_| AppError::InvalidCredentials)?,
            connection_generation: ConnectionGeneration::try_from(number(
                headers,
                "x-openbot-connection-generation",
            )?)
            .map_err(|_| AppError::InvalidCredentials)?,
        },
    })
}

fn response(
    bundle: crate::application::local_agents::harness::NativeBundleResponse,
    headers: &HeaderMap,
) -> AppResult<Response> {
    let etag = format!("\"{}\"", bundle.digest.as_str());
    // Every conditional read above checked current authority, including cached clients.
    let matched = if headers.contains_key(IF_NONE_MATCH) {
        one_header(headers, IF_NONE_MATCH.as_str())? == etag
    } else {
        false
    };
    let mut response = if matched {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(CONTENT_TYPE, "application/json")], bundle.bytes).into_response()
    };
    response.headers_mut().insert(
        ETAG,
        etag.parse()
            .map_err(|_| AppError::Internal("Invalid native bundle ETag".into()))?,
    );
    response
        .headers_mut()
        .insert(CACHE_CONTROL, "private, no-store".parse().unwrap());
    Ok(response)
}

fn number(headers: &HeaderMap, name: &str) -> AppResult<u64> {
    let value = one_header(headers, name)?;
    if value.len() > 16 {
        return Err(AppError::InvalidCredentials);
    }
    value
        .parse::<u64>()
        .ok()
        .filter(|value| *value <= MAX_SAFE_INTEGER)
        .ok_or(AppError::InvalidCredentials)
}
