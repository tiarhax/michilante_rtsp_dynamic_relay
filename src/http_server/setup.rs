use std::sync::Arc;

use aws_config::BehaviorVersion;
use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::{
    config::{implementation::AWSCameraConfigRepository, interface::CameraConfigRepository},
    http_server::{
        appstate::{AppState},
        endpoints::{
            add_stream, add_stream_to_state, remove_stale_streams, remove_stream, list_streams, AddStreamInput,
        },
    },
    rtsp_server::{load_rtsp_server_config, start_server},
};

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
    pub load_default_streams: bool,
    pub table_name: String,
    pub partition_key: String
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

    let load_default_streams: bool = std::env::var("LOAD_DEFAULT_STREAMS")
        .map_err(|_| ReadConfigErr {
            reason: "LOAD_DEFAULT_STREAMS not set or invalid".to_string(),
        })?
        .parse()
        .map_err(|_| ReadConfigErr {
            reason: "LOAD_DEFAULT_STREAMS must be a valid boolean".to_string(),
        })?;
    let table_name = std::env::var("TABLE_NAME")
        .map_err(|_| ReadConfigErr {
            reason: "TABLE_NAME not set or invalid".to_string(),
        })?;

    let partition_key = std::env::var("PARTITION_KEY")
        .map_err(|_| ReadConfigErr {
            reason: "PARTITION_KEY not set or invalid".to_string(),
        })?;

    Ok(ServerConfig {
        http_port,
        http_host,
        stream_expiration_time_in_minutes,
        root_url,
        load_default_streams,
        table_name,
        partition_key,
    })
}

pub async fn setup_and_run() -> Result<(), StartupServerError> {
    tracing_subscriber::fmt::init();
    if let Err(_) = dotenvy::dotenv() {
        tracing::info!(
            ".env file could not be loaded, expecting env variables to be already present"
        );
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

    if server_config.load_default_streams {
        let camera_config = AWSCameraConfigRepository::new(
            aws_config::load_defaults(BehaviorVersion::v2025_01_17()).await,
            server_config.table_name,
            server_config.partition_key
        )
        .await;
        let cameras = match camera_config.list_all().await {
            Ok(cameras) => cameras,
            Err(e) => {
                eprintln!("Failed to list cameras: {:?}", e);
                std::process::exit(1);
            }
        };

        let add_stream_inputs = cameras
            .into_iter()
            .map(|e| AddStreamInput {
                name: e.id,
                down_scale: false,
                source_url: e.source_url,
            })
            .collect::<Vec<AddStreamInput>>();

        for add_stream_input in add_stream_inputs {
            add_stream_to_state(app_state.clone(), add_stream_input).await
                .map_err(|e| {
                    StartupServerError {
                        reason: format!("Failed to add default stream: {:?}", e),
                    }
                })?;
        }
    }

    let app = Router::new()
        .route("/streams", post(add_stream))
        .route("/streams", get(list_streams))
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
