// Copyright 2016 The Fuchsia Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

#include "escher/impl/command_buffer.h"

#include "escher/impl/mesh_impl.h"
#include "escher/impl/resource.h"
#include "escher/renderer/framebuffer.h"
#include "escher/renderer/image.h"

#include "ftl/macros.h"

namespace escher {
namespace impl {

CommandBuffer::CommandBuffer(vk::Device device,
                             vk::CommandBuffer command_buffer,
                             vk::Fence fence)
    : device_(device), command_buffer_(command_buffer), fence_(fence) {}

CommandBuffer::~CommandBuffer() {
  FTL_DCHECK(!is_active_ && !is_submitted_);
  // Owner is responsible for destroying command buffer and fence.
}

void CommandBuffer::Begin() {
  FTL_DCHECK(!is_active_ && !is_submitted_);
  is_active_ = true;
  auto result = command_buffer_.begin(vk::CommandBufferBeginInfo());
  FTL_DCHECK(result == vk::Result::eSuccess);
}

bool CommandBuffer::Submit(vk::Queue queue,
                           CommandBufferFinishedCallback callback) {
  FTL_DCHECK(is_active_ && !is_submitted_);
  is_submitted_ = true;
  callback_ = std::move(callback);

  auto end_command_buffer_result = command_buffer_.end();
  FTL_DCHECK(end_command_buffer_result == vk::Result::eSuccess);

  vk::SubmitInfo submit_info;
  submit_info.commandBufferCount = 1;
  submit_info.pCommandBuffers = &command_buffer_;
  submit_info.waitSemaphoreCount = wait_semaphores_for_submit_.size();
  submit_info.pWaitSemaphores = wait_semaphores_for_submit_.data();
  submit_info.pWaitDstStageMask = wait_semaphore_stages_.data();
  submit_info.signalSemaphoreCount = signal_semaphores_for_submit_.size();
  submit_info.pSignalSemaphores = signal_semaphores_for_submit_.data();

  auto submit_result = queue.submit(1, &submit_info, fence_);
  if (submit_result != vk::Result::eSuccess) {
    FTL_LOG(WARNING) << "failed queue submission: " << to_string(submit_result);
    // Clearing these flags allows Retire() to make progress.
    is_active_ = is_submitted_ = false;
    return false;
  }
  return true;
}

void CommandBuffer::AddWaitSemaphore(SemaphorePtr semaphore,
                                     vk::PipelineStageFlags stage) {
  if (semaphore) {
    // Build up list that will be used when frame is submitted.
    wait_semaphores_for_submit_.push_back(semaphore->value());
    wait_semaphore_stages_.push_back(stage);
    // Retain semaphore to ensure that it doesn't prematurely die.
    wait_semaphores_.push_back(std::move(semaphore));
  }
}

void CommandBuffer::AddSignalSemaphore(SemaphorePtr semaphore) {
  if (semaphore) {
    // Build up list that will be used when frame is submitted.
    signal_semaphores_for_submit_.push_back(semaphore->value());
    // Retain semaphore to ensure that it doesn't prematurely die.
    signal_semaphores_.push_back(std::move(semaphore));
  }
}

void CommandBuffer::AddUsedResource(ResourcePtr resource) {
  used_resources_.push_back(std::move(resource));
}

void CommandBuffer::DrawMesh(const MeshPtr& mesh) {
  AddUsedResource(mesh);

  AddWaitSemaphore(mesh->TakeWaitSemaphore(),
                   vk::PipelineStageFlagBits::eVertexInput);

  auto mesh_impl = static_cast<MeshImpl*>(mesh.get());
  vk::Buffer vbo = mesh_impl->vertex_buffer();
  vk::DeviceSize vbo_offset = mesh_impl->vertex_buffer_offset();
  uint32_t vbo_binding = mesh_impl->vertex_buffer_binding();
  command_buffer_.bindVertexBuffers(vbo_binding, 1, &vbo, &vbo_offset);
  command_buffer_.bindIndexBuffer(mesh_impl->index_buffer(),
                                  mesh_impl->vertex_buffer_offset(),
                                  vk::IndexType::eUint32);
  command_buffer_.drawIndexed(mesh_impl->num_indices, 1, 0, 0, 0);
}

void CommandBuffer::CopyImage(ImagePtr src_image,
                              ImagePtr dst_image,
                              vk::ImageLayout src_layout,
                              vk::ImageLayout dst_layout,
                              vk::ImageCopy* region) {
  command_buffer_.copyImage(src_image->get(), src_layout, dst_image->get(),
                            dst_layout, 1, region);
  AddUsedResource(std::move(src_image));
  AddUsedResource(std::move(dst_image));
}

void CommandBuffer::TransitionImageLayout(ImagePtr image,
                                          vk::ImageLayout old_layout,
                                          vk::ImageLayout new_layout) {
  vk::ImageMemoryBarrier barrier;
  barrier.oldLayout = old_layout;
  barrier.newLayout = new_layout;
  barrier.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
  barrier.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
  barrier.image = image->get();

  if (new_layout == vk::ImageLayout::eDepthStencilAttachmentOptimal) {
    barrier.subresourceRange.aspectMask = vk::ImageAspectFlagBits::eDepth;
    if (image->HasStencilComponent()) {
      barrier.subresourceRange.aspectMask |= vk::ImageAspectFlagBits::eStencil;
    }
  } else {
    barrier.subresourceRange.aspectMask = vk::ImageAspectFlagBits::eColor;
  }

  // TODO: assert that image only has one level.
  barrier.subresourceRange.baseMipLevel = 0;
  barrier.subresourceRange.levelCount = 1;
  barrier.subresourceRange.baseArrayLayer = 0;
  barrier.subresourceRange.layerCount = 1;

  if (old_layout == vk::ImageLayout::ePreinitialized &&
      new_layout == vk::ImageLayout::eTransferSrcOptimal) {
    barrier.srcAccessMask = vk::AccessFlagBits::eHostWrite;
    barrier.dstAccessMask = vk::AccessFlagBits::eTransferRead;
  } else if (old_layout == vk::ImageLayout::ePreinitialized &&
             new_layout == vk::ImageLayout::eTransferDstOptimal) {
    barrier.srcAccessMask = vk::AccessFlagBits::eHostWrite;
    barrier.dstAccessMask = vk::AccessFlagBits::eTransferWrite;
  } else if (old_layout == vk::ImageLayout::eTransferDstOptimal &&
             new_layout == vk::ImageLayout::eShaderReadOnlyOptimal) {
    barrier.srcAccessMask = vk::AccessFlagBits::eTransferWrite;
    barrier.dstAccessMask = vk::AccessFlagBits::eShaderRead;
  } else if (old_layout == vk::ImageLayout::eUndefined &&
             new_layout == vk::ImageLayout::eDepthStencilAttachmentOptimal) {
    barrier.dstAccessMask = vk::AccessFlagBits::eDepthStencilAttachmentRead |
                            vk::AccessFlagBits::eDepthStencilAttachmentWrite;
  } else {
    FTL_LOG(ERROR) << "Unsupported layout transition from: "
                   << to_string(old_layout) << " to: " << to_string(new_layout);
    FTL_DCHECK(false);
    return;
  }
  command_buffer_.pipelineBarrier(vk::PipelineStageFlagBits::eTopOfPipe,
                                  vk::PipelineStageFlagBits::eTopOfPipe,
                                  vk::DependencyFlags(), 0, nullptr, 0, nullptr,
                                  1, &barrier);

  AddUsedResource(std::move(image));
}

void CommandBuffer::BeginRenderPass(
    vk::RenderPass render_pass,
    const FramebufferPtr& framebuffer,
    const std::vector<vk::ClearValue>& clear_values) {
  uint32_t width = framebuffer->width();
  uint32_t height = framebuffer->height();

  vk::RenderPassBeginInfo info;
  info.renderPass = render_pass;
  info.renderArea.offset.x = 0;
  info.renderArea.offset.y = 0;
  info.renderArea.extent.width = width;
  info.renderArea.extent.height = height;
  info.clearValueCount = static_cast<uint32_t>(clear_values.size());
  info.pClearValues = clear_values.data();
  info.framebuffer = framebuffer->framebuffer();

  command_buffer_.beginRenderPass(&info, vk::SubpassContents::eInline);

  vk::Viewport viewport;
  viewport.width = static_cast<float>(width);
  viewport.height = static_cast<float>(height);
  viewport.minDepth = static_cast<float>(0.0f);
  viewport.maxDepth = static_cast<float>(1.0f);
  command_buffer_.setViewport(0, 1, &viewport);

  // TODO: probably unnecessary?
  vk::Rect2D scissor;
  scissor.extent.width = width;
  scissor.extent.height = height;
  scissor.offset.x = 0;
  scissor.offset.y = 0;
  command_buffer_.setScissor(0, 1, &scissor);

  // TODO: should we retain the framebuffer?
}

bool CommandBuffer::Retire() {
  if (!is_active_ && !is_submitted_) {
    // Submission failed, so proceed with cleanup.
  } else {
    FTL_DCHECK(is_active_ && is_submitted_);
    // Check if fence has been reached.
    auto fence_status = device_.getFenceStatus(fence_);
    if (fence_status == vk::Result::eNotReady) {
      // Fence has not been reached; try again later.
      return false;
    }
  }
  is_active_ = is_submitted_ = false;
  device_.resetFences(1, &fence_);

  used_resources_.clear();

  if (callback_) {
    callback_();
    callback_ = nullptr;
  }

  // TODO: move semaphores to pool for reuse?
  wait_semaphores_.clear();
  wait_semaphores_for_submit_.clear();
  wait_semaphore_stages_.clear();
  signal_semaphores_.clear();
  signal_semaphores_for_submit_.clear();

  auto result = command_buffer_.reset(vk::CommandBufferResetFlags());
  FTL_DCHECK(result == vk::Result::eSuccess);

  return true;
}

}  // namespace impl
}  // namespace escher
