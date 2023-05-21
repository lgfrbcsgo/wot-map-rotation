use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::PgPool;
use tracing::warn;

use crate::api_client::ApiClient;
use crate::auth::create_token;
use crate::error::{ApiError, Result};
use crate::model::{
    AuthenticatePayload, AuthenticateResponse, CreatePlayedMapPayload, CurrentMap, CurrentServer,
    GetCurrentMapsQuery, GetCurrentMapsResponse, GetCurrentServersResponse,
};
use crate::util::{ValidJson, ValidQuery};
use crate::{AppContext, AppId, ServerSecret};

pub fn router() -> Router<AppContext> {
    Router::new()
        .route("/api/played-map", post(create_played_map))
        .route("/api/current-maps", get(get_current_maps))
        .route("/api/current-servers", get(get_current_servers))
        .route("/api/authenticate", get(authenticate))
}

async fn create_played_map(
    State(pool): State<PgPool>,
    ValidJson(payload): ValidJson<CreatePlayedMapPayload>,
) -> Result<StatusCode> {
    let row = sqlx::query_file!(
        "queries/insert_played_map.sql",
        "",
        payload.server,
        payload.map,
        payload.mode,
        payload.bottom_tier,
        payload.top_tier
    )
    .fetch_optional(&pool)
    .await?;

    match row {
        Some(_) => Ok(StatusCode::NO_CONTENT),
        None => {
            warn!(
                "Unknown server `{}`, map `{}`, or mode `{}`.",
                payload.server, payload.map, payload.mode
            );
            Err(ApiError::Validation("Unknown server, map, or mode."))
        }
    }
}

async fn get_current_maps(
    State(pool): State<PgPool>,
    ValidQuery(query): ValidQuery<GetCurrentMapsQuery>,
) -> Result<Json<GetCurrentMapsResponse>> {
    let rows = sqlx::query_file_as!(
        CurrentMap,
        "queries/select_current_maps.sql",
        query.server,
        query.min_tier,
        query.max_tier
    )
    .fetch_all(&pool)
    .await?;

    Ok(Json(GetCurrentMapsResponse::from_rows(rows)))
}

async fn get_current_servers(
    State(pool): State<PgPool>,
) -> Result<Json<GetCurrentServersResponse>> {
    let rows = sqlx::query_file_as!(CurrentServer, "queries/select_current_servers.sql")
        .fetch_all(&pool)
        .await?;

    Ok(Json(GetCurrentServersResponse::from_rows(rows)))
}

async fn authenticate(
    State(app_id): State<AppId>,
    State(server_secret): State<ServerSecret>,
    ValidJson(payload): ValidJson<AuthenticatePayload>,
) -> Result<Json<AuthenticateResponse>> {
    let api_client = ApiClient::new(payload.region, app_id);

    let token_details = api_client.extend_access_token(payload.access_token).await?;

    let account_info = api_client
        .get_public_account_info(token_details.account_id)
        .await?;

    if account_info.statistics.all.battles >= 200 {
        let token = create_token(token_details.account_id, server_secret)?;
        Ok(Json(AuthenticateResponse { token }))
    } else {
        Err(ApiError::Validation("Not enough battles."))
    }
}
