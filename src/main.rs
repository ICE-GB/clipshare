use crate::clipboard::Clipboard;
use clap::{command, Parser};
use clipboard::ClipboardObject;
use std::{error::Error, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select,
};
use tracing::{debug, error_span, info, instrument, trace, Instrument, Level};
use tracing_subscriber::FmtSubscriber;

mod clipboard;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Server port
    #[arg(short, long)]
    port: Option<u16>,

    /// Remote server url
    #[arg(short, long)]
    url: Option<String>,

    /// Don´t clear the clipboard on start
    #[arg(long)]
    no_clear: bool,

    /// Key
    #[arg(short, long)]
    key: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // a builder for `FmtSubscriber`.
    let subscriber = FmtSubscriber::builder()
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(Level::TRACE)
        // completes the builder.
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let args = Cli::parse();

    let clipboard = Arc::new(if args.no_clear {
        Clipboard::new()
    } else {
        Clipboard::cleared()
    });

    let key = std::env::var("CLIPSHARE_KEY").unwrap_or(args.key.unwrap_or("clipshare".to_string()));
    trace!(key);

    match args.url {
        Some(url) => start_client(clipboard, url, key).await,
        None => start_server(clipboard, args.port, key).await,
    }
}

#[instrument(skip(clipboard))]
async fn start_server(
    clipboard: Arc<Clipboard>,
    port: Option<u16>,
    key: String,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind(("0.0.0.0", port.unwrap_or(0))).await?;
    let port = listener.local_addr()?.port();
    eprintln!("Run `clipshare ip:{port}` on another machine of your network");

    while let Ok((stream, addr)) = listener.accept().await {
        trace!("New connection arrived");
        let ip = addr.ip();
        let clipboard = clipboard.clone();
        let key: String = key.clone();
        tokio::spawn(
            async move {
                let (mut reader, mut writer) = tokio::io::split(stream);

                let mut buf = [0; 1];
                reader.read_exact(&mut buf).await?;
                trace!("Read kind {buf:?}");
                match buf[0] {
                    0 => {
                        let mut buf = [0; std::mem::size_of::<u64>()];
                        reader.read_exact(&mut buf).await?;
                        let len = u64::from_be_bytes(buf).try_into()?;
                        trace!(len, "Read key len");

                        let mut buf = vec![0; len];
                        reader.read_exact(&mut buf).await?;
                        trace!(len, "Read key");

                        let client_key = std::str::from_utf8(&buf)?;
                        trace!(client_key);

                        if !key.eq(&client_key) {
                            error_span!("Key mismatch");
                            writer.shutdown().await?;
                        }
                    }
                    _n => {
                        error_span!("Key error");
                        writer.shutdown().await?;
                    }
                }

                if let Err(err) = select! {
                    result = recv_clipboard(clipboard.clone(), reader) => result,
                    result = send_clipboard(clipboard.clone(), writer) => result,
                } {
                    debug!(error = %err, "Server error");
                }
                trace!("Finishing server connection");
                Ok::<_, Box<dyn Error + Send + Sync>>(())
            }
            .instrument(error_span!("Connection", %ip)),
        );
    }

    Ok(())
}

#[instrument(skip(clipboard))]
async fn start_client(
    clipboard: Arc<Clipboard>,
    addr: String,
    key: String,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!("starting client");

    trace!("Begin client connection to {addr}");
    let stream = TcpStream::connect(addr).await?;
    let ip = stream.peer_addr()?.ip();

    let (reader, mut writer) = tokio::io::split(stream);
    let span = error_span!("Connection", %ip).entered();
    eprintln!("Clipboards connected");

    // 发送一个密钥
    let buf = [
        &[0][..],
        &u64::try_from(key.as_bytes().len())?.to_be_bytes()[..],
    ]
    .concat();
    writer.write_all(&buf).await?;
    writer.write_all(key.as_bytes()).await?;
    writer.flush().await?;

    if let Err(err) = select! {
        result = recv_clipboard(clipboard.clone(), reader).in_current_span() => result,
        result = send_clipboard(clipboard.clone(), writer).in_current_span() => result,
    } {
        debug!(error = %err, "Client error");
    }

    trace!("Finish client connection");
    span.exit();
    eprintln!("Clipboard closed");
    Ok(())
}

#[instrument(skip(clipboard, stream))]
async fn send_clipboard(
    clipboard: Arc<Clipboard>,
    mut stream: impl AsyncWrite + Send + Unpin,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        clipboard
            .paste()
            .in_current_span()
            .await?
            .write(&mut stream)
            .in_current_span()
            .await?;
        stream.flush().await?;
    }
}

#[instrument(skip(clipboard, stream))]
async fn recv_clipboard(
    clipboard: Arc<Clipboard>,
    mut stream: impl AsyncRead + Send + Unpin,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        let obj = ClipboardObject::from_reader(&mut stream)
            .in_current_span()
            .await?;
        clipboard.copy(obj).in_current_span().await?;
    }
}
