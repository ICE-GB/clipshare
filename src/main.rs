use std::{error::Error, sync::Arc};
use clap::{command, Parser};
use clipboard::ClipboardObject;
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    select,
};
use tracing::{debug, error_span, info, instrument, Instrument, Level, trace};
use tracing_subscriber::FmtSubscriber;
use crate::clipboard::Clipboard;

mod clipboard;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Clipboard id to connect to
    clipboard: Option<String>,

    /// Don´t clear the clipboard on start
    #[arg(long)]
    no_clear: bool,
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

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let args = Cli::parse();

    let clipboard = Arc::new(if args.no_clear {
        Clipboard::new()
    } else {
        Clipboard::cleared()
    });

    match args.clipboard {
        Some(addr) => start_client(clipboard, addr).await,
        None => start_server(clipboard).await,
    }
}

#[instrument(skip(clipboard))]
async fn start_server(clipboard: Arc<Clipboard>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();
    eprintln!("Run `clipshare ip:{port}` on another machine of your network");

    while let Ok((stream, addr)) = listener.accept().await {
        trace!("New connection arrived");
        let ip = addr.ip();
        let clipboard = clipboard.clone();
        tokio::spawn(
            async move {
                let (reader, writer) = tokio::io::split(stream);

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
) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!("starting client");

    trace!("Begin client connection to {addr}");
    let stream = TcpStream::connect(addr).await?;
    let ip = stream.peer_addr()?.ip();

    let (reader, writer) = tokio::io::split(stream);
    let span = error_span!("Connection", %ip).entered();
    eprintln!("Clipboards connected");

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