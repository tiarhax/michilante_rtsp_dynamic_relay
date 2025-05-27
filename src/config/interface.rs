use std::future::Future;
use std::env;
#[derive(Debug, Clone)]
pub struct Camera {
    pub id: String,
    pub source_url: String,
}

pub trait CameraConfigRepository {
    type Error;
    fn list_all(&self) -> impl Future<Output = Result<Vec<Camera>, Self::Error>> + Send;
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

impl ServerConfig {
    pub fn new(host: String, port: u16, user: String, password: String) -> Self {
        Self { host, port, user, password }
    }

    pub fn load_from_env() -> Self {
        let host = env::var("SERVER_HOST").expect("SERVER_HOST must be set");
        let port = env::var("SERVER_PORT")
            .expect("SERVER_PORT must be set")
            .parse::<u16>()
            .expect("SERVER_PORT must be a valid u16");
        let user = env::var("SERVER_USER").expect("SERVER_USER must be set");
        let password = env::var("SERVER_PASSWORD").expect("SERVER_PASSWORD must be set");

        Self::new(host, port, user, password)
    }
}


pub trait ServerConfigRepository {
    type Error;
    fn get_config(&self) -> impl Future<Output = Result<Vec<Camera>, Self::Error>> + Send;
}


