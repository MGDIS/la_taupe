use actix_multipart::form::{bytes::Bytes, text::Text, MultipartForm};
use actix_web::{http::header::ContentType, post, web, HttpResponse, Responder};
use reqwest::Response;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::timeout;

use crate::analysis::{Analysis, Hint, Type};
use crate::http::server::AppState;

pub const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;

#[derive(Deserialize)]
struct RequestedFile {
    url: String,
    hint: Option<Hint>,
}

#[derive(Debug, MultipartForm)]
struct UploadForm {
    file: Bytes,
    hint: Option<Text<String>>,
}

#[derive(Deserialize, Serialize)]
pub struct AnalysisError {
    pub upstream_body: Option<String>,
    pub upstream_status_code: Option<u16>,
    pub body: Option<String>,
}

#[post("/analyze/upload")]
pub async fn analyze_upload(
    MultipartForm(form): MultipartForm<UploadForm>,
    state: web::Data<AppState>,
) -> impl Responder {
    let mut filename = String::from("uploaded_file");
    if let Some(fname) = &form.file.file_name {
        filename = fname.to_string();
    }

    if form.file.data.is_empty() {
        return HttpResponse::BadRequest().json(AnalysisError {
            upstream_body: None,
            upstream_status_code: None,
            body: Some("No file provided".to_string()),
        });
    }

    let file_bytes = form.file.data.to_vec();

    let hint = form.hint.as_ref().and_then(|h| {
        let h_lower = h.to_lowercase();
        match h_lower.as_str() {
            "rib" => Some(Hint::Type(Type::Rib)),
            "2ddoc" => Some(Hint::Type(Type::Twoddoc)),
            _ => None,
        }
    });

    process_with_concurrency_control(state, file_bytes, hint, filename).await
}

#[post("/analyze")]
pub async fn analyze(
    requested_file: web::Json<RequestedFile>,
    state: web::Data<AppState>,
) -> impl Responder {
    let response = match reqwest::get(&requested_file.url).await {
        Ok(response) => response,
        Err(e) => {
            log::error!("Request failed: {}", e);
            return HttpResponse::InternalServerError().json(AnalysisError {
                upstream_body: None,
                upstream_status_code: None,
                body: Some(format!("Request failed: {}", e)),
            });
        }
    };

    if response.status().is_success() {
        handle_response(response, requested_file.hint, state).await
    } else {
        handle_error(response).await
    }
}

async fn process_with_concurrency_control(
    state: web::Data<AppState>,
    bytes: Vec<u8>,
    hint: Option<Hint>,
    name: String,
) -> HttpResponse {
    let _permit = match state.semaphore.acquire().await {
        Ok(permit) => permit,
        Err(_) => {
            return HttpResponse::ServiceUnavailable().json(AnalysisError {
                upstream_body: None,
                upstream_status_code: None,
                body: Some("Service overloaded".to_string()),
            });
        }
    };

    let mut lt = state.leptess_pool.acquire().await;
    let engine = state.ocr_engine_pool.acquire().await;

    let ocr_timeout = Duration::from_secs(state.ocr_timeout_secs);
    let leptess_pool = state.leptess_pool.clone();
    let ocr_engine_pool = state.ocr_engine_pool.clone();

    let result = timeout(
        ocr_timeout,
        web::block(move || {
            let analysis_result = Analysis::analyze(bytes, hint, &name, &mut lt, &engine);
            (analysis_result, lt, engine)
        }),
    )
    .await;

    match result {
        Ok(Ok((analysis_result, lt, engine))) => {
            leptess_pool.release(lt).await;
            ocr_engine_pool.release(engine).await;
            match analysis_result {
                Ok(analysis) => HttpResponse::Ok()
                    .content_type(ContentType::json())
                    .json(analysis),
                Err(error_msg) => HttpResponse::UnprocessableEntity()
                    .content_type(ContentType::json())
                    .json(AnalysisError {
                        upstream_status_code: None,
                        upstream_body: None,
                        body: Some(error_msg),
                    }),
            }
        }
        Ok(Err(e)) => {
            // web::block panic — instances lost, replenish pools
            log::error!("OCR processing panicked: {:?}", e);
            leptess_pool.replenish().await;
            ocr_engine_pool.replenish().await;
            HttpResponse::InternalServerError().json(AnalysisError {
                upstream_body: None,
                upstream_status_code: None,
                body: Some("Internal processing error".to_string()),
            })
        }
        Err(_) => {
            // Timeout — blocking thread still running, instances will be dropped when it finishes
            // Replenish with new instances
            log::warn!("OCR processing timed out after {}s", state.ocr_timeout_secs);
            leptess_pool.replenish().await;
            ocr_engine_pool.replenish().await;
            HttpResponse::ServiceUnavailable().json(AnalysisError {
                upstream_body: None,
                upstream_status_code: None,
                body: Some("OCR processing timeout".to_string()),
            })
        }
    }
}

async fn handle_error(resp: Response) -> HttpResponse {
    let status = resp.status();
    let upstream_status_code = Some(status.as_u16());
    let is_server_error = status.is_server_error();

    let upstream_body = Some(
        resp.text()
            .await
            .unwrap_or_else(|_| "unreadable upstream error".to_string()),
    );

    if is_server_error {
        HttpResponse::BadGateway().json(AnalysisError {
            upstream_body,
            upstream_status_code,
            body: Some("upstream server error".to_string()),
        })
    } else {
        HttpResponse::InternalServerError().json(AnalysisError {
            upstream_body,
            upstream_status_code,
            body: Some("upstream client error".to_string()),
        })
    }
}

async fn handle_response(
    mut resp: Response,
    hint: Option<Hint>,
    state: web::Data<AppState>,
) -> HttpResponse {
    let len = resp
        .headers()
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(MAX_FILE_SIZE);

    if len > MAX_FILE_SIZE {
        return HttpResponse::UnprocessableEntity()
            .content_type(ContentType::json())
            .json(AnalysisError {
                upstream_status_code: None,
                upstream_body: None,
                body: Some("File too big".to_string()),
            });
    }

    let mut bytes: Vec<u8> = Vec::with_capacity(len);

    while let Ok(chunk) = resp.chunk().await {
        match chunk {
            Some(data) => bytes.extend_from_slice(&data),
            None => break,
        }

        if bytes.len() > MAX_FILE_SIZE {
            break;
        }
    }

    let size = bytes.len();

    if size > MAX_FILE_SIZE {
        return HttpResponse::UnprocessableEntity()
            .content_type(ContentType::json())
            .json(AnalysisError {
                upstream_status_code: None,
                upstream_body: None,
                body: Some("File too big".to_string()),
            });
    }

    process_with_concurrency_control(state, bytes, hint, "remote_file".to_string()).await
}
