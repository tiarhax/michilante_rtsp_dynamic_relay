use anyhow::Error;
use derive_more::derive::{Display, Error};
use gst_rtsp_server::{prelude::*, RTSPMedia, RTSPMountPoints};

#[derive(Debug, Display, Error)]
#[display("Could not get mount points")]
struct NoMountPoints;
mod auth {

    pub mod imp {
        use gst_rtsp::{RTSPHeaderField, RTSPStatusCode};
        use gst_rtsp_server::{prelude::*, subclass::prelude::*, RTSPContext};

        impl Default for Auth {
            fn default() -> Self {
                let user = std::env::var("RTSP_SERVER_USER").expect("SERVER_USER configuration missing");
                let password = std::env::var("RTSP_SERVER_PASSWORD")
                    .expect("SERVER_PASSWORD configuration missing");

                Self {
                    user: user,
                    password: password,
                }
            }
        }
        pub struct Auth {
            pub user: String,
            pub password: String,
        }

        impl Auth {
            fn external_auth(&self, auth: &str) -> Option<String> {
                if let Ok(decoded) = data_encoding::BASE64.decode(auth.as_bytes()) {
                    if let Ok(decoded) = std::str::from_utf8(&decoded) {
                        let tokens = decoded.split(':').collect::<Vec<_>>();

                        if tokens == vec![self.user.clone(), self.password.clone()] {
                            return Some(tokens[0].into());
                        }
                    }
                }
                None
            }

            fn external_access_check(&self, user: &str) -> bool {
                user == self.user
            }
        }

        #[glib::object_subclass]
        impl ObjectSubclass for Auth {
            const NAME: &'static str = "RsRTSPAuth";
            type Type = super::Auth;
            type ParentType = gst_rtsp_server::RTSPAuth;
        }

        impl ObjectImpl for Auth {}

        impl RTSPAuthImpl for Auth {
            fn authenticate(&self, ctx: &RTSPContext) -> bool {
                let req = ctx
                    .request()
                    .expect("Context without request. Should not happen!");

                if let Some(auth_credentials) = req.parse_auth_credentials().first() {
                    if let Some(authorization) = auth_credentials.authorization() {
                        if let Some(user) = self.external_auth(authorization) {
                            ctx.set_token(
                                gst_rtsp_server::RTSPToken::builder()
                                    .field("user", user)
                                    .build(),
                            );
                            return true;
                        }
                    }
                }

                false
            }

            fn check(&self, ctx: &RTSPContext, role: &glib::GString) -> bool {
                if !role.starts_with("auth.check.media.factory") {
                    return true;
                }

                if ctx.token().is_none() {
                    if !self.authenticate(ctx) {
                        if let Some(resp) = ctx.response() {
                            resp.init_response(RTSPStatusCode::Unauthorized, ctx.request());
                            resp.add_header(
                                RTSPHeaderField::WwwAuthenticate,
                                "Basic realm=\"CustomRealm\"",
                            );
                            if let Some(client) = ctx.client() {
                                client.send_message(resp, ctx.session());
                            }
                        }
                        return false;
                    }
                }

                if let Some(token) = ctx.token() {
                    if self.external_access_check(&token.string("user").unwrap_or_default()) {
                        return true;
                    } else if let Some(resp) = ctx.response() {
                        resp.init_response(RTSPStatusCode::NotFound, ctx.request());
                        if let Some(client) = ctx.client() {
                            client.send_message(resp, ctx.session());
                        }
                    }
                }
                false
            }
        }
    }

    glib::wrapper! {
        pub struct Auth(ObjectSubclass<imp::Auth>) @extends gst_rtsp_server::RTSPAuth;
    }

    impl Default for Auth {
        // Creates a new instance of our auth
        fn default() -> Self {
            glib::Object::new()
        }
    }
}
pub struct MountServerResult {
    pub mount_points: RTSPMountPoints,
    pub root_url: String,
}
#[derive(Debug)]
pub struct RTSPServerConfig {
    pub host_address: String,
    pub host_name: String,
    pub port: String,
    pub user: String,
    pub password: String,
}
#[derive(Debug)]
pub struct RTSPServerInitializationError {
    pub reason: String
}

#[derive(Debug)]
pub struct RTSPServerReadConfigError {
    pub reason: String
}

pub fn load_rtsp_server_config() -> Result<RTSPServerConfig, RTSPServerReadConfigError> {
    let host_address = std::env::var("RTSP_SERVER_HOST_ADDRESS").map_err(|err| RTSPServerReadConfigError {
        reason: format!("Failed to read RTSP_SERVER_HOST from environment: {}", err),
    })?;
    let host_name = std::env::var("RTSP_SERVER_HOST_NAME").map_err(|err| RTSPServerReadConfigError {
        reason: format!("Failed to read RTSP_SERVER_HOST from environment: {}", err),
    })?;
    let port = std::env::var("RTSP_SERVER_PORT").map_err(|err| RTSPServerReadConfigError {
        reason: format!("Failed to read RTSP_SERVER_PORT from environment: {}", err),
    })?;
    let user = std::env::var("RTSP_SERVER_USER").map_err(|err| RTSPServerReadConfigError {
        reason: format!("Failed to read RTSP_SERVER_USER from environment: {}", err),
    })?;
    let password = std::env::var("RTSP_SERVER_PASSWORD").map_err(|err| RTSPServerReadConfigError {
        reason: format!("Failed to read RTSP_SERVER_PASSWORD from environment: {}", err),
    })?;

    Ok(RTSPServerConfig {
        host_address,
        host_name,
        port,
        user,
        password,
    })
}

pub fn start_server(config: RTSPServerConfig) -> Result<MountServerResult, RTSPServerInitializationError> {
    gstreamer::init().map_err(|err| RTSPServerInitializationError {
        reason: format!("Failed to initialize GStreamer: {}", err),
    })?;
    let server = gst_rtsp_server::RTSPServer::new();

    let auth = auth::Auth::default();
    server.set_auth(Some(&auth));
    tracing::info!("initializing rtsp server at: {}:{}", config.host_name, config.port);
    server.set_service(&config.port);
    server.set_address(&config.host_address);
    let mounts = server.mount_points().ok_or_else(|| RTSPServerInitializationError {
        reason: "Failed to get mount points from the RTSP server".to_string(),
    })?;
    let root_url = format!(
        "rtsp://{}:{}@{}:{}/",
        config.user, config.password, config.host_name, config.port
    );
    server.attach(None).map_err(|e| RTSPServerInitializationError {
        reason: format!("could not attach context due to error {:?}", e),
    })?;

    let res = MountServerResult {
        mount_points: mounts,
        root_url,
    };
    Ok(res)
}
