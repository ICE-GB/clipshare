use std::{error::Error, io, mem, process::exit, sync::Arc, time::Duration};

use clap::{command, Parser};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    select,
    time::{sleep, timeout},
};
use tracing::{debug, error_span, instrument, metadata::LevelFilter, trace, Instrument};
use tracing_error::ErrorLayer;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::clipboard::Clipboard;

mod clipboard;

const HANDSHAKE: &[u8; 9] = b"clipshare";

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Clipboard id to connect to
    clipboard: Option<u16>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::ERROR.into())
                .from_env_lossy(),
        )
        .with(ErrorLayer::default())
        .init();

    let clipboard = Arc::new(Clipboard::new());
    match Cli::parse().clipboard {
        Some(port) => start_client(clipboard, port).await,
        None => start_server(clipboard).await,
    }
}

#[instrument(skip(clipboard))]
async fn start_server(clipboard: Arc<Clipboard>) -> Result<(), Box<dyn Error + Send + Sync>> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;
    let port = socket.local_addr()?.port();

    tokio::spawn(
        async move {
            loop {
                if socket.send_to(HANDSHAKE, ("255.255.255.255", port)).await? == 0 {
                    debug!("Failed to send UDP packet");
                    break;
                }
                sleep(Duration::from_secs(3)).await;
            }
            io::Result::Ok(())
        }
        .instrument(error_span!("Port publishing", port)),
    );

    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    eprintln!("Run `clipshare {port}` on another machine of your network");

    while let Ok((mut stream, addr)) = listener.accept().await {
        trace!("New connection arrived");
        let ip = addr.ip();
        let clipboard = clipboard.clone();
        tokio::spawn(
            async move {
                let (reader, writer) = stream.split();

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
    clipboard_port: u16,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let socket = UdpSocket::bind(("0.0.0.0", clipboard_port)).await?;
    eprintln!("Connecting to clipboard {clipboard_port}...");
    let mut buf = [0_u8; 9];

    let Ok(Ok((_, addr))) = timeout(Duration::from_secs(5), socket.recv_from(&mut buf)).await else {
        eprintln!("Timeout trying to connect to clipboard {clipboard_port}");
        exit(1);
    };

    if &buf == HANDSHAKE {
        trace!("Begin client connection");
        let mut stream = TcpStream::connect(addr).await?;
        let (reader, writer) = stream.split();
        let ip = reader.peer_addr()?.ip();
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
    } else {
        eprintln!("Clipboard {clipboard_port} not found");
        exit(1);
    }
}

#[instrument(skip(clipboard, stream))]
async fn send_clipboard(
    clipboard: Arc<Clipboard>,
    mut stream: impl AsyncWrite + Send + Unpin,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        let paste = clipboard.paste().in_current_span().await?;
        let text = paste.as_bytes();
        let buf = [&text.len().to_be_bytes(), text].concat();
        trace!(text = paste, "Sent text");
        if stream.write(&buf).await? == 0 {
            trace!("Stream closed");
            break Ok(());
        }
    }
}

#[instrument(skip(clipboard, stream))]
async fn recv_clipboard(
    clipboard: Arc<Clipboard>,
    mut stream: impl AsyncRead + Send + Unpin,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    loop {
        let mut buf = [0; mem::size_of::<usize>()];
        if stream.read(&mut buf).await? == 0 {
            trace!("Stream closed");
            break Ok(());
        }
        let len = usize::from_be_bytes(buf);
        let mut buf = vec![0; len];
        stream.read_exact(&mut buf).await?;

        if let Ok(text) = std::str::from_utf8(&buf) {
            trace!(text = text, "Received text");
            clipboard.copy(text).in_current_span().await?;
        }
    }
}
