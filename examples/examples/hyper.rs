use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use log::info as log_info;
use std::convert::Infallible;
use std::net::SocketAddr;
use tracing::{debug, error, info, span, warn};
use tracing_attributes::instrument;
use tracing_core::dispatcher::Dispatch;
use tracing_logstash::{FieldAction, FieldRouter};
use tracing_subscriber::Layer;
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
    tracing_log::LogTracer::init().expect("Failed to initialise LogTracer");
    let (appender, _guard) = tracing_appender::non_blocking(std::io::stderr());

    let filter = tracing_subscriber::filter::EnvFilter::try_from_default_env().ok();

    let mut field_routes = FieldRouter::default();
    field_routes.add_tag("some_tag", "some_tag", FieldAction::Value);

    let logger = tracing_logstash::init(appender, field_routes);
    let subscriber = logger.with_subscriber(Registry::default());
    let dispatch = if let Some(filter) = filter {
        Dispatch::new(filter.with_subscriber(subscriber))
    } else {
        Dispatch::new(subscriber)
    };
    tracing_core::dispatcher::set_global_default(dispatch)
        .expect("Unable to install global subscriber");

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
