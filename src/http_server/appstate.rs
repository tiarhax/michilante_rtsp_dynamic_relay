use std::{collections::HashMap, sync::Arc};

use chrono::Utc;
use gst_rtsp_server::{RTSPMedia, RTSPMountPoints};
use serde::Serialize;
use tokio::sync::Mutex;
type MediaMap = Arc<Mutex<HashMap<String, Vec<glib::WeakRef<RTSPMedia>>>>>;

#[derive(Clone, Serialize)]
pub struct StreamInfo {
    pub id: String,
    pub name: String,
    pub url: String,
}

#[derive(Clone)]
pub enum ExpirationDate{
    Never,
    At(chrono::DateTime<Utc>)
}
#[derive(Clone)]
pub struct StreamInfoInternal {
    pub id: String,
    pub name: String,
    pub url: String,
    pub expiration_date: ExpirationDate,
    pub added_at: chrono::DateTime<Utc> 
}

#[derive(Clone)]
pub struct AppState {
    pub streams: Arc<Mutex<Vec<StreamInfoInternal>>>,
    pub mounts: Arc<Mutex<RTSPMountPoints>>,
    pub root_url: String,
    pub rtsp_root_url: String,
    pub media_map: MediaMap,
    pub stream_expiration_time_in_minutes: i64,
}

impl AppState {
    pub fn new(stream_expiration_time_in_minutes: i64, root_url: &str, rtsp_root_url: &str,  mounts: RTSPMountPoints) -> Self {
        let streams: Vec<StreamInfoInternal> = vec![];
        let streams = Mutex::new(streams);
        let streams = Arc::new(streams);

        let mounts: Mutex<RTSPMountPoints> = Mutex::new(mounts);
        let mounts = Arc::new(mounts);

        let media_map: HashMap<String, Vec<glib::WeakRef<RTSPMedia>>> = HashMap::new();
        let media_map = Mutex::new(media_map);
        let media_map = Arc::new(media_map);

        AppState {
            streams,
            mounts,
            root_url: root_url.to_owned(),
            media_map,
            stream_expiration_time_in_minutes,
            rtsp_root_url: rtsp_root_url.to_owned()
        }
    }
}
