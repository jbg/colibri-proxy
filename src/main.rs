use std::{env, error::Error, net::SocketAddr};

use futures::stream::StreamExt;
use http::{Request, Response, StatusCode};
use k8s_openapi::api::core::v1::Pod;
use tokio::{io::AsyncWriteExt, net::{TcpListener, TcpStream}};

#[tokio::main]
async fn main() {
  let k8s = kube::Client::try_default().await.expect("k8s client setup failed");
  let k8s_pods: kube::Api<Pod> = kube::Api::namespaced(k8s, "default");

  let port: u16 = env::var("PORT").as_deref().unwrap_or("8080").parse().expect("invalid port");
  let addr = SocketAddr::from(([0, 0, 0, 0], port));
  let mut listener = TcpListener::bind(&addr).await.expect("failed to bind");
  eprintln!("listening on {}", addr);

  while let Ok((stream, addr)) = listener.accept().await {
    let k8s_pods = k8s_pods.clone();
    tokio::spawn(async move {
      if let Err(e) = handle_connection(k8s_pods, stream, addr).await {
        eprintln!("error handling websocket connection: {:?}", e);
      }
    });
  }
}

async fn handle_connection(k8s_pods: kube::Api<Pod>, mut stream: TcpStream, addr: SocketAddr) -> Result<(), Box<dyn Error>> {
  let mut peek_buf = [0u8; 6];
  stream.peek(&mut peek_buf).await?;
  if &peek_buf == b"GET / " {
    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await?;
    return Ok(());
  }

  let mut path = String::new();
  let incoming = tokio_tungstenite::accept_hdr_async(stream, |request: &Request<()>, response: Response<()>| {
    path = request.uri().path_and_query().unwrap().as_str().into();
    if !path.starts_with("/colibri-ws/jvb-") {
      Err(Response::builder().status(StatusCode::NOT_FOUND).body(None).unwrap())
    }
    else {
      Ok(response)
    }
  }).await?;

  // unwrap() is ok: the path has at least two / (see above)
  let pod_name = path.split('/').nth(2).unwrap();
  let pod = k8s_pods.get(pod_name).await?;

  if let Some(pod_ip) = pod.status.and_then(|status| status.pod_ip) {
    let uri = format!("ws://{}:8080{}", pod_ip, path);
    eprintln!("proxying websocket connection from {} to {}", addr, uri);
    let (outgoing, _response) = tokio_tungstenite::connect_async(&uri).await?;
    let (out_sink, out_stream) = outgoing.split();
    let (in_sink, in_stream) = incoming.split();
    futures::future::select(
      in_stream.forward(out_sink),
      out_stream.forward(in_sink),
    ).await.factor_first().0?;
    Ok(())
  }
  else {
    Err("pod does not have an IP".into())
  }
}
