use axum::{extract::{Path, State}, response::IntoResponse, Json};
use gst_rtsp_server::prelude::{RTSPMediaExt, RTSPMediaFactoryExt, RTSPMountPointsExt};
use serde::{Deserialize, Serialize};
use tokio::task;
use tracing;
use crate::http_server::error::{InternalError, UserInputError};

use super::{appstate::{AppState, StreamInfo, StreamInfoInternal}, error::AppError};

#[derive(Debug, Deserialize,Serialize)]
pub struct AddStreamOutput {
    id: String,
    name: String,
    url: String
}

#[derive(Debug, Deserialize)]
pub struct AddStreamInput {
    pub name: String,
    pub source_url: String,
    pub down_scale: bool
}

pub async fn add_stream_to_state(state: AppState, req: AddStreamInput) -> Result<AddStreamOutput, AppError> {
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
    let id =  ulid::Ulid::new();
    let path = format!("/{}", id.to_string());
    let path_clone = path.clone();
    factory.connect_media_configure(move |_, media| {
        let mut media_map = task::block_in_place(|| {
            handle.block_on(media_map_clone.lock())
        });
        
        let v = media_map.entry(path_clone.clone()).or_insert_with(Vec::new);
        v.push(glib::object::ObjectExt::downgrade(&media));
    });


    let url = format!("{}{}", state.rtsp_root_url, id.to_string());
    state.mounts.lock().await.add_factory(&path.to_string(), factory);
    let stream_info = StreamInfo {
        id: id.to_string(),
        name: req.name.clone(),
        url: url.clone()
    };
    let output = AddStreamOutput {
        id: id.to_string(),
        name: req.name,
        url
    };

    let stream_info_internal = StreamInfoInternal {
        url: stream_info.url,
        id: stream_info.id,
        name: stream_info.name,
        added_at: chrono::Utc::now(),
    };
    state.streams.lock().await.push(stream_info_internal);

    Ok(output)
}

pub async fn add_stream(
    State(state): State<AppState>,
    Json(req): Json<AddStreamInput>
) -> impl IntoResponse {


    
    match add_stream_to_state(state, req).await {
        Ok(output) => Ok(Json(output)),
        Err(err) => Err(err.into_response()),
    }
}

async fn remove_stream_by_id(id: &str, state: &State<AppState>) -> Result<(), AppError>  {
    let mut streams_infos = state.streams.lock().await;
    streams_infos.retain(|e|  e.id != id);
    let path = format!("/{}", id.to_string());
    tracing::info!("removing factory {}", path);
    state.mounts.lock().await.remove_factory(&path);
    let medias = state.media_map.lock().await;
    let medias = medias.get(&path);
    if let Some(medias) = medias {
        tracing::info!("{} clients found", medias.len());
        for weak_media in  medias {
            
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

pub async  fn remove_stream(
    Path(id): Path<String>,
    State(state): State<AppState> 
) -> Result<String, AppError> {

    remove_stream_by_id(&id, &State(state)).await?;
    Ok("Stream Removed".to_string())
}

pub async fn remove_stale_streams(state: State<AppState>) -> Result<String, AppError> {
    let current_time = chrono::Utc::now();
    let stale_streams_ids = {
        let streams = state.streams.lock().await;
        
        streams.iter()
            .filter(|s| { 
                (current_time - s.added_at).num_minutes() >= state.stream_expiration_time_in_minutes
            })
            .map(|s| { s.id.clone()})
            .collect::<Vec<String>>()
    };

    tracing::info!("{} stale streams found", stale_streams_ids.len());

    for stale_stream in &stale_streams_ids {
        if let Err(err) = remove_stream_by_id(stale_stream, &state).await {
            let reason = match err {
                AppError::UserInputError(user_input_error) => user_input_error.message,
                AppError::InternalError(internal_error) => internal_error.debug_message
            };
            tracing::error!("Failed to remove stale stream {}: {}", stale_stream, reason );
        }
    }

    Ok("Stale streams removed".to_owned())
}


