//! Cancellable HTTP waits with an owned runtime per request. Uploads read bounded
//! chunks on that runtime, never in detached blocking tasks that outlive staging.
use crate::{lfs_pool, Options};
use anyhow::Result;
use std::{
    future::Future,
    io::{self, Read},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

fn wait<T>(
    runtime: &tokio::runtime::Runtime,
    options: &Options,
    stopped: &AtomicBool,
    future: impl Future<Output = reqwest::Result<T>>,
) -> Result<T> {
    runtime.block_on(async {
        let mut future = std::pin::pin!(future);
        loop {
            lfs_pool::check(options, stopped)?;
            match tokio::time::timeout(Duration::from_millis(25), future.as_mut()).await {
                Ok(result) => {
                    lfs_pool::check(options, stopped)?;
                    return result.map_err(|_| anyhow::anyhow!("LFS HTTPS request failed (TLS, timeout or transport); URLs/headers withheld"));
                }
                Err(_) => continue,
            }
        }
    })
}

pub(crate) struct Response {
    // Drop the response before shutting down its runtime. Runtime shutdown drops
    // connection/upload tasks and their readers before this request can return.
    response: reqwest::Response,
    runtime: tokio::runtime::Runtime,
    pending: bytes::Bytes,
    options: Options,
    stopped: Arc<AtomicBool>,
}
impl Response {
    pub(crate) fn send(
        request: reqwest::RequestBuilder,
        options: &Options,
        stopped: Arc<AtomicBool>,
    ) -> Result<Self> {
        lfs_pool::check(options, &stopped)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| anyhow::anyhow!("Cannot initialize cancellable LFS runtime"))?;
        let response = wait(&runtime, options, &stopped, async { request.send().await })?;
        Ok(Self {
            response,
            runtime,
            pending: bytes::Bytes::new(),
            options: options.clone(),
            stopped,
        })
    }
    pub(crate) fn status(&self) -> reqwest::StatusCode {
        self.response.status()
    }
    pub(crate) fn content_length(&self) -> Option<u64> {
        self.response.content_length()
    }
}
impl Read for Response {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        lfs_pool::check(&self.options, &self.stopped).map_err(io::Error::other)?;
        if buffer.is_empty() {
            return Ok(0);
        }
        while self.pending.is_empty() {
            match wait(
                &self.runtime,
                &self.options,
                &self.stopped,
                self.response.chunk(),
            )
            .map_err(io::Error::other)?
            {
                Some(bytes) => self.pending = bytes,
                None => return Ok(0),
            }
        }
        let count = buffer.len().min(self.pending.len());
        buffer[..count].copy_from_slice(&self.pending.split_to(count));
        lfs_pool::check(&self.options, &self.stopped).map_err(io::Error::other)?;
        Ok(count)
    }
}

