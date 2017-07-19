// Copyright 2016 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "apps/modular/src/device_runner/user_controller_impl.h"

#include <memory>
#include <utility>

#include "application/lib/app/connect.h"
#include "apps/modular/lib/fidl/array_to_string.h"

namespace modular {

UserControllerImpl::UserControllerImpl(
    app::ApplicationLauncher* const application_launcher,
    AppConfigPtr user_runner,
    AppConfigPtr user_shell,
    AppConfigPtr story_shell,
    fidl::InterfaceHandle<auth::TokenProviderFactory> token_provider_factory,
    auth::AccountPtr account,
    fidl::InterfaceRequest<mozart::ViewOwner> view_owner_request,
    fidl::InterfaceRequest<UserController> user_controller_request,
    DoneCallback done)
    : user_context_binding_(this),
      user_controller_binding_(this, std::move(user_controller_request)),
      done_(done) {
  // 1. Launch UserRunner in the current environment.
  user_runner_.reset(
      new AppClient<UserRunner>(application_launcher, std::move(user_runner)));

  // 2. Initialize the UserRunner service.
  user_runner_->primary_service()->Initialize(
      std::move(account), std::move(user_shell), std::move(story_shell),
      std::move(token_provider_factory), user_context_binding_.NewBinding(),
      std::move(view_owner_request));
}

// |UserController|
void UserControllerImpl::Logout(const LogoutCallback& done) {
  FTL_LOG(INFO) << "UserController::Logout()";
  logout_response_callbacks_.push_back(done);
  if (logout_response_callbacks_.size() > 1) {
    return;
  }

  // This should prevent us from receiving any further requests.
  user_controller_binding_.Unbind();
  user_context_binding_.Unbind();

  user_runner_->AppTerminate([this] {
    for (const auto& done : logout_response_callbacks_) {
      done();
    }
    // We announce |OnLogout| only at point just before deleting ourselves,
    // so we can avoid any race conditions that may be triggered by |Shutdown|
    // (which in-turn will call this |Logout| since we have not completed yet).
    user_watchers_.ForAllPtrs(
        [](UserWatcher* watcher) { watcher->OnLogout(); });
    done_(this);
  });
}

// |UserController|
void UserControllerImpl::Watch(fidl::InterfaceHandle<UserWatcher> watcher) {
  user_watchers_.AddInterfacePtr(UserWatcherPtr::Create(std::move(watcher)));
}

// |UserContext|
// TODO(alhaad): Reconcile UserContext.Logout() and UserControllerImpl.Logout().
void UserControllerImpl::Logout() {
  FTL_LOG(INFO) << "UserContext::Logout()";
  Logout([] {});
}

}  // namespace modular
