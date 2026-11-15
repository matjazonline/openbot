//! Authenticate the native operation before reading or decoding model-controlled JSON.
use super::*;
use crate::application::local_agents::mcp::{
    MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, NativeMcpBody, NativeMcpRequest,
};
use crate::domain::local_agents::OperationId;
use axum::extract::{Path, Request};

pub(super) async fn post(
    State(service): State<Arc<LocalMachineConnections>>,
    Path(operation): Path<OperationId>,
    request: Request,
) -> AppResult<Response> {
    let authentication = bundles::request(request.headers(), operation)?;
    let _permit = service
        .bundle_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError::ServiceUnavailable("Native MCP request capacity exhausted".into())
        })?;
    tokio::select! { biased;
        _=service.shutdown.cancelled()=>Err(AppError::ServiceUnavailable("Native MCP server is stopping".into())),
        result=timeout(Duration::from_secs(15),async {
            // This read authenticates the credential and complete current source authority;
            // the write repeats those checks under its own transaction after parsing.
            service.store.authorize_native_mcp(&authentication).await?;
            let bytes=axum::body::to_bytes(request.into_body(),MAX_REQUEST_BYTES).await
                .map_err(|_|AppError::BadRequest("Native MCP request exceeds its body bound".into()))?;
            let body:NativeMcpBody=serde_json::from_slice(&bytes)
                .map_err(|_|AppError::BadRequest("Invalid native MCP request".into()))?;
            let result=service.store.native_mcp(&NativeMcpRequest {
                verifier:authentication.verifier,identity:authentication.identity,operation,body,
            }).await?;
            let bytes=serde_json::to_vec(&result).map_err(|_|AppError::Internal("Cannot encode native MCP status".into()))?;
            if bytes.len()>MAX_RESPONSE_BYTES {return Err(AppError::BadRequest("Native MCP result exceeds 2 MiB".into()));}
            Ok(([(axum::http::header::CONTENT_TYPE,"application/json"),
                (axum::http::header::CACHE_CONTROL,"private, no-store")],bytes).into_response())
        })=>result.map_err(|_|AppError::ServiceUnavailable("Native MCP request timed out".into()))?,
    }
}
