// Copyright 2018 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "garnet/lib/ui/sketchy/sketchy_system.h"

namespace scenic {

SketchySystem::SketchySystem(SystemContext context,
                             gfx::ScenicSystem* scenic_system)
    : System(std::move(context)), scenic_system_(scenic_system) {}

SketchySystem::~SketchySystem() = default;

std::unique_ptr<CommandDispatcher> SketchySystem::CreateCommandDispatcher(
    CommandDispatcherContext context) {
  return std::make_unique<SketchyCommandDispatcher>(std::move(context),
                                                    scenic_system_);
}

SketchyCommandDispatcher::SketchyCommandDispatcher(
    CommandDispatcherContext context,
    gfx::ScenicSystem* scenic_system)
    : CommandDispatcher(std::move(context)), scenic_system_(scenic_system) {
  FXL_DCHECK(scenic_system_);
}

SketchyCommandDispatcher::~SketchyCommandDispatcher() = default;

bool SketchyCommandDispatcher::ApplyCommand(const ui::Command& command) {
  FXL_CHECK(false) << "not implemented";
  return false;
}

}  // namespace scenic
