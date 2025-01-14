use std::fs::File;
use std::io::{BufReader, Error as IOError, ErrorKind};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use rustls_pemfile::{certs, pkcs8_private_keys};
use tokio::net::TcpListener;
use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig};
use tokio_rustls::TlsAcceptor;

use pgwire::api::auth::noop::NoopStartupHandler;
use pgwire::api::portal::Portal;
use pgwire::api::query::{ExtendedQueryHandler, SimpleQueryHandler};
use pgwire::api::results::{text_query_response, FieldInfo, Response, Tag, TextDataRowEncoder};
use pgwire::api::{ClientInfo, Type};
use pgwire::error::PgWireResult;
use pgwire::tokio::process_socket;

pub struct DummyProcessor;

#[async_trait]
impl SimpleQueryHandler for DummyProcessor {
    async fn do_query<C>(&self, _client: &C, query: &str) -> PgWireResult<Vec<Response>>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        println!("{:?}", query);
        if query.starts_with("SELECT") {
            let f1 = FieldInfo::new("id".into(), None, None, Type::INT4);
            let f2 = FieldInfo::new("name".into(), None, None, Type::VARCHAR);

            let data = vec![
                (Some(0), Some("Tom".to_string())),
                (Some(1), Some("Jerry".to_string())),
                (Some(2), None),
            ];
            let data_row_stream = stream::iter(data.into_iter().map(|r| {
                let mut encoder = TextDataRowEncoder::new(2);
                encoder.append_field(r.0.as_ref())?;
                encoder.append_field(r.1.as_ref())?;

                encoder.finish()
            }));

            Ok(vec![Response::Query(text_query_response(
                vec![f1, f2],
                data_row_stream,
            ))])
        } else {
            Ok(vec![Response::Execution(Tag::new_for_execution(
                "OK",
                Some(1),
            ))])
        }
    }
}

#[async_trait]
impl ExtendedQueryHandler for DummyProcessor {
    async fn do_query<C>(
        &self,
        _client: &mut C,
        _portal: &Portal,
        _max_row: usize,
    ) -> PgWireResult<Response>
    where
        C: ClientInfo + Unpin + Send + Sync,
    {
        todo!()
    }
}

fn setup_tls() -> Result<TlsAcceptor, IOError> {
    let cert = certs(&mut BufReader::new(File::open("examples/ssl/server.crt")?))
        .map_err(|_| IOError::new(ErrorKind::InvalidInput, "invalid cert"))
        .map(|mut certs| certs.drain(..).map(Certificate).collect())?;
    let key = pkcs8_private_keys(&mut BufReader::new(File::open("examples/ssl/server.key")?))
        .map_err(|_| IOError::new(ErrorKind::InvalidInput, "invalid key"))
        .map(|mut keys| keys.drain(..).map(PrivateKey).next().unwrap())?;

    let config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(cert, key)
        .map_err(|err| IOError::new(ErrorKind::InvalidInput, err))?;

    Ok(TlsAcceptor::from(Arc::new(config)))
}

#[tokio::main]
pub async fn main() {
    let processor = Arc::new(DummyProcessor);
    let authenticator = Arc::new(NoopStartupHandler);

    let server_addr = "127.0.0.1:5433";
    let tls_acceptor = Arc::new(setup_tls().unwrap());
    let listener = TcpListener::bind(server_addr).await.unwrap();

    println!("Listening to {}", server_addr);
    loop {
        let incoming_socket = listener.accept().await.unwrap();
        let tls_acceptor_ref = tls_acceptor.clone();
        let authenticator_ref = authenticator.clone();
        let processor_ref = processor.clone();
        tokio::spawn(async move {
            process_socket(
                incoming_socket.0,
                Some(tls_acceptor_ref),
                authenticator_ref,
                processor_ref.clone(),
                processor_ref,
            )
            .await
        });
    }
}
