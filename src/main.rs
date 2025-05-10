use dynamic_rtsp_relay::http_server::setup::setup_and_run;

#[tokio::main]
async fn main() {
    if let Err(err) = setup_and_run().await {
        panic!("failed to initialize server due to error: {:?}", err);
    }
}