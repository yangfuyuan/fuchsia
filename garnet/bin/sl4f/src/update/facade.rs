// Copyright 2019 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use crate::common_utils::common::{get_proxy_or_connect, macros::parse_arg};
use anyhow::Error;
use fidl_fuchsia_update::{Initiator, ManagerMarker, ManagerProxy, Options};
use fidl_fuchsia_update_channelcontrol::{ChannelControlMarker, ChannelControlProxy};
use parking_lot::RwLock;
use serde_json::Value;

use super::types::{CheckNowResult, StateHelper};

/// Facade providing access to fuchsia.update FIDL interface.
#[derive(Debug)]
pub struct UpdateFacade {
    manager: RwLock<Option<ManagerProxy>>,
    channel_control: RwLock<Option<ChannelControlProxy>>,
}

impl UpdateFacade {
    pub fn new() -> Self {
        UpdateFacade { manager: RwLock::new(None), channel_control: RwLock::new(None) }
    }

    fn manager(&self) -> Result<ManagerProxy, Error> {
        get_proxy_or_connect::<ManagerMarker>(&self.manager)
    }

    fn channel_control(&self) -> Result<ChannelControlProxy, Error> {
        get_proxy_or_connect::<ChannelControlMarker>(&self.channel_control)
    }

    pub async fn get_state(&self) -> Result<StateHelper, Error> {
        Ok(StateHelper(self.manager()?.get_state().await?))
    }

    pub async fn check_now(&self, args: Value) -> Result<CheckNowResult, Error> {
        let service_initiated = parse_arg!(args, as_bool, "service-initiated").unwrap_or(false);
        let options = Options {
            initiator: Some(if service_initiated { Initiator::Service } else { Initiator::User }),
        };
        let check_started = self.manager()?.check_now(options, None).await?;
        Ok(CheckNowResult { check_started })
    }

    pub async fn get_current_channel(&self) -> Result<String, Error> {
        Ok(self.channel_control()?.get_current().await?)
    }

    pub async fn get_target_channel(&self) -> Result<String, Error> {
        Ok(self.channel_control()?.get_target().await?)
    }

    pub async fn set_target_channel(&self, args: Value) -> Result<(), Error> {
        let channel = parse_arg!(args, as_str, "channel")?;
        Ok(self.channel_control()?.set_target(channel).await?)
    }

