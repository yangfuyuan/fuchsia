// Copyright 2018 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "garnet/lib/ui/scenic/system.h"

#include "garnet/lib/ui/scenic/scenic.h"

namespace scenic {

SystemContext::SystemContext(component::ApplicationContext* app_context,
                             fxl::TaskRunner* task_runner,
                             Clock* clock)
    : app_context_(app_context), task_runner_(task_runner), clock_(clock) {
  FXL_DCHECK(app_context_);
  FXL_DCHECK(task_runner_);
  FXL_DCHECK(clock_);
}

SystemContext::SystemContext(SystemContext&& context)
    : SystemContext(context.app_context_,
                    context.task_runner_,
                    context.clock_) {
  auto& other_app_context =
      const_cast<component::ApplicationContext*&>(context.app_context_);
  auto& other_task_runner = const_cast<fxl::TaskRunner*&>(context.task_runner_);
  auto& other_clock = const_cast<Clock*&>(context.clock_);
  other_app_context = nullptr;
  other_task_runner = nullptr;
  other_clock = nullptr;
}

System::System(SystemContext context, bool initialized_after_construction)
    : initialized_(initialized_after_construction),
      context_(std::move(context)) {}

void System::SetToInitialized() {
  initialized_ = true;
  if (on_initialized_callback_) {
    on_initialized_callback_(this);
    on_initialized_callback_ = nullptr;
  }
}

System::~System() = default;

TempSystemDelegate::TempSystemDelegate(SystemContext context,
                                       bool initialized_after_construction)
    : System(std::move(context), initialized_after_construction) {}

}  // namespace scenic
