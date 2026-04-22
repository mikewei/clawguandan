use clawguandan::api::{AppState, router};
use clawguandan::store::TableStore;
use std::net::{IpAddr, SocketAddr};
use tower_http::trace::TraceLayer;

pub async fn serve(ip: IpAddr, port: u16) -> Result<(), String> {
    let addr = SocketAddr::new(ip, port);
    let state = AppState {
        store: TableStore::new(),
        listen_port: port,
        bind_ip: ip,
    };
    let app = router(state).layer(TraceLayer::new_for_http());

    tracing::info!(%addr, "clawguandan listening");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| e.to_string())?;
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}
