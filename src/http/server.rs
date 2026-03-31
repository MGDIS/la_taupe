use std::env;
use std::sync::Arc;

use super::{analyze, ping, version};
use actix_multipart::form::MultipartFormConfig;
use actix_multipart::MultipartError;
use actix_web::http::header::ContentType;
use actix_web::{middleware::Logger, web, App, HttpResponse, HttpServer};
use env_logger::Env;
use std::io::Write;
use std::net::{SocketAddr, ToSocketAddrs};
use tokio::sync::Semaphore;

use crate::config::AppConfig;
use crate::http::analyze::{AnalysisError, MAX_FILE_SIZE};
use crate::pool::{LepTessPool, OcrEnginePool};

pub struct AppState {
    pub semaphore: Arc<Semaphore>,
    pub leptess_pool: Arc<LepTessPool>,
    pub ocr_engine_pool: Arc<OcrEnginePool>,
    pub ocr_timeout_secs: u64,
}

#[actix_web::main]
pub async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .init();

    let config = AppConfig::from_env();

    let state = web::Data::new(AppState {
        semaphore: Arc::new(Semaphore::new(config.max_concurrent_ocr)),
        leptess_pool: Arc::new(LepTessPool::new(config.max_concurrent_ocr)),
        ocr_engine_pool: Arc::new(OcrEnginePool::new(config.max_concurrent_ocr)),
        ocr_timeout_secs: config.ocr_timeout_secs,
    });

    HttpServer::new(move || {
        let multipart_config = MultipartFormConfig::default()
            .total_limit(MAX_FILE_SIZE)
            .memory_limit(MAX_FILE_SIZE)
            .error_handler(|err, _req| {
                if let MultipartError::Payload(_) = &err {
                    let response = HttpResponse::UnprocessableEntity()
                        .content_type(ContentType::json())
                        .json(AnalysisError {
                            upstream_status_code: None,
                            upstream_body: None,
                            body: Some("File too big".to_string()),
                        });
                    return actix_web::error::InternalError::from_response(err, response).into();
                }
                err.into()
            });

        App::new()
            .app_data(state.clone())
            .app_data(multipart_config)
            .wrap(Logger::new(r#"{"timestamp":"%t","method":"%r","status":%s,"response_time":%D,"remote_addr":"%a","user_agent":"%{User-Agent}i","referer":"%{Referer}i","remote_file":"%{X-Remote-File}i"}"#))
            .service(analyze::analyze)
            .service(analyze::analyze_upload)
            .service(ping::ping)
            .service(version::version)
    })
    .workers(config.workers)
    .bind(binding_address())?
    .run()
    .await
}

pub fn binding_address() -> SocketAddr {
    let address = env::var("LA_TAUPE_ADDRESS")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .to_string();
    address.to_socket_addrs().unwrap().next().unwrap()
}