    pub async fn get_channel_list(&self) -> Result<Vec<String>, Error> {
        Ok(self.channel_control()?.get_target_list().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fidl::endpoints::create_proxy_and_stream;
    use fidl_fuchsia_update::{CheckStartedResult, ManagerRequest, ManagerState, State};
    use fidl_fuchsia_update_channelcontrol::ChannelControlRequest;
    use fuchsia_async as fasync;
    use futures::prelude::*;
    use serde_json::json;

    #[fasync::run_singlethreaded(test)]
    async fn test_get_state() {
        let (proxy, mut stream) = create_proxy_and_stream::<ManagerMarker>().unwrap();
        let facade =
            UpdateFacade { manager: RwLock::new(Some(proxy)), channel_control: RwLock::new(None) };
        let facade_fut = async move {
            assert_eq!(
                facade.get_state().await.unwrap(),
                StateHelper(State {
                    state: Some(ManagerState::PerformingUpdate),
                    version_available: Some("1.0".to_string())
                })
            );
        };
        let stream_fut = async move {
            match stream.try_next().await {
                Ok(Some(ManagerRequest::GetState { responder })) => {
                    responder
                        .send(State {
                            state: Some(ManagerState::PerformingUpdate),
                            version_available: Some("1.0".to_string()),
                        })
                        .unwrap();
                }
                err => panic!("Err in request handler: {:?}", err),
            }
        };
        future::join(facade_fut, stream_fut).await;
    }

    #[fasync::run_singlethreaded(test)]
    async fn test_check_now() {
        let (proxy, mut stream) = create_proxy_and_stream::<ManagerMarker>().unwrap();
        let facade =
            UpdateFacade { manager: RwLock::new(Some(proxy)), channel_control: RwLock::new(None) };
        let facade_fut = async move {
            let args = json!({"service-initiated":true});
            assert_eq!(
                facade.check_now(args).await.unwrap(),
                CheckNowResult { check_started: CheckStartedResult::Started }
            );
        };
        let stream_fut = async move {
            match stream.try_next().await {
                Ok(Some(ManagerRequest::CheckNow { options, monitor, responder })) => {
                    assert_eq!(options.initiator, Some(Initiator::Service));
                    assert_eq!(monitor, None);
                    responder.send(CheckStartedResult::Started).unwrap();
                }
                err => panic!("Err in request handler: {:?}", err),
            }
        };
        future::join(facade_fut, stream_fut).await;
    }

    #[fasync::run_singlethreaded(test)]
    async fn test_get_current_channel() {
        let (proxy, mut stream) = create_proxy_and_stream::<ChannelControlMarker>().unwrap();
        let facade =
            UpdateFacade { manager: RwLock::new(None), channel_control: RwLock::new(Some(proxy)) };
        let facade_fut = async move {
            assert_eq!(facade.get_current_channel().await.unwrap(), "current-channel");
        };
        let stream_fut = async move {
            match stream.try_next().await {
                Ok(Some(ChannelControlRequest::GetCurrent { responder })) => {
                    responder.send("current-channel").unwrap();
                }
                err => panic!("Err in request handler: {:?}", err),
            }
        };
        future::join(facade_fut, stream_fut).await;
    }

    #[fasync::run_singlethreaded(test)]
    async fn test_get_target_channel() {
        let (proxy, mut stream) = create_proxy_and_stream::<ChannelControlMarker>().unwrap();
        let facade =
            UpdateFacade { manager: RwLock::new(None), channel_control: RwLock::new(Some(proxy)) };
        let facade_fut = async move {
            assert_eq!(facade.get_target_channel().await.unwrap(), "target-channel");
        };
        let stream_fut = async move {
            match stream.try_next().await {
                Ok(Some(ChannelControlRequest::GetTarget { responder })) => {
                    responder.send("target-channel").unwrap();
                }
                err => panic!("Err in request handler: {:?}", err),
            }
        };
        future::join(facade_fut, stream_fut).await;
    }

    #[fasync::run_singlethreaded(test)]
    async fn test_set_target_channel() {
        let (proxy, mut stream) = create_proxy_and_stream::<ChannelControlMarker>().unwrap();
        let facade =
            UpdateFacade { manager: RwLock::new(None), channel_control: RwLock::new(Some(proxy)) };
        let facade_fut = async move {
            let args = json!({"channel":"target-channel"});
            facade.set_target_channel(args).await.unwrap();
        };
        let stream_fut = async move {
            match stream.try_next().await {
                Ok(Some(ChannelControlRequest::SetTarget { channel, responder })) => {
                    assert_eq!(channel, "target-channel");
                    responder.send().unwrap();
                }
                err => panic!("Err in request handler: {:?}", err),
            }
        };
        future::join(facade_fut, stream_fut).await;
    }

    #[fasync::run_singlethreaded(test)]
    async fn test_get_channel_list() {
        let (proxy, mut stream) = create_proxy_and_stream::<ChannelControlMarker>().unwrap();
        let facade =
            UpdateFacade { manager: RwLock::new(None), channel_control: RwLock::new(Some(proxy)) };
        let facade_fut = async move {
            assert_eq!(facade.get_channel_list().await.unwrap(), vec!["channel1", "channel2"]);
        };
        let stream_fut = async move {
            match stream.try_next().await {
                Ok(Some(ChannelControlRequest::GetTargetList { responder })) => {
                    responder.send(&mut vec!["channel1", "channel2"].into_iter()).unwrap();
                }
                err => panic!("Err in request handler: {:?}", err),
            }
        };
        future::join(facade_fut, stream_fut).await;
    }
}
