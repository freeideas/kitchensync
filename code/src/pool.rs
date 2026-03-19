use std::sync::{Arc, Condvar, Mutex};

use crate::config::{PeerConfig, PeerUrl, UrlScheme};
use crate::local_peer::LocalConnection;
use crate::logging::Logger;
use crate::peer::{PeerError, PeerFs};
use crate::sftp_peer::SftpConnection;

pub struct ConnectionPool {
    available: Mutex<Vec<Box<dyn PeerFs>>>,
    count: Mutex<usize>,
    max: usize,
    condvar: Condvar,
    urls: Vec<PeerUrl>,
    connection_timeout: u64,
    peer_name: String,
    logger: Arc<Logger>,
}

impl ConnectionPool {
    pub fn new(urls: Vec<PeerUrl>, max: usize, connection_timeout: u64, peer_name: String, logger: Arc<Logger>) -> Self {
        ConnectionPool {
            available: Mutex::new(Vec::new()),
            count: Mutex::new(0),
            max,
            condvar: Condvar::new(),
            urls,
            connection_timeout,
            peer_name,
            logger,
        }
    }

    pub fn acquire(&self) -> Result<PoolGuard<'_>, PeerError> {
        let mut avail = self.available.lock().unwrap();
        loop {
            if let Some(c) = avail.pop() {
                let in_use = {
                    let count = self.count.lock().unwrap();
                    *count - avail.len()
                };
                self.logger.trace(&format!(
                    "pool acquire peer={} connections={}/{}",
                    self.peer_name, in_use, self.max
                ));
                return Ok(PoolGuard {
                    pool: self,
                    conn: Some(c),
                });
            }
            let mut count = self.count.lock().unwrap();
            if *count < self.max {
                *count += 1;
                let in_use = *count - avail.len();
                drop(count);
                drop(avail);
                self.logger.trace(&format!(
                    "pool acquire peer={} connections={}/{}",
                    self.peer_name, in_use, self.max
                ));
                match self.create_connection() {
                    Ok(c) => {
                        return Ok(PoolGuard {
                            pool: self,
                            conn: Some(c),
                        });
                    }
                    Err(e) => {
                        let mut count = self.count.lock().unwrap();
                        *count -= 1;
                        return Err(e);
                    }
                }
            }
            drop(count);
            avail = self.condvar.wait(avail).unwrap();
        }
    }

    fn release(&self, conn: Box<dyn PeerFs>) {
        let mut avail = self.available.lock().unwrap();
        avail.push(conn);
        let count = self.count.lock().unwrap();
        let in_use = *count - avail.len();
        self.logger.trace(&format!(
            "pool release peer={} connections={}/{}",
            self.peer_name, in_use, self.max
        ));
        self.condvar.notify_one();
    }

    fn create_connection(&self) -> Result<Box<dyn PeerFs>, PeerError> {
        connect_to_urls(&self.urls, self.connection_timeout)
    }
}

pub struct PoolGuard<'a> {
    pool: &'a ConnectionPool,
    conn: Option<Box<dyn PeerFs>>,
}

impl<'a> PoolGuard<'a> {
    pub fn conn(&self) -> &dyn PeerFs {
        self.conn.as_ref().unwrap().as_ref()
    }
}

impl<'a> Drop for PoolGuard<'a> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            self.pool.release(conn);
        }
    }
}

/// Try each URL in order. First success wins.
pub fn connect_to_urls(urls: &[PeerUrl], timeout: u64) -> Result<Box<dyn PeerFs>, PeerError> {
    let mut last_err = PeerError::IoError("No URLs configured".to_string());
    for url in urls {
        match connect_url(url, timeout) {
            Ok(conn) => return Ok(conn),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

fn connect_url(url: &PeerUrl, timeout: u64) -> Result<Box<dyn PeerFs>, PeerError> {
    match url.scheme {
        UrlScheme::File => {
            let conn = LocalConnection::new(&url.path)?;
            Ok(Box::new(conn))
        }
        UrlScheme::Sftp => {
            let conn = SftpConnection::connect(url, timeout)?;
            Ok(Box::new(conn))
        }
    }
}

/// Represents a connected peer with a listing connection and a transfer pool.
pub struct ConnectedPeer {
    pub name: String,
    pub listing_conn: Box<dyn PeerFs>,
    pub pool: ConnectionPool,
}

impl ConnectedPeer {
    pub fn connect(
        config: &PeerConfig,
        max_connections: usize,
        connection_timeout: u64,
        logger: Arc<Logger>,
    ) -> Result<Self, PeerError> {
        let listing_conn = connect_to_urls(&config.urls, connection_timeout)?;
        let pool = ConnectionPool::new(
            config.urls.clone(),
            max_connections,
            connection_timeout,
            config.name.clone(),
            logger,
        );
        Ok(ConnectedPeer {
            name: config.name.clone(),
            listing_conn,
            pool,
        })
    }
}
