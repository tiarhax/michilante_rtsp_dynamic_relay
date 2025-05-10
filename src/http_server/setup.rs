use std::sync::Arc;

use axum::{
    routing::{delete, post},
    Router,
};
use dotenvy::dotenv;
use gst_rtsp_server::RTSPMountPoints;
use tokio::sync::Mutex;

use crate::{http_server::{
    appstate::{AppState, StreamInfoInternal},
    endpoints::{add_stream, remove_stale_streams, remove_stream},
}, rtsp_server::{load_rtsp_server_config, start_server}};

#[derive(Debug, Clone)]
struct ReadConfigErr {
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct StartupServerError {
    pub reason: String,
}

struct ServerConfig {
    pub http_port: i32,
    pub http_host: String,
    pub stream_expiration_time_in_minutes: i64,
    pub root_url: String,
}
fn read_config() -> Result<ServerConfig, ReadConfigErr> {


    let http_port: i32 = std::env::var("HTTP_PORT")
        .map_err(|_| ReadConfigErr {
            reason: "HTTP_PORT not set or invalid".to_string(),
        })?
        .parse()
        .map_err(|_| ReadConfigErr {
            reason: "HTTP_PORT must be a valid integer".to_string(),
        })?;

    let http_host: String = std::env::var("HTTP_HOST").map_err(|_| ReadConfigErr {
        reason: "HTTP_HOST not set".to_string(),
    })?;

    let stream_expiration_time_in_minutes: i64 = std::env::var("STREAM_EXPIRATION_TIME_IN_MINUTES")
        .map_err(|_| ReadConfigErr {
            reason: "STREAM_EXPIRATION_TIME_IN_MINUTES not set or invalid".to_string(),
        })?
        .parse()
        .map_err(|_| ReadConfigErr {
            reason: "STREAM_EXPIRATION_TIME_IN_MINUTES must be a valid integer".to_string(),
        })?;

    let root_url: String = std::env::var("ROOT_URL").map_err(|_| ReadConfigErr {
        reason: "ROOT_URL not set".to_string(),
    })?;
    Ok(ServerConfig {
        http_port,
        http_host,
        stream_expiration_time_in_minutes,
        root_url,
    })
}



pub async fn setup_and_run() -> Result<(), StartupServerError> {
    tracing_subscriber::fmt::init();
    if let Err(_) = dotenvy::dotenv() {
        tracing::info!(".env file could not be loaded, expecting env variables to be already present");
    }
    let server_config = read_config().map_err(|err| StartupServerError {
        reason: format!("{:?}", err),
    })?;

    let rtsp_server_config = load_rtsp_server_config().map_err(|err| StartupServerError {
        reason: format!("Failed to load RTSP server config: {:?}", err),
    })?;
    let mount_points = start_server(rtsp_server_config).map_err(|err| StartupServerError {
        reason: format!("Failed to start RTSP server: {:?}", err),
    })?;
    let app_state = AppState::new(
        server_config.stream_expiration_time_in_minutes,
        &server_config.root_url,
        &mount_points.root_url.clone().to_owned(),
        mount_points.mount_points,
    );

    let app = Router::new()
        .route("/streams", post(add_stream))
        .route("/streams/{id}", delete(remove_stream))
        .route("/streams/stale", delete(remove_stale_streams))
        .with_state(app_state);
    let bind_str = format!("{}:{}", server_config.http_host, server_config.http_port);

    tracing::info!("Starting server on {}", bind_str);
    let listener = tokio::net::TcpListener::bind(&bind_str)
        .await
        .map_err(|err| StartupServerError {
            reason: format!("Failed to bind to {}: {:?}", bind_str, err),
        })?;

    tokio::spawn(async {
        tracing::info!("initializing main loop");
        let main_loop = glib::MainLoop::new(None, false);
        main_loop.run();
    });

    axum::serve(listener, app)
        .await
        .map_err(|err| StartupServerError {
            reason: format!("Server error: {:?}", err),
        })?;
    Ok(())
}
