use anyhow::Context;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::PgPool;
use tracing::warn;

use crate::auth::{create_token, TokenClaims};
use crate::error::{ClientError, Result};
use crate::model::{
    AuthenticateResponse, CurrentMap, CurrentMaps, CurrentServer, CurrentServers,
    GetCurrentMapsQuery, ReportPlayedMapBody,
};
use crate::service::api_client::ApiClient;
use crate::service::openid_client::{OpenIDClient, OpenIDParams};
use crate::util::validation::{ValidForm, ValidJson, ValidQuery};
use crate::{AppContext, AppId, ServerSecret};

pub fn router() -> Router<AppContext> {
    Router::new()
        .route("/api/played-map", post(report_played_map))
        .route("/api/current-maps", get(get_current_maps))
        .route("/api/current-servers", get(get_current_servers))
        .route("/api/authenticate", post(authenticate))
}

async fn report_played_map(
    State(pool): State<PgPool>,
    claims: TokenClaims,
    ValidJson(body): ValidJson<ReportPlayedMapBody>,
) -> Result<StatusCode> {
    let row = sqlx::query_file!(
        "queries/insert_played_map.sql",
        claims.sub,
        body.server,
        body.map,
        body.mode,
        body.bottom_tier,
        body.top_tier
    )
    .fetch_optional(&pool)
    .await
    .with_context(|| format!("Failed to insert played map: {:?}", body))?;

    if row.is_none() {
        warn!(
            "Unrecognized server, map, or mode: {}, {}, {}",
            body.server, body.map, body.mode
        )
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn get_current_maps(
    State(pool): State<PgPool>,
    ValidQuery(query): ValidQuery<GetCurrentMapsQuery>,
) -> Result<Json<CurrentMaps>> {
    let rows = sqlx::query_file_as!(
        CurrentMap,
        "queries/select_current_maps.sql",
        query.server,
        query.min_tier,
        query.max_tier
    )
    .fetch_all(&pool)
    .await
    .with_context(|| format!("Failed to select current maps: {:?}", query))?;

    Ok(Json(CurrentMaps::from_rows(rows)))
}

async fn get_current_servers(State(pool): State<PgPool>) -> Result<Json<CurrentServers>> {
    let rows = sqlx::query_file_as!(CurrentServer, "queries/select_current_servers.sql")
        .fetch_all(&pool)
        .await
        .context("Failed to select current servers")?;

    Ok(Json(CurrentServers::from_rows(rows)))
}

async fn authenticate(
    State(app_id): State<AppId>,
    State(server_secret): State<ServerSecret>,
    ValidForm(params): ValidForm<OpenIDParams>,
) -> Result<Json<AuthenticateResponse>> {
    let openid_client = OpenIDClient::new();
    let api_client = ApiClient::new(params.endpoint.region(), app_id);

    let account = openid_client
        .verify_id(params)
        .await
        .context("Failed to verify account with OpenID provider")?
        .ok_or(ClientError::OpenIDRejected)?;

    let account_info = api_client
        .get_public_account_info(account.account_id)
        .await
        .with_context(|| format!("Failed to fetch number of battles: {:?}", account))?;

    if account_info.statistics.all.battles < 200 {
        Err(ClientError::NotEnoughBattles)?;
    }

    let token = create_token(account.account_id, &server_secret)?;
    Ok(Json(AuthenticateResponse { token }))
}
