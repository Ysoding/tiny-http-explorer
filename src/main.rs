use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use clap::Parser;
use http_server::Opts;
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower_http::services::ServeDir;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let opts = Opts::parse();
    process_http_server(opts.dir, opts.port).await?;

    Ok(())
}

pub async fn process_http_server(path: PathBuf, port: u16) -> Result<()> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("serving {:?} on {}", path, port);

    let state = HttpServerState { path: path.clone() };
    let router = Router::new()
        //  static server
        .nest_service("/tower", ServeDir::new(path))
        .route("/", get(root_handler))
        .route("/*path", get(handler))
        .with_state(Arc::new(state));

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

#[derive(Debug)]
struct HttpServerState {
    path: PathBuf,
}

async fn root_handler(State(state): State<Arc<HttpServerState>>) -> (StatusCode, Html<String>) {
    match list_dir(&state.path, &state.path).await {
        Ok(body) => (StatusCode::OK, Html(body)),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Html(e.to_string())),
    }
}

async fn handler(Path(path): Path<String>, State(state): State<Arc<HttpServerState>>) -> Response {
    // let path = path.unwrap_or_else(|| "/".to_string());
    let p = std::path::Path::new(&state.path).join(path);
    info!("handle: {:?}", p);
    if !p.exists() {
        info!("Path {} not found", p.display());
        StatusCode::NOT_FOUND.into_response()
    } else if p.is_dir() {
        match list_dir(p.as_path(), &state.path).await {
            Ok(body) => Html(body).into_response(),
            Err(e) => {
                warn!("{}", e.to_string());
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    } else {
        // file
        match tokio::fs::read(&p).await {
            Ok(content) => {
                info!("Read {} bytes", content.len());
                let mime_type = mime_guess::from_path(&p).first_or_text_plain();

                let mut res = content.into_response();

                let h = res.headers_mut();
                h.insert(header::CONTENT_TYPE, mime_type.to_string().parse().unwrap());
                // h.insert(header::CACHE_CONTROL, "max-age=86400".parse().unwrap());

                res
            }
            Err(e) => {
                warn!("Error reading file: {:?}", e);
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }
}

async fn list_dir(
    dir_path: &std::path::Path,
    base_path: &std::path::Path,
) -> anyhow::Result<String> {
    let mut entries = tokio::fs::read_dir(dir_path).await?;
    let mut body = String::new();
    body.push_str("<html><body><ul>");

    while let Some(entry) = entries.next_entry().await? {
        let file_name = entry.file_name();
        let file_name_str = file_name.to_string_lossy();
        let file_path = std::path::Path::new(&dir_path).join(&file_name);

        let metadata = entry.metadata().await?;

        let icon = if metadata.is_dir() { "📁" } else { "📄" };

        // 使用绝对路径 /跳转 /a/b/c  -> http://xxx/a/b/c
        let displayed_path = file_path.strip_prefix(base_path)?;
        body.push_str(&format!(
            "<li>{} <a href=\"/{}\">{}</a>  - {} bytes</li>",
            icon,
            displayed_path.to_string_lossy(),
            file_name_str,
            metadata.len()
        ));
    }
    body.push_str("</ul></body></html>");

    Ok(body)
}
