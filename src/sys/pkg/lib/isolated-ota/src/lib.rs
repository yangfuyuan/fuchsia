// Copyright 2020 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod cache;
mod omaha;
mod pkgfs;
mod resolver;
mod updater;

use {
    crate::{cache::Cache, pkgfs::Pkgfs, resolver::Resolver, updater::Updater},
    fidl::endpoints::{ClientEnd, Proxy, ServerEnd},
    fidl_fuchsia_io::{DirectoryMarker, DirectoryProxy},
    fuchsia_async as fasync,
    std::sync::Arc,
    thiserror::Error,
};

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("error launching pkgfs")]
    PkgfsLaunchError(#[source] anyhow::Error),

    #[error("error launching pkg-cache")]
    PkgCacheLaunchError(#[source] anyhow::Error),

    #[error("error launching pkg-resolver")]
    PkgResolverLaunchError(#[source] anyhow::Error),

    #[error("error launching system-updater and installing update")]
    InstallError(#[source] anyhow::Error),

    #[error("error setting up resources")]
    FidlError(#[source] fidl::Error),

    #[error("IO error occurred")]
    IoError(#[source] std::io::Error),
}

pub struct OmahaConfig {
    /// The app_id to use for Omaha.
    pub app_id: String,
    /// The URL of the Omaha server.
    pub server_url: String,
}

/// Installs all packages and writes the Fuchsia ZBI from the latest build on the given channel.
///
/// If successful, a reboot should be the only thing necessary to boot Fuchsia.
///
/// # Arguments
/// * `blobfs` - The root directory of the blobfs we are installing to. The blobfs must work, but
///     there is no requirement on the state of any blobs (i.e. an empty blobfs, or one with missing or
///     corrupt blobs is ok)
/// * `paver_connector` - a directory which contains a service file named fuchsia.paver.Paver
/// * `repository_config_file` - A folder containing a json-serialized fidl_fuchsia_pkg_ext::RepositoryConfigs file
/// * `channel_name` - The channel to update from.
/// * `board_name` - Board name to pass to the system updater.
/// * `version` - Current version installed
/// * `omaha_cfg` - The |OmahaConfig| to use for Omaha. If None, the update will not use Omaha to
///     determine the updater URL.
pub async fn download_and_apply_update(
    blobfs: ClientEnd<DirectoryMarker>,
    paver_connector: ClientEnd<DirectoryMarker>,
    repository_config_file: std::fs::File,
    channel_name: &str,
    board_name: &str,
    version: &str,
    omaha_cfg: Option<OmahaConfig>,
) -> Result<(), UpdateError> {
    let blobfs_proxy = DirectoryProxy::from_channel(
        fasync::Channel::from_channel(blobfs.into_channel()).map_err(UpdateError::IoError)?,
    );
    let (blobfs_clone, remote) =
        fidl::endpoints::create_endpoints::<DirectoryMarker>().map_err(UpdateError::FidlError)?;
    blobfs_proxy
        .clone(fidl_fuchsia_io::CLONE_FLAG_SAME_RIGHTS, ServerEnd::from(remote.into_channel()))
        .map_err(UpdateError::FidlError)?;

    let pkgfs = Pkgfs::launch(blobfs_clone).map_err(UpdateError::PkgfsLaunchError)?;
    let cache = Arc::new(Cache::launch(&pkgfs).map_err(UpdateError::PkgCacheLaunchError)?);
    let resolver = Arc::new(
        Resolver::launch(&pkgfs, Arc::clone(&cache), repository_config_file, channel_name)
            .map_err(UpdateError::PkgResolverLaunchError)?,
    );

    let (blobfs_clone, remote) =
        fidl::endpoints::create_endpoints::<DirectoryMarker>().map_err(UpdateError::FidlError)?;
    blobfs_proxy
        .clone(fidl_fuchsia_io::CLONE_FLAG_SAME_RIGHTS, ServerEnd::from(remote.into_channel()))
        .map_err(UpdateError::FidlError)?;

    if let Some(cfg) = omaha_cfg {
        omaha::install_update(
            blobfs_clone,
            paver_connector,
            cache,
            resolver,
            board_name.to_owned(),
            cfg.app_id,
            cfg.server_url,
            version.to_owned(),
            channel_name.to_owned(),
        )
        .await
        .map_err(UpdateError::InstallError)?;
    } else {
        Updater::launch(blobfs_clone, paver_connector, cache, resolver, &board_name, None)
            .await
            .map_err(UpdateError::InstallError)?;
    }
    Ok(())
}
