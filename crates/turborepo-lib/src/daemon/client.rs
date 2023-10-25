use std::io;

use thiserror::Error;
use tonic::{Code, Status};
use tracing::info;
use turbopath::AbsoluteSystemPathBuf;

use self::proto::turbod_client::TurbodClient;
use super::{
    connector::{DaemonConnector, DaemonConnectorError},
    endpoint::SocketOpenError,
};
use crate::{get_version, globwatcher::HashGlobSetupError};

pub mod proto {
    tonic::include_proto!("turbodprotocol");
}

#[derive(Debug, Clone)]
pub struct DaemonClient<T: Clone> {
    client: TurbodClient<tonic::transport::Channel>,
    connect_settings: T,
}

impl<T: Clone> DaemonClient<T> {
    /// Interrogate the server for its version.
    #[tracing::instrument(skip(self))]
    pub(super) async fn handshake(&mut self) -> Result<(), DaemonError> {
        let _ret = self
            .client
            .hello(proto::HelloRequest {
                version: get_version().to_string(),
                // todo(arlyon): add session id
                ..Default::default()
            })
            .await?;

        Ok(())
    }

    /// Stops the daemon and closes the connection, returning
    /// the connection settings that were used to connect.
    pub async fn stop(mut self) -> Result<T, DaemonError> {
        info!("Stopping daemon");
        self.client.shutdown(proto::ShutdownRequest {}).await?;
        Ok(self.connect_settings)
    }
    pub async fn get_changed_outputs(
        &mut self,
        hash: String,
        output_globs: Vec<String>,
    ) -> Result<Vec<String>, DaemonError> {
        Ok(self
            .client
            .get_changed_outputs(proto::GetChangedOutputsRequest { hash, output_globs })
            .await?
            .into_inner()
            .changed_output_globs)
    }

    #[allow(dead_code)]
    pub async fn notify_outputs_written(
        &mut self,
        hash: String,
        output_globs: Vec<String>,
        output_exclusion_globs: Vec<String>,
        time_saved: u64,
    ) -> Result<(), DaemonError> {
        self.client
            .notify_outputs_written(proto::NotifyOutputsWrittenRequest {
                hash,
                output_globs,
                output_exclusion_globs,
                time_saved,
            })
            .await?;

        Ok(())
    }

    /// Get the status of the daemon.
    pub async fn status(&mut self) -> Result<proto::DaemonStatus, DaemonError> {
        self.client
            .status(proto::StatusRequest {})
            .await?
            .into_inner()
            .daemon_status
            .ok_or(DaemonError::MalformedResponse)
    }

    pub async fn discover_packages(
        &mut self,
    ) -> Result<Vec<crate::daemon::proto::PackageFiles>, DaemonError> {
        let response = self
            .client
            .discover_packages(proto::DiscoverPackagesRequest {})
            .await?
            .into_inner();

        tracing::info!("{:?}", response);

        Ok(response.package_files)
    }
}

impl DaemonClient<()> {
    pub fn new(client: TurbodClient<tonic::transport::Channel>) -> Self {
        Self {
            client,
            connect_settings: (),
        }
    }

    /// Augment the client with the connect settings, allowing it to be
    /// restarted.
    pub fn with_connect_settings(
        self,
        connect_settings: DaemonConnector,
    ) -> DaemonClient<DaemonConnector> {
        DaemonClient {
            client: self.client,
            connect_settings,
        }
    }
}

impl DaemonClient<DaemonConnector> {
    /// Stops the daemon, closes the connection, and opens a new connection.
    pub async fn restart(self) -> Result<DaemonClient<DaemonConnector>, DaemonError> {
        self.stop().await?.connect().await.map_err(Into::into)
    }

    pub fn pid_file(&self) -> &turbopath::AbsoluteSystemPathBuf {
        &self.connect_settings.pid_file
    }

    pub fn sock_file(&self) -> &turbopath::AbsoluteSystemPathBuf {
        &self.connect_settings.sock_file
    }
}

#[derive(Error, Debug)]
pub enum DaemonError {
    /// The server was connected but is now unavailable.
    #[error("server is unavailable")]
    Unavailable,
    #[error("error opening socket: {0}")]
    SocketOpen(#[from] SocketOpenError),
    /// The server is running a different version of turborepo.
    #[error("version mismatch")]
    VersionMismatch,
    /// There is an issue with the underlying grpc transport.
    #[error("bad grpc transport: {0}")]
    GrpcTransport(#[from] tonic::transport::Error),
    /// The daemon returned an unexpected status code.
    #[error("bad grpc status code: {0}")]
    GrpcFailure(tonic::Code),
    /// The daemon returned a malformed response.
    #[error("malformed response")]
    MalformedResponse,
    /// There was an issue connecting to the daemon.
    #[error("unable to connect: {0}")]
    DaemonConnect(#[from] DaemonConnectorError),
    /// The timeout specified was invalid.
    #[error("invalid timeout specified ({0})")]
    #[allow(dead_code)]
    InvalidTimeout(String),
    /// The server is unable to start file watching.
    #[error("unable to start file watching")]
    SetupFileWatching(#[from] HashGlobSetupError),

    #[error("unable to display output: {0}")]
    DisplayError(#[from] serde_json::Error),

    #[error("unable to construct log file name: {0}")]
    InvalidLogFile(#[from] time::Error),

    #[error("unable to complete daemon clean")]
    CleanFailed,

    #[error("failed to setup cookie dir {1}: {0}")]
    CookieDir(io::Error, AbsoluteSystemPathBuf),
}

impl From<Status> for DaemonError {
    fn from(status: Status) -> DaemonError {
        match status.code() {
            Code::FailedPrecondition | Code::Unimplemented => DaemonError::VersionMismatch,
            Code::Unavailable => DaemonError::Unavailable,
            c => DaemonError::GrpcFailure(c),
        }
    }
}