pub(crate) fn upload_body(reader: impl Read + Send + 'static) -> reqwest::Body {
    // Synchronous local reads are bounded to 64 KiB and run only while the
    // request's runtime is polling. No spawned file-I/O tasks retain staging.
    reqwest::Body::wrap_stream(futures_util::stream::try_unfold(
        reader,
        |mut reader| async {
            let mut bytes = vec![0; 65536];
            let count = reader.read(&mut bytes)?;
            if count == 0 {
                return Ok::<_, io::Error>(None);
            }
            bytes.truncate(count);
            Ok(Some((bytes, reader)))
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        net::TcpListener,
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Instant,
    };

    // Plain loopback sockets test transport lifetimes, not production TLS policy.
    fn client(timeout: Duration) -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .retry(reqwest::retry::never())
            .timeout(timeout)
            .build()
            .unwrap()
    }
    fn headers(socket: &mut std::net::TcpStream) -> Vec<u8> {
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = vec![];
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            assert_eq!(socket.read(&mut byte).unwrap(), 1);
            request.push(byte[0]);
            assert!(request.len() <= 8192);
        }
        request
    }
    #[test]
    fn cancellation_interrupts_batch_headers_bodies_and_sibling_waits() {
        for stage in ["headers", "body", "sibling"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/batch", listener.local_addr().unwrap());
            let (ready_tx, ready_rx) = mpsc::channel();
            let (stop_tx, stop_rx) = mpsc::channel();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                assert!(headers(&mut socket).starts_with(b"POST "));
                if stage == "body" {
                    socket
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{")
                        .unwrap();
                    socket.flush().unwrap();
                }
                ready_tx.send(()).unwrap();
                stop_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                listener.set_nonblocking(true).unwrap();
                assert!(listener.accept().is_err(), "LFS request was retried");
            });
            let options = Options::default();
            let stopped = Arc::new(AtomicBool::new(false));
            let flag = if stage == "sibling" {
                stopped.clone()
            } else {
                options.cancelled.clone()
            };
            let cancel = std::thread::spawn(move || {
                ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                std::thread::sleep(Duration::from_millis(50));
                let start = Instant::now();
                flag.store(true, Ordering::Release);
                start
            });
            let result = Response::send(
                client(Duration::from_secs(5)).post(url).body("{}"),
                &options,
                stopped,
            )
            .and_then(|mut response| {
                let mut bytes = vec![];
                response.read_to_end(&mut bytes)?;
                Ok(())
            });
            assert!(result.is_err());
            assert!(
                cancel.join().unwrap().elapsed() < Duration::from_millis(500),
                "{stage} cancellation exceeded 500 ms"
            );
            if stage == "sibling" {
                assert!(!options.cancelled.load(Ordering::Relaxed));
            }
            stop_tx.send(()).unwrap();
            server.join().unwrap();
        }
    }

    struct TrackedReader {
        dropped: Arc<AtomicBool>,
        reads: Arc<AtomicUsize>,
    }
    impl Read for TrackedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            assert!(buffer.len() <= 65536, "Upload buffering exceeded 64 KiB");
            assert!(!self.dropped.load(Ordering::Acquire));
            self.reads.fetch_add(1, Ordering::Relaxed);
            buffer.fill(42);
            Ok(buffer.len())
        }
    }
    impl Drop for TrackedReader {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }
    #[test]
    fn early_response_shutdown_releases_upload_reader() {
        for code in [200, 503] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/object", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                headers(&mut socket);
                write!(
                    socket,
                    "HTTP/1.1 {code} Fixture\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            });
            let options = Options::default();
            let dropped = Arc::new(AtomicBool::new(false));
            let reader = TrackedReader {
                dropped: dropped.clone(),
                reads: Arc::new(AtomicUsize::new(0)),
            };
            let response = Response::send(
                client(Duration::from_secs(5))
                    .put(url)
                    .header(reqwest::header::CONTENT_LENGTH, 128 * 1024 * 1024u64)
                    .body(upload_body(reader.take(128 * 1024 * 1024))),
                &options,
                Arc::new(AtomicBool::new(false)),
            );
            // Some HTTP stacks reject an early close before exposing its status.
            // Both paths must synchronously release the upload reader.
            drop(response);
            assert!(dropped.load(Ordering::Acquire));
            assert!(!options.cancelled.load(Ordering::Relaxed));
            server.join().unwrap();
        }
    }
    #[test]
    fn cancelled_backpressured_upload_releases_reader_before_return() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/object", listener.local_addr().unwrap());
        let (ready_tx, ready_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            assert!(headers(&mut socket).starts_with(b"PUT "));
            // Do not drain the payload: exercise actual socket backpressure.
            ready_tx.send(()).unwrap();
            stop_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        let options = Options::default();
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = options.cancelled.clone();
        let cancel = std::thread::spawn(move || {
            ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            let start = Instant::now();
            flag.store(true, Ordering::Release);
            start
        });
        let dropped = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let reader = lfs_pool::Reader {
            inner: TrackedReader {
                dropped: dropped.clone(),
                reads: reads.clone(),
            },
            options: options.clone(),
            stopped: stopped.clone(),
        };
        let size = 128 * 1024 * 1024u64;
        let result = Response::send(
            client(Duration::from_secs(5))
                .put(url)
                .header(reqwest::header::CONTENT_LENGTH, size)
                .body(upload_body(reader.take(size))),
            &options,
            stopped,
        );
        assert!(result.is_err());
        assert!(
            dropped.load(Ordering::Acquire),
            "Upload reader outlived request cancellation"
        );
        assert!(cancel.join().unwrap().elapsed() < Duration::from_millis(500));
        let before = reads.load(Ordering::Relaxed);
        assert!(before > 0 && before < 2048, "Upload was buffered in full");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            reads.load(Ordering::Relaxed),
            before,
            "Reader ran after request returned"
        );
        stop_tx.send(()).unwrap();
        server.join().unwrap();
    }

    #[test]
    fn body_polling_preserves_deadline_and_slow_success() {
        for stall in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/object", listener.local_addr().unwrap());
            let (stop_tx, stop_rx) = mpsc::channel();
            let server = std::thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                headers(&mut socket);
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\n")
                    .unwrap();
                if !stall {
                    for _ in 0..6 {
                        std::thread::sleep(Duration::from_millis(30));
                        socket.write_all(b"x").unwrap();
                        socket.flush().unwrap();
                    }
                }
                stop_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            });
            let options = Options::default();
            let start = Instant::now();
            let mut response = Response::send(
                client(if stall {
                    Duration::from_millis(150)
                } else {
                    Duration::from_secs(5)
                })
                .get(url),
                &options,
                Arc::new(AtomicBool::new(false)),
            )
            .unwrap();
            let mut bytes = vec![];
            let result = response.read_to_end(&mut bytes);
            if stall {
                assert!(result.is_err());
                assert!(start.elapsed() < Duration::from_secs(2));
            } else {
                assert!(result.is_ok());
                assert_eq!(bytes, b"xxxxxx");
                assert!(start.elapsed() >= Duration::from_millis(180));
            }
            drop(response);
            stop_tx.send(()).unwrap();
            server.join().unwrap();
        }
    }
}
