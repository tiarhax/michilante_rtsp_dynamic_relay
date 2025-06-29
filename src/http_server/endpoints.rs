use crate::http_server::{
    appstate::ExpirationDate,
    error::{InternalError, UserInputError},
};
use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use gst_rtsp_server::prelude::{RTSPMediaExt, RTSPMediaFactoryExt, RTSPMountPointsExt};
use serde::{Deserialize, Serialize};
use tokio::task;
use tracing;
use ulid::Ulid;

use super::{
    appstate::{AppState, StreamInfo, StreamInfoInternal},
    error::AppError,
};

#[derive(Debug, Deserialize, Serialize)]
pub struct AddStreamOutput {
    id: String,
    name: String,
    url: String,
    expiration_date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AddStreamInput {
    pub name: String,
    pub source_url: String,
    pub down_scale: bool,
    pub expirable: bool,
}

#[derive(Debug, Deserialize)]
pub struct AddStreamToStateInput {
    pub id: String,
    pub name: String,
    pub source_url: String,
    pub down_scale: bool,
    pub expirable: bool,
}

#[derive(Debug, Deserialize)]
pub struct AddPermanentStreamInput {
    pub name: String,
    pub source_url: String,
    pub down_scale: bool,
}

pub async fn add_stream_to_state(
    state: AppState,
    req: AddStreamToStateInput,
) -> Result<AddStreamOutput, AppError> {
    let media_map_clone = state.media_map.clone();
    let handle = tokio::runtime::Handle::current();
    let factory = gst_rtsp_server::RTSPMediaFactory::new();

    let source_url = req.source_url.clone();

    let launch = if req.down_scale {
        format!(
            "rtspsrc location={} latency=0 ! rtph264depay ! h264parse ! avdec_h264 ! videoscale ! video/x-raw,width=640,height=320,format=I420 ! x264enc tune=zerolatency bitrate=500 speed-preset=ultrafast key-int-max=30 ! h264parse ! rtph264pay config-interval=1 name=pay0 pt=96",
            source_url
        )
    } else {
        format!(
            "rtspsrc location={} latency=50 protocols=tcp ! \
             rtph264depay ! h264parse config-interval=1 ! \
             rtph264pay name=pay0 pt=96",
            source_url
        )
    };

    factory.set_launch(&launch);

    factory.set_shared(true);
    let id = req.id;
    let path = format!("/{}", id.to_string());
    let path_clone = path.clone();
    factory.connect_media_configure(move |_, media| {
        let mut media_map = task::block_in_place(|| handle.block_on(media_map_clone.lock()));

        let v = media_map.entry(path_clone.clone()).or_insert_with(Vec::new);
        v.push(glib::object::ObjectExt::downgrade(&media));
    });

    let url = format!("{}{}", state.rtsp_root_url, id.to_string());
    state
        .mounts
        .lock()
        .await
        .add_factory(&path.to_string(), factory);
    let stream_info = StreamInfo {
        id: id.to_string(),
        name: req.name.clone(),
        url: url.clone(),
    };

    let expiration_date = if req.expirable {
        let current_time =
            chrono::Utc::now() + chrono::Duration::minutes(state.stream_expiration_time_in_minutes);
        ExpirationDate::At(current_time)
    } else {
        ExpirationDate::Never
    };

    let output = AddStreamOutput {
        id: id.to_string(),
        name: req.name,
        url,
        expiration_date: match expiration_date {
            ExpirationDate::Never => None,
            ExpirationDate::At(date_time) => Some(date_time.to_rfc3339()),
        },
    };
    let stream_info_internal = StreamInfoInternal {
        url: stream_info.url,
        id: stream_info.id,
        name: stream_info.name,
        added_at: chrono::Utc::now(),
        expiration_date,
    };
    state.streams.lock().await.push(stream_info_internal);

    Ok(output)
}

pub async fn add_stream(
    State(state): State<AppState>,
    Json(req): Json<AddStreamInput>,
) -> impl IntoResponse {
    let add_stream_internal_input = AddStreamToStateInput {
        id: Ulid::new().to_string(),
        name: req.name,
        source_url: req.source_url,
        down_scale: req.down_scale,
        expirable: req.expirable,
    };
    match add_stream_to_state(state, add_stream_internal_input).await {
        Ok(output) => Ok(Json(output)),
        Err(err) => Err(err.into_response()),
    }
}

pub async fn put_permanent_stream(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AddPermanentStreamInput>,
) -> Result<Json<AddStreamOutput>, AppError> {
    let add_stream_internal_input = AddStreamToStateInput {
        id: id.clone(),
        name: req.name,
        source_url: req.source_url,
        down_scale: req.down_scale,
        expirable: false,
    };
    remove_stream_by_id(&add_stream_internal_input.id, &state).await?;
    let result = add_stream_to_state(state, add_stream_internal_input).await?;
    Ok(Json(result))
}

async fn remove_stream_if_has_no_clients(id: &str, state: &AppState) -> Result<(), AppError> {
    let medias = state.media_map.lock().await;
    let mut streams_infos = state.streams.lock().await;

    let path = format!("/{}", id.to_string());
    let medias = medias.get(&path);
    if let Some(medias) = medias {
        let clients_count = medias.len();
        let found_clients = if clients_count > 0 {
            let mut found_clients_count = 0;
            for weak_media in medias {
                if let Some(media) = weak_media.upgrade() {
                    found_clients_count += media.n_streams()
                }

                if found_clients_count > 0 {
                    break;
                }
            }

            found_clients_count
        } else {
            0
        };

        let stream = streams_infos.iter().find(|s| s.id == id);
        let now = Utc::now();
        if stream.is_none() {
            tracing::warn!("stream not found while trying to remove it");
            return Ok(());
        }
        let stream = stream.unwrap();
        let minutes_until_expiration = (now - stream.added_at).num_minutes();
        if found_clients > 0 && minutes_until_expiration < state.stream_max_life_time_in_minutes {
            tracing::info!("{} clients found, ignoring", found_clients);
            return Ok(());
        } else {
            tracing::info!("removing factory {}", path);
            state.mounts.lock().await.remove_factory(&path);
            streams_infos.retain(|e| e.id != id);

            tracing::info!("{} clients found", medias.len());
            for weak_media in medias {
                if let Some(media) = weak_media.upgrade() {
                    media.unprepare().map_err(|err| {
                        AppError::InternalError(InternalError {
                            debug_message: format!("error while unpreparing media: {:?}", err),
                        })
                    })?;
                }
            }
        }
    }

    Ok(())
}

async fn remove_stream_by_id(id: &str, state: &AppState) -> Result<(), AppError> {
    let mut streams_infos = state.streams.lock().await;
    streams_infos.retain(|e| e.id != id);
    let path = format!("/{}", id.to_string());
    tracing::info!("removing factory {}", path);
    state.mounts.lock().await.remove_factory(&path);
    let medias = state.media_map.lock().await;
    let medias = medias.get(&path);
    if let Some(medias) = medias {
        tracing::info!("{} clients found", medias.len());
        for weak_media in medias {
            if let Some(media) = weak_media.upgrade() {
                media.unprepare().map_err(|err| {
                    AppError::InternalError(InternalError {
                        debug_message: format!("error while unpreparing media: {:?}", err),
                    })
                })?;
            }
        }
    }

    Ok(())
}

pub async fn remove_stream(
    Path(id): Path<String>,
    State(state): State<AppState>,
) -> Result<String, AppError> {
    remove_stream_by_id(&id, &state).await?;
    Ok("Stream Removed".to_string())
}

pub async fn remove_stale_streams(state: State<AppState>) -> Result<String, AppError> {
    let current_time = chrono::Utc::now();
    let stale_streams_ids = {
        let streams = state.streams.lock().await;

        streams
            .iter()
            .filter(|s| match s.expiration_date {
                ExpirationDate::Never => true,
                ExpirationDate::At(expiration_date) => expiration_date <= current_time,
            })
            .map(|s| s.id.clone())
            .collect::<Vec<String>>()
    };

    tracing::info!("{} stale streams found", stale_streams_ids.len());

    for stale_stream in &stale_streams_ids {
        if let Err(err) = remove_stream_if_has_no_clients(stale_stream, &state).await {
            let reason = match err {
                AppError::UserInputError(user_input_error) => user_input_error.message,
                AppError::InternalError(internal_error) => internal_error.debug_message,
            };
            tracing::error!("Failed to remove stale stream {}: {}", stale_stream, reason);
        }
    }

    Ok("Stale streams removed".to_owned())
}

#[derive(Debug, Serialize)]
pub struct StreamInfoListItem {
    pub id: String,
    pub name: String,
    pub url: String,
    pub added_at: String,
    pub expiration_date: Option<String>,
}
pub async fn list_streams(
    state: State<AppState>,
) -> Result<Json<Vec<StreamInfoListItem>>, AppError> {
    let mut result: Vec<StreamInfoListItem> = vec![];
    {
        let streams = state.streams.lock().await;
        for stream in streams.iter() {
            result.push(StreamInfoListItem {
                id: stream.id.clone(),
                name: stream.name.clone(),
                url: stream.url.clone(),
                added_at: stream.added_at.to_rfc3339(),
                expiration_date: match stream.expiration_date {
                    ExpirationDate::Never => None,
                    ExpirationDate::At(date_time) => Some(date_time.to_rfc3339()),
                },
            });
        }
    }

    Ok(Json(result))
}
