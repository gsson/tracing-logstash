use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use log::info as log_info;
use std::convert::Infallible;
use std::net::SocketAddr;
use tracing::info;
use tracing::instrument;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Registry;

#[instrument]
async fn hello_world(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    info!(some_tag = 43, "tracing crate macro");
    log_info!("log crate macro");
    Ok(Response::new("Hello, World".into()))
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

#[tokio::main]
async fn main() {
    let logger = tracing_logstash::Layer::default().event_format(
        tracing_logstash::logstash::LogstashFormat::default().with_constants(vec![
            ("service.name", "tracing-logstash".to_owned()),
            ("service.environment", "development".to_owned()),
        ]),
    );

    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info"))
        .unwrap();

    let collector = Registry::default().with(logger).with(env_filter);

    tracing::subscriber::set_global_default(collector).unwrap();

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));

    let make_svc = make_service_fn(|_conn| async {
        // service_fn converts our function into a `Service`
        Ok::<_, Infallible>(service_fn(hello_world))
    });

    let server = Server::bind(&addr)
        .serve(make_svc)
        .with_graceful_shutdown(shutdown_signal());

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
