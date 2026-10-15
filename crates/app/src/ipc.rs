use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use proto::{Error, ErrorCode, Request, RequestEnvelope, Response};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

use crate::app::{self, Event};

pub enum BindOutcome {
    Bound(std::os::unix::net::UnixListener),
    AlreadyRunning,
}

pub fn prepare(path: &Path) -> io::Result<BindOutcome> {
    if std::os::unix::net::UnixStream::connect(path).is_ok() {
        return Ok(BindOutcome::AlreadyRunning);
    }
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    let listener = std::os::unix::net::UnixListener::bind(path)?;
    listener.set_nonblocking(true)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    return Ok(BindOutcome::Bound(listener));
}

pub fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
}

pub async fn serve(listener: UnixListener) {
    loop {
        match listener.accept().await {
            Ok((stream, _address)) => {
                tokio::spawn(handle(stream));
            }
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
}

async fn handle(stream: UnixStream) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    loop {
        match read_frame(&mut reader).await {
            Ok(Some(frame)) => {
                let (response, is_quit) = handle_frame(&frame).await;
                if write_half.write_all(&proto::encode(&response)).await.is_err() {
                    break;
                }
                if is_quit {
                    app::post(Event::Quit);
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {
                let response = Response::err(ErrorCode::Malformed, "frame exceeds the line limit");
                let _ = write_half.write_all(&proto::encode(&response)).await;
                break;
            }
        }
    }
}

async fn handle_frame(frame: &[u8]) -> (Response, bool) {
    match proto::decode::<RequestEnvelope>(frame) {
        Ok(envelope) => {
            if envelope.v != proto::PROTOCOL_VERSION {
                return (Response::err(ErrorCode::Invalid, "unsupported protocol version"), false);
            }
            let is_quit = matches!(envelope.request, Request::Quit);
            let (sender, receiver) = oneshot::channel();
            app::post(Event::Request(envelope.request, sender));
            let response = match receiver.await {
                Ok(response) => response,
                Err(_) => Response::err(ErrorCode::Internal, "request dropped"),
            };
            return (response, is_quit);
        }
        Err(Error::TooLong) => return (Response::err(ErrorCode::Malformed, "line too long"), false),
        Err(Error::Malformed(_)) => return (Response::err(ErrorCode::Malformed, "malformed request"), false),
    }
}

async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    let read = (&mut *reader).take(proto::MAX_LINE as u64).read_until(b'\n', &mut frame).await?;
    if read == 0 {
        return Ok(None);
    }
    if frame.last() != Some(&b'\n') {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame exceeds the line limit or is incomplete"));
    }
    return Ok(Some(frame));
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::{Request, ResponseBody, encode};

    #[tokio::test]
    async fn rejects_malformed_frame() {
        let (response, _) = handle_frame(b"garbage\n").await;
        match response.body {
            ResponseBody::Err(body) => assert_eq!(body.code, ErrorCode::Malformed),
            ResponseBody::Ok { .. } => panic!("expected error"),
        }
    }

    #[tokio::test]
    async fn rejects_wrong_protocol_version() {
        let mut frame = encode(&RequestEnvelope::new(Request::Status));
        let text = String::from_utf8(frame.clone()).expect("utf8");
        frame = text.replace("\"v\":1", "\"v\":99").into_bytes();
        let (response, _) = handle_frame(&frame).await;
        match response.body {
            ResponseBody::Err(body) => assert_eq!(body.code, ErrorCode::Invalid),
            ResponseBody::Ok { .. } => panic!("expected error"),
        }
    }

    #[tokio::test]
    async fn read_frame_rejects_oversize() {
        let mut input = vec![b'a'; proto::MAX_LINE + 8];
        input.push(b'\n');
        let mut reader = BufReader::new(&input[..]);
        assert!(read_frame(&mut reader).await.is_err());
    }

    #[tokio::test]
    async fn read_frame_returns_none_on_eof() {
        let mut reader = BufReader::new(&b""[..]);
        assert!(matches!(read_frame(&mut reader).await, Ok(None)));
    }
}
